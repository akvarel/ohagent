//! Opt-in outbound publishing. Coalesces changes into the latest snapshot, never a queue.
use crate::{valid_id, ProgressStore};
use std::time::Duration;

pub struct PublisherConfig {
    pub url: reqwest::Url,
    pub token: String,
    pub tenant_id: String,
    pub source_id: String,
}
impl PublisherConfig {
    pub fn new(
        url: &str,
        token: String,
        tenant_id: String,
        source_id: String,
    ) -> Result<Self, &'static str> {
        let url = reqwest::Url::parse(url).map_err(|_| "invalid progress URL")?;
        let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        if (url.scheme() != "https" && !(url.scheme() == "http" && loopback))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/api/progress/snapshots"
        {
            return Err("progress URL must be HTTPS (HTTP only on loopback), without credentials or query, at /api/progress/snapshots");
        }
        if token.len() < 24 || !valid_id(&tenant_id) || !valid_id(&source_id) {
            return Err("progress publishing needs an explicit tenant, source and existing token of at least 24 bytes");
        }
        Ok(Self {
            url,
            token,
            tenant_id,
            source_id,
        })
    }
    pub fn from_env() -> Result<Option<Self>, &'static str> {
        let Ok(url) = std::env::var("OHAGENT_PROGRESS_REMOTE_URL") else {
            return Ok(None);
        };
        let required =
            |name| std::env::var(name).map_err(|_| "missing progress publisher configuration");
        Self::new(
            &url,
            required("OHAGENT_PROGRESS_PUBLISH_TOKEN")?,
            required("OHAGENT_PROGRESS_TENANT_ID")?,
            required("OHAGENT_PROGRESS_SOURCE_ID")?,
        )
        .map(Some)
    }
}

pub async fn publish_once(
    client: &reqwest::Client,
    config: &PublisherConfig,
    store: &ProgressStore,
    sequence: u64,
) -> Result<(), reqwest::Error> {
    client
        .post(config.url.clone())
        .bearer_auth(&config.token)
        .json(&store.snapshot(&config.tenant_id, &config.source_id, sequence))
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

pub async fn run(config: PublisherConfig, store: ProgressStore) {
    // Redirects must never forward credentials to an alternate destination.
    let client = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(8))
        .build()
    {
        Ok(client) => client,
        Err(_) => {
            tracing::warn!("Progress publisher client unavailable");
            return;
        }
    };
    let mut sequence = 0u64;
    let mut delay = 5;
    loop {
        sequence = sequence.saturating_add(1);
        match publish_once(&client, &config, &store, sequence).await {
            Ok(()) => delay = 5,
            Err(_) => {
                // Do not log reqwest errors, which can contain configured URLs.
                tracing::warn!("Progress publication failed; latest snapshot will be retried");
                delay = (delay * 2).min(60);
            }
        }
        tokio::time::sleep(Duration::from_secs(delay)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn destinations_and_scope_are_validated() {
        let make =
            |url| PublisherConfig::new(url, "x".repeat(32), "tenant".into(), "desktop".into());
        assert!(make("https://monitor.example/api/progress/snapshots").is_ok());
        assert!(make("http://127.0.0.1:9191/api/progress/snapshots").is_ok());
        for url in [
            "http://monitor.example/api/progress/snapshots",
            "https://a:b@monitor.example/api/progress/snapshots",
            "https://monitor.example/api/progress/snapshots?key=secret",
            "https://monitor.example/other",
        ] {
            assert!(make(url).is_err());
        }
        assert!(PublisherConfig::new(
            "https://monitor.example/api/progress/snapshots",
            "x".repeat(32),
            "".into(),
            "desktop".into()
        )
        .is_err());
    }
}
