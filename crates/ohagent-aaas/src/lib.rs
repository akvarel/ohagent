//! Durable tenant-scoped AaaS pilot orchestration. Jcode execution is injected via Runtime.
mod auth;
mod http;
mod model;
mod runtime;
mod store;

pub use auth::OidcConfig;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
pub use http::router;
pub use model::*;
pub use runtime::{EventSink, Runtime};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc};

#[derive(Clone, Debug)]
pub struct Config {
    pub database_path: PathBuf,
    pub oidc: Option<OidcConfig>,
}
#[derive(Clone)]
pub struct State {
    pub(crate) store: Arc<store::Store>,
    pub(crate) authorizer: Option<Arc<dyn auth::Authorizer>>,
    pub(crate) public_auth: Option<Value>,
    pub(crate) runtime: Option<Arc<dyn Runtime>>,
    pub(crate) runtime_slots: Arc<tokio::sync::Semaphore>,
    pub(crate) stream_slots: Arc<tokio::sync::Semaphore>,
}
impl State {
    pub async fn open(
        config: Config,
        runtime: Option<Arc<dyn Runtime>>,
    ) -> Result<Self, BackendError> {
        let store = Arc::new(store::Store::open(&config.database_path)?);
        let (authorizer, public_auth) = if let Some(config) = config.oidc {
            let oidc = auth::Oidc::discover(config).await?;
            let mut public = json!({"issuer":oidc.config.issuer,"client_id":oidc.config.client_id,"authorization_endpoint":oidc.metadata.authorization_endpoint,"token_endpoint":oidc.metadata.token_endpoint});
            if let Some(logout) = &oidc.metadata.logout_endpoint {
                public["logout_endpoint"] = json!(logout);
            }
            (Some(oidc as Arc<dyn auth::Authorizer>), Some(public))
        } else {
            (None, None)
        };
        Ok(Self {
            store,
            authorizer,
            public_auth,
            runtime,
            runtime_slots: Arc::new(tokio::sync::Semaphore::new(8)),
            stream_slots: Arc::new(tokio::sync::Semaphore::new(64)),
        })
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum BackendError {
    Invalid,
    Unauthorized,
    Unavailable,
    NotFound,
    Conflict,
    Fenced,
    Cursor,
    Storage,
    Capacity,
}
impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code())
    }
}
impl std::error::Error for BackendError {}
impl BackendError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Invalid => "invalid_request",
            Self::Unauthorized => "unauthorized",
            Self::Unavailable => "aaas_unavailable",
            Self::NotFound => "not_found",
            Self::Conflict => "version_or_idempotency_conflict",
            Self::Fenced => "producer_fenced",
            Self::Cursor => "reset_required",
            Self::Storage => "storage_unavailable",
            Self::Capacity => "pilot_capacity_reached",
        }
    }
}
impl From<rusqlite::Error> for BackendError {
    fn from(_: rusqlite::Error) -> Self {
        Self::Storage
    }
}
impl From<serde_json::Error> for BackendError {
    fn from(_: serde_json::Error) -> Self {
        Self::Storage
    }
}
impl IntoResponse for BackendError {
    fn into_response(self) -> Response {
        let status = match self {
            Self::Invalid => StatusCode::BAD_REQUEST,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Conflict | Self::Fenced | Self::Cursor => StatusCode::CONFLICT,
            Self::Storage => StatusCode::SERVICE_UNAVAILABLE,
            Self::Capacity => StatusCode::TOO_MANY_REQUESTS,
        };
        (status, Json(json!({"error":{"code":self.code()}}))).into_response()
    }
}
#[cfg(test)]
mod tests;

// Compile the exact production adapter into the private test harness. This
// avoids a second executor, a dependency cycle, and public test-auth injection.
#[cfg(test)]
extern crate self as ohagent_aaas;
#[cfg(test)]
#[path = "../../ohagent-daemon/src/aaas.rs"]
mod live_adapter;
