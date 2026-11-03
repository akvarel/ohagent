//! Standalone restricted service, using the same router embedded in the ohAgent daemon.
use ohagent_document_api::{router, DocumentApiState};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = std::env::var("OHAGENT_API_KEY")?;
    if api_key.trim().len() < 24 {
        return Err("OHAGENT_API_KEY must contain at least 24 characters".into());
    }
    let address =
        std::env::var("OHAGENT_DOCUMENT_BIND").unwrap_or_else(|_| "127.0.0.1:9092".into());
    let listener = tokio::net::TcpListener::bind(&address).await?;
    let app = router(DocumentApiState::from_env(api_key));
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
