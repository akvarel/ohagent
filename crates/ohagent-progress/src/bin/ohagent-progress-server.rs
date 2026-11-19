//! Read-only remote monitor. Does not host any agent execution/admin endpoints.
use axum::http::{header, HeaderValue};
use axum::{response::Redirect, routing::get};
use ohagent_progress::receiver::Receiver;
use std::error::Error;
use tower_http::{
    services::{ServeDir, ServeFile},
    set_header::SetResponseHeaderLayer,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let required = |key| {
        std::env::var(key).map_err(|_|format!("Missing {key}. Configure existing approved credentials, never pass them on the command line."))
    };
    let receiver = Receiver::new(
        required("OHAGENT_PROGRESS_TENANT_ID")?,
        required("OHAGENT_PROGRESS_VIEW_TOKEN")?,
        required("OHAGENT_PROGRESS_PUBLISH_TOKEN")?,
    )?;
    let dir = std::env::var("OHAGENT_PROGRESS_WEB_ROOT")
        .unwrap_or_else(|_| "target/dashboard-dist".into());
    if !std::path::Path::new(&dir).join("index.html").is_file() {
        return Err("Dashboard build not found. Build crates/ohagent-dashboard and configure OHAGENT_PROGRESS_WEB_ROOT.".into());
    }
    let app = receiver.router()
        .route("/", get(|| async { Redirect::temporary("/progress") }))
        .route_service("/progress", ServeFile::new(std::path::Path::new(&dir).join("index.html")))
        .nest_service("/assets", ServeDir::new(std::path::Path::new(&dir).join("assets")))
        .layer(SetResponseHeaderLayer::overriding(header::CACHE_CONTROL, HeaderValue::from_static("no-store")))
        .layer(SetResponseHeaderLayer::overriding(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")))
        .layer(SetResponseHeaderLayer::overriding(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer")))
        .layer(SetResponseHeaderLayer::overriding(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'")));
    let bind = std::env::var("OHAGENT_PROGRESS_BIND").unwrap_or_else(|_| "127.0.0.1:9191".into());
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    eprintln!("Progress monitor listening on {}. Open /progress. Use a TLS reverse proxy for remote access.", listener.local_addr()?);
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
