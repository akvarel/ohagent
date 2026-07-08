//! ohAgent daemon — binary entry point.

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    ohagent_daemon::run().await
}
