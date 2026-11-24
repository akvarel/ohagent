use super::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use jsonwebtoken::{encode, EncodingKey, Header};
use ring::{
    rand::SystemRandom,
    signature::{Ed25519KeyPair, KeyPair},
};
use serde_json::{json, Value};

fn fixture() -> (Oidc, EncodingKey) {
    let rng = SystemRandom::new();
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
    let pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
    let keys=serde_json::from_value(json!({"keys":[{"kty":"OKP","crv":"Ed25519","x":URL_SAFE_NO_PAD.encode(pair.public_key().as_ref()),"kid":"fixture","alg":"EdDSA","use":"sig"}]})).unwrap();
    let oidc = Oidc {
        config: OidcConfig {
            issuer: "https://issuer.example".into(),
            audience: "aaas".into(),
            client_id: "public-client".into(),
            required_access_scope: None,
        },
        metadata: Metadata {
            issuer: "https://issuer.example".into(),
            authorization_endpoint: "https://issuer.example/authorize".into(),
            token_endpoint: "https://issuer.example/token".into(),
            logout_endpoint: None,
            jwks_uri: "https://issuer.example/jwks".into(),
        },
        client: reqwest::Client::new(),
        keys: Mutex::new((keys, Instant::now())),
    };
    (oidc, EncodingKey::from_ed_der(pkcs8.as_ref()))
}
fn claims() -> Value {
    json!({"iss":"https://issuer.example","aud":"aaas","sub":"alice","exp":chrono::Utc::now().timestamp()+3600})
}
fn token(key: &EncodingKey, claims: &Value) -> String {
    let mut header = Header::new(Algorithm::EdDSA);
    header.kid = Some("fixture".into());
    encode(&header, claims, key).unwrap()
}
#[tokio::test]
async fn asymmetric_jwt_validates_signature_issuer_audience_expiry_subject_and_key_intent() {
    let (oidc, key) = fixture();
    let valid = claims();
    let principal = oidc.authorize(&token(&key, &valid)).await.unwrap();
    assert_eq!(principal.tenant, hash("https://issuer.example\0alice"));
    for (field, value) in [
        ("iss", json!("https://evil.example")),
        ("aud", json!("other")),
        ("exp", json!(1)),
        ("sub", json!("")),
        ("nbf", json!(chrono::Utc::now().timestamp() + 3600)),
    ] {
        let mut bad = valid.clone();
        bad[field] = value;
        assert!(
            matches!(
                oidc.authorize(&token(&key, &bad)).await,
                Err(BackendError::Unauthorized)
            ),
            "{field}"
        );
    }
    for field in ["iss", "aud", "exp", "sub"] {
        let mut bad = valid.clone();
        bad.as_object_mut().unwrap().remove(field);
        assert!(
            matches!(
                oidc.authorize(&token(&key, &bad)).await,
                Err(BackendError::Unauthorized)
            ),
            "missing {field}"
        );
    }
    let (_, other_key) = fixture();
    assert!(matches!(
        oidc.authorize(&token(&other_key, &valid)).await,
        Err(BackendError::Unauthorized)
    ));
    let mut header = Header::new(Algorithm::EdDSA);
    header.kid = Some("unknown".into());
    assert!(matches!(
        oidc.authorize(&encode(&header, &valid, &key).unwrap())
            .await,
        Err(BackendError::Unauthorized)
    ));
    let mut header = Header::new(Algorithm::HS256);
    header.kid = Some("fixture".into());
    assert!(matches!(
        oidc.authorize(
            &encode(
                &header,
                &valid,
                &EncodingKey::from_secret(b"labelled test fixture")
            )
            .unwrap()
        )
        .await,
        Err(BackendError::Unauthorized)
    ));
    {
        let mut keys = oidc.keys.lock().await;
        keys.0.keys[0].common.public_key_use = Some(jsonwebtoken::jwk::PublicKeyUse::Encryption);
    }
    assert!(matches!(
        oidc.authorize(&token(&key, &valid)).await,
        Err(BackendError::Unauthorized)
    ));
}
#[tokio::test]
async fn authentik_profile_rejects_id_tokens_and_wrong_authorized_party() {
    let (mut oidc, key) = fixture();
    oidc.config.audience = oidc.config.client_id.clone();
    oidc.config.required_access_scope = Some("openid".into());
    let mut valid = claims();
    valid["aud"] = json!("public-client");
    valid["scope"] = json!("openid profile");
    valid["azp"] = json!("public-client");
    assert!(oidc.authorize(&token(&key, &valid)).await.is_ok());
    for field in ["scope", "azp"] {
        let mut bad = valid.clone();
        bad.as_object_mut().unwrap().remove(field);
        assert!(matches!(
            oidc.authorize(&token(&key, &bad)).await,
            Err(BackendError::Unauthorized)
        ));
    }
    for (field, value) in [
        ("scope", json!("not-openid profile")),
        ("scope", json!(["openid"])),
        ("azp", json!("other-client")),
    ] {
        let mut bad = valid.clone();
        bad[field] = value;
        assert!(matches!(
            oidc.authorize(&token(&key, &bad)).await,
            Err(BackendError::Unauthorized)
        ));
    }
}

#[tokio::test]
async fn insecure_issuer_configuration_is_rejected_without_network() {
    for issuer in [
        "http://issuer.example",
        "https://user:pass@issuer.example",
        "not-a-url",
    ] {
        assert!(matches!(
            Oidc::discover(OidcConfig {
                issuer: issuer.into(),
                audience: "aaas".into(),
                client_id: "client".into(),
                required_access_scope: None,
            })
            .await,
            Err(BackendError::Unavailable)
        ));
    }
}
