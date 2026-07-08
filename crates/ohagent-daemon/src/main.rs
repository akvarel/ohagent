//! ohAgent daemon — binary entry point.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    ohagent_daemon::run().await
}
