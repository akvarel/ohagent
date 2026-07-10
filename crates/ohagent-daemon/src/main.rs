//! ohAgent daemon — binary entry point.

/// ohAgent daemon entry point.
///
/// Uses multi-thread tokio runtime so that API server, Telegram gateway,
/// cron tasks, and background workers run concurrently without blocking.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    ohagent_daemon::run().await
}
