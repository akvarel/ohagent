//! ohAgent daemon — binary entry point.
//! Delegates to `ohagent_daemon::run()`.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    ohagent_daemon::run().await
}
