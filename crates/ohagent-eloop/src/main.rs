use clap::{Parser, Subcommand};
use ohagent_eloop::context::FilesystemRepositoryContextCollector;
use ohagent_eloop::{
    ContractVerifier, EloopEngine, ExecutionPolicy, FileRunStore, GoalRequest,
    RepositoryContextCollector, TemplateTaskCompiler,
};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "eloop",
    version,
    about = "ohAgent engineering task compiler and campaign foundation"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Compile a short engineering goal into task.json and task.md.
    Plan {
        /// Short natural-language engineering objective.
        goal: String,

        /// Repository to inspect.
        #[arg(long, default_value = ".")]
        repository: PathBuf,

        /// Directory where plan runs are persisted.
        #[arg(long, default_value = "engineering-runs")]
        output: PathBuf,

        /// Allow production deployment in the generated policy.
        #[arg(long, default_value_t = false)]
        allow_deploy: bool,

        /// Require a clean Git working tree before future execution.
        #[arg(long, default_value_t = false)]
        require_clean_git: bool,
    },

    /// Print detected repository context without creating a task.
    Context {
        #[arg(long, default_value = ".")]
        repository: PathBuf,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Plan {
            goal,
            repository,
            output,
            allow_deploy,
            require_clean_git,
        } => {
            let policy = ExecutionPolicy {
                deployment_allowed: allow_deploy,
                require_clean_git,
                ..ExecutionPolicy::default()
            };
            let engine = EloopEngine::new(
                FilesystemRepositoryContextCollector,
                TemplateTaskCompiler,
                ContractVerifier,
                FileRunStore::new(output),
            );
            let result = engine
                .plan_goal(GoalRequest {
                    goal,
                    repository,
                    policy,
                })
                .await?;
            println!("Plan created: {}", result.run_directory.display());
            println!("Task: {}", result.task.title);
            println!("Risk: {:?}", result.task.risk);
            println!(
                "Verification commands: {}",
                result.task.verification_commands.len()
            );
        }
        Command::Context { repository } => {
            let context = FilesystemRepositoryContextCollector.collect(&repository)?;
            println!("{}", serde_json::to_string_pretty(&context)?);
        }
    }
    Ok(())
}
