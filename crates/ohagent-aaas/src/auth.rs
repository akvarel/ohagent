#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;

use crate::{store::hash, BackendError};
use jsonwebtoken::{decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

#[derive(Clone, Debug)]
pub struct OidcConfig {
    pub issuer: String,
    pub audience: String,
    pub client_id: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Metadata {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    #[serde(
        rename = "end_session_endpoint",
        skip_serializing_if = "Option::is_none"
    )]
    pub logout_endpoint: Option<String>,
    #[serde(skip_serializing)]
    pub jwks_uri: String,
}
#[derive(Clone)]
pub(crate) struct Principal {
    pub tenant: String,
}
#[async_trait::async_trait]
pub(crate) trait Authorizer: Send + Sync {
    async fn authorize(&self, token: &str) -> Result<Principal, BackendError>;
}
pub(crate) struct Oidc {
    pub config: OidcConfig,
    pub metadata: Metadata,
    client: reqwest::Client,
    keys: Mutex<(JwkSet, Instant)>,
}
fn https(value: &str) -> bool {
    reqwest::Url::parse(value).is_ok_and(|u| {
        u.scheme() == "https"
            && u.host_str().is_some()
            && u.username().is_empty()
            && u.password().is_none()
            && u.fragment().is_none()
    })
}
async fn fetch<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
) -> Result<T, BackendError> {
    if !https(url) {
        return Err(BackendError::Unavailable);
    }
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|_| BackendError::Unavailable)?
        .error_for_status()
        .map_err(|_| BackendError::Unavailable)?;
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| BackendError::Unavailable)?
    {
        if body.len() + chunk.len() > 262_144 {
            return Err(BackendError::Unavailable);
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| BackendError::Unavailable)
}
impl Oidc {
    pub async fn discover(config: OidcConfig) -> Result<Arc<Self>, BackendError> {
        if !https(&config.issuer) || config.audience.is_empty() || config.client_id.is_empty() {
            return Err(BackendError::Unavailable);
        }
        let client = reqwest::Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| BackendError::Unavailable)?;
        let metadata: Metadata = fetch(
            &client,
            &format!(
                "{}/.well-known/openid-configuration",
                config.issuer.trim_end_matches('/')
            ),
        )
        .await?;
        if metadata.issuer != config.issuer
            || !https(&metadata.authorization_endpoint)
            || !https(&metadata.token_endpoint)
            || !https(&metadata.jwks_uri)
            || metadata.logout_endpoint.as_ref().is_some_and(|s| !https(s))
        {
            return Err(BackendError::Unavailable);
        }
        let keys = fetch(&client, &metadata.jwks_uri).await?;
        Ok(Arc::new(Self {
            config,
            metadata,
            client,
            keys: Mutex::new((keys, Instant::now())),
        }))
    }
}
#[derive(Deserialize)]
struct Claims {
    sub: String,
    iss: String,
}
#[async_trait::async_trait]
impl Authorizer for Oidc {
    async fn authorize(&self, token: &str) -> Result<Principal, BackendError> {
        if token.len() > 16_384 {
            return Err(BackendError::Unauthorized);
        }
        let header = decode_header(token).map_err(|_| BackendError::Unauthorized)?;
        if !matches!(
            header.alg,
            Algorithm::RS256
                | Algorithm::RS384
                | Algorithm::RS512
                | Algorithm::ES256
                | Algorithm::ES384
                | Algorithm::EdDSA
        ) {
            return Err(BackendError::Unauthorized);
        }
        let kid = header.kid.ok_or(BackendError::Unauthorized)?;
        let mut keys = self.keys.lock().await;
        // Refresh only on a bounded schedule. Unknown kid fails closed until refresh.
        if keys.1.elapsed() > Duration::from_secs(300) {
            keys.0 = fetch(&self.client, &self.metadata.jwks_uri).await?;
            keys.1 = Instant::now();
        }
        let jwk = keys.0.find(&kid).ok_or(BackendError::Unauthorized)?;
        if jwk
            .common
            .public_key_use
            .as_ref()
            .is_some_and(|usage| *usage != jsonwebtoken::jwk::PublicKeyUse::Signature)
            || jwk
                .common
                .key_operations
                .as_ref()
                .is_some_and(|ops| !ops.contains(&jsonwebtoken::jwk::KeyOperations::Verify))
            || jwk
                .common
                .key_algorithm
                .is_some_and(|alg| alg.to_string() != format!("{:?}", header.alg))
        {
            return Err(BackendError::Unauthorized);
        }
        let key = DecodingKey::from_jwk(jwk).map_err(|_| BackendError::Unauthorized)?;
        let mut validation = Validation::new(header.alg);
        validation.set_issuer(&[&self.config.issuer]);
        validation.set_audience(&[&self.config.audience]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        validation.validate_nbf = true;
        validation.leeway = 0;
        let claims = decode::<Claims>(token, &key, &validation)
            .map_err(|_| BackendError::Unauthorized)?
            .claims;
        if claims.sub.is_empty() || claims.sub.len() > 1024 {
            return Err(BackendError::Unauthorized);
        }
        Ok(Principal {
            tenant: hash(&format!("{}\0{}", claims.iss, claims.sub)),
        })
    }
}
