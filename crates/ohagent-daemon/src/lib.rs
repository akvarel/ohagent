//! ohagent-daemon: 24/7 daemon process for ohAgent.
//!
//! Runs as a long-lived process (systemd service or background).
//! Manages lifecycle, health checks, graceful shutdown,
//! hosts the messaging gateway, and serves the REST API.

mod api;
mod auth;
mod context_compressor;
mod metrics;
mod migrations;
mod openai_api;
mod plugin_api;
mod rate_limiter;
mod reasoning;
mod system_prompt;
mod webhooks;
mod ws;

use anyhow::Result;
use clap::Parser;
use std::sync::Arc;
use tokio::signal;
use tracing::info;
use tracing_subscriber::EnvFilter;

use ohagent_core::jcode_bridge::JcodeBridge;
use ohagent_core::vault::{resolve_secret, VaultClient};
use ohagent_gateway::platforms::telegram::TelegramAdapter;
use ohagent_gateway::platforms::whatsapp::WhatsAppAdapter;
use ohagent_gateway::platforms::slack::SlackAdapter;
use ohagent_gateway::adapter::PlatformAdapter;
use ohagent_memory::engine::MemoryEngine;
use ohagent_memory::models::MemoryConfig;
use ohagent_skills::registry::SkillRegistry;
use ohagent_skills::SkillConfig;
use crate::system_prompt::{PersistentInstructions, SystemPromptBuilder, SkillPrompt};
use jcode_provider_core::Provider;
use jcode_base::mcp::SharedMcpPool;
use ohagent_plugins::{PluginConfig, PluginManager};
use std::sync::Mutex as StdMutex;

/// Register external provider runtimes (OpenRouter, OpenAI-compatible profiles).
///
/// Must be called once at startup before creating any MultiProvider.
fn setup_provider_runtimes() {
    use jcode_base::provider::external;
    use jcode_provider_openrouter_runtime::OpenRouterProvider;

    external::register_openrouter_factory(|spec| {
        use external::OpenRouterRuntimeSpec;
        let provider: std::sync::Arc<dyn Provider> = match spec {
            OpenRouterRuntimeSpec::Default => std::sync::Arc::new(OpenRouterProvider::new()?),
            OpenRouterRuntimeSpec::OpenRouterApiKey => {
                std::sync::Arc::new(OpenRouterProvider::new_openrouter_api_key_runtime()?)
            }
            OpenRouterRuntimeSpec::CompatibleProfile(profile) => std::sync::Arc::new(
                OpenRouterProvider::new_openai_compatible_profile_runtime(profile)?,
            ),
            OpenRouterRuntimeSpec::NamedProfile { name, config } => std::sync::Arc::new(
                OpenRouterProvider::new_named_openai_compatible(&name, &config)?,
            ),
        };
        Ok(provider)
    });

    external::register_profile_catalog_refresh(
        jcode_provider_openrouter_runtime::maybe_schedule_openai_compatible_profile_catalog_refresh,
    );
    external::register_standard_openrouter_catalog_refresh(
        jcode_provider_openrouter_runtime::maybe_schedule_standard_openrouter_catalog_refresh,
    );

    tracing::info!("Provider runtimes registered (OpenRouter + compatible profiles)");
}

/// ohAgent — 24/7 personal AI agent built on Jcode.
#[derive(Parser, Debug)]
#[command(name = "ohagent", version, about)]
struct Cli {
    /// Path to configuration file
    #[arg(short, long, default_value = "~/.ohagent/config.toml")]
    config: String,

    /// Log level (trace, debug, info, warn, error)
    #[arg(short, long, default_value = "info")]
    log_level: String,

    /// Health check port
    #[arg(long, default_value = "9090")]
    health_port: u16,

    /// Enable Telegram gateway
    #[arg(long, default_value = "true")]
    telegram: bool,
}

/// Main daemon state.
struct Daemon {
    health_port: u16,
    enable_telegram: bool,
    shutdown: Arc<tokio::sync::Notify>,
    bridge: Arc<JcodeBridge>,
    memory: Option<Arc<MemoryEngine>>,
    skills: Option<Arc<SkillRegistry>>,
    usage: Option<Arc<ohagent_core::usage_tracker::UsageTracker>>,
    message_log: Option<Arc<ohagent_core::message_log::MessageLog>>,
    router: Option<Arc<std::sync::Mutex<ohagent_core::model_router::ModelRouter>>>,
    start_time: chrono::DateTime<chrono::Utc>,
    keys_path: String,
    vault: Arc<VaultClient>,
    auth_config: auth::AuthConfig,
    rate_limiter: Arc<rate_limiter::RateLimiter>,
    metrics: Arc<metrics::Metrics>,
    system_prompt_builder: Option<SystemPromptBuilder>,
    session_store: Option<Arc<ohagent_core::session_store::SessionStore>>,
    tool_registry: Option<Arc<ohagent_core::tools::ToolRegistry>>,
    push: Option<Arc<ohagent_core::push::PushService>>,
    scheduler: Option<Arc<ohagent_core::scheduler::Scheduler>>,
    whatsapp: Option<Arc<WhatsAppAdapter>>,
    slack: Option<Arc<SlackAdapter>>,
    /// Kept alive to own MCP server child processes (passed to bridge on startup).
    #[allow(dead_code)]
    mcp_pool: Option<Arc<SharedMcpPool>>,
    plugin_manager: Arc<StdMutex<PluginManager>>,
}

impl Daemon {
    fn new(health_port: u16, enable_telegram: bool) -> Result<Self> {
        // Register provider runtimes before creating any provider
        setup_provider_runtimes();

        // Load model router (intelligent model selection based on task)
        let router = match ohagent_core::model_router::ModelRouter::load() {
            Ok(r) => {
                info!(models = r.list_models().len(), "Model router loaded");
                let prefs_path =
                    std::path::PathBuf::from(shellexpand::tilde("~/.ohagent/model_prefs.toml").to_string());
                let r = r.with_prefs_path(prefs_path);
                let disabled_path =
                    std::path::PathBuf::from(shellexpand::tilde("~/.ohagent/disabled_models.json").to_string());
                let r = r.with_disabled_path(disabled_path);
                Some(Arc::new(std::sync::Mutex::new(r)))
            }
            Err(e) => {
                tracing::warn!(error = %e, "Model router unavailable — using default provider");
                None
            }
        };

        // Initialize Vault client for secret resolution
        let vault = VaultClient::from_env();
        if vault.available() {
            info!("Vault client initialized");
        } else {
            info!("Vault not configured — falling back to env vars and keys.toml");
        }
        let vault = Arc::new(vault);

        // Load keys from keys.toml for fallback
        let keys_path = shellexpand::tilde("~/.ohagent/keys.toml").to_string();
        let keys_config: std::collections::HashMap<String, String> =
            match std::fs::read_to_string(&keys_path) {
                Ok(content) => {
                    #[derive(serde::Deserialize, Default)]
                    struct KeysToml {
                        #[serde(default)]
                        keys: std::collections::HashMap<String, String>,
                    }
                    toml::from_str::<KeysToml>(&content)
                        .map(|k| k.keys)
                        .unwrap_or_default()
                }
                Err(_) => std::collections::HashMap::new(),
            };

        // Resolve provider API keys via Vault → env → keys.toml
        let rt = tokio::runtime::Handle::current();
        let (deepseek_key, anthropic_key, openai_key, siliconflow_key, scaleway_key, groq_key) = rt.block_on(async {
            let dk = resolve_secret(
                &vault,
                "providers/deepseek/api-key",
                "DEEPSEEK_API_KEY",
                &keys_config,
            ).await;
            let ak = resolve_secret(
                &vault,
                "providers/anthropic/api-key",
                "ANTHROPIC_API_KEY",
                &keys_config,
            ).await;
            let ok = resolve_secret(
                &vault,
                "providers/openai/api-key",
                "OPENAI_API_KEY",
                &keys_config,
            ).await;
            let sfk = resolve_secret(
                &vault,
                "providers/siliconflow/api-key",
                "SF_API_KEY",
                &keys_config,
            ).await;
            let swk = resolve_secret(
                &vault,
                "providers/scaleway/secret-key",
                "SCW_SECRET_KEY",
                &keys_config,
            ).await;
            let gk = resolve_secret(
                &vault,
                "providers/groq/api-key",
                "GROQ_API_KEY",
                &keys_config,
            ).await;
            (dk, ak, ok, sfk, swk, gk)
        });

        // Set resolved keys into env for jcode provider resolution
        if let Some(ref key) = deepseek_key {
            std::env::set_var("DEEPSEEK_API_KEY", key);
        }
        if let Some(ref key) = anthropic_key {
            std::env::set_var("ANTHROPIC_API_KEY", key);
        }
        if let Some(ref key) = openai_key {
            std::env::set_var("OPENAI_API_KEY", key);
        }
        if let Some(ref key) = siliconflow_key {
            std::env::set_var("SF_API_KEY", key);
        }
        if let Some(ref key) = scaleway_key {
            std::env::set_var("SCW_SECRET_KEY", key);
        }
        if let Some(ref key) = groq_key {
            std::env::set_var("GROQ_API_KEY", key);
        }

        // Build default provider (fallback if router unavailable)
        let provider: Arc<dyn Provider> = {
            let multi = jcode_base::provider::MultiProvider::default();

            // Configure from environment / Vault
            if let Ok(_api_key) = std::env::var("DEEPSEEK_API_KEY") {
                multi
                    .set_model("deepseek:deepseek-v4-flash")
                    .map_err(|e| anyhow::anyhow!("Failed to set DeepSeek model: {e}"))?;
                info!(
                    provider = %multi.display_name(),
                    "Default provider: DeepSeek"
                );
            } else if let Ok(_api_key) = std::env::var("ANTHROPIC_API_KEY") {
                multi
                    .set_model("claude:claude-sonnet-4-6")
                    .map_err(|e| anyhow::anyhow!("Failed to set Claude model: {e}"))?;
                info!(
                    provider = %multi.display_name(),
                    "Default provider: Claude"
                );
            } else {
                info!("No provider API key found. Using default provider (may need /login).");
            }

            Arc::new(multi)
        };

        // Initialize message log (prompt/response logging)
        let message_log = match ohagent_core::message_log::MessageLog::open(
            &shellexpand::tilde("~/.ohagent/message_log.db").to_string(),
        ) {
            Ok(log) => {
                info!("Message log initialized");
                // Apply migrations to message log DB
                if let Err(e) = log.with_conn(|conn| {
                    migrations::run(conn).map_err(|e| anyhow::anyhow!("{e}"))
                }) {
                    tracing::warn!(error = %e, "Message log migrations failed");
                }
                Some(Arc::new(log))
            }
            Err(e) => {
                tracing::warn!(error = %e, "Message log not available");
                None
            }
        };

        // Wrap provider with LoggingProvider for prompt/response capture
        let provider: Arc<dyn jcode_provider_core::Provider> = if let Some(ref log) = message_log {
            info!("Wrapping provider with LoggingProvider");
            Arc::new(ohagent_core::logging_provider::LoggingProvider::new(
                provider,
                Arc::clone(log),
                "default".into(),
            ))
        } else {
            provider
        };

        let mut bridge = JcodeBridge::new(provider);
        if let Some(ref r) = router {
            bridge = bridge.with_router(Arc::clone(r));
        }

        // Register built-in tools: bash, write, edit, read, ls
        let mut tool_registry = ohagent_core::tools::ToolRegistry::new();
        ohagent_core::builtin_tools::register_builtin_tools(
            &mut tool_registry,
            &shellexpand::tilde("~/.ohagent/workspace").to_string(),
        );
        let tool_registry = Arc::new(tool_registry);
        bridge = bridge.with_tools((*tool_registry).clone());
        info!(tools = tool_registry.list().len(), "Built-in tools registered");

        // Initialize push notification service
        let push = match std::env::var("TELEGRAM_BOT_TOKEN") {
            Ok(token) => {
                info!("Push notification service initialized (Telegram)");
                Some(Arc::new(ohagent_core::push::PushService::new(token)))
            }
            Err(_) => {
                tracing::debug!("TELEGRAM_BOT_TOKEN not set — push notifications disabled");
                None
            }
        };

        // Initialize MCP server pool (shared across all sessions).
        // Servers are defined in ~/.jcode/mcp.json (auto-imported from
        // Claude Code / Codex CLI on first run).
        let mcp_pool = {
            let pool = Arc::new(SharedMcpPool::from_default_config());
            let (connected, failures) = rt.block_on(pool.connect_all());
            if !failures.is_empty() {
                for (name, err) in &failures {
                    tracing::warn!(
                        server = %name,
                        error = %err,
                        "MCP server connection failed"
                    );
                }
            }
            if connected > 0 {
                info!(
                    connected = connected,
                    failed = failures.len(),
                    "MCP pool initialized"
                );
                Some(pool)
            } else if failures.is_empty() {
                // No servers configured — not an error
                tracing::debug!("MCP pool: no servers configured in ~/.jcode/mcp.json");
                None
            } else {
                tracing::warn!(
                    failed = failures.len(),
                    "MCP pool: all server connections failed"
                );
                None
            }
        };

        if let Some(ref pool) = mcp_pool {
            bridge = bridge.with_mcp_pool(Arc::clone(pool));
        }

        let bridge = Arc::new(bridge);

        // Initialize memory engine
        let memory = match MemoryEngine::open(MemoryConfig::default()) {
            Ok(engine) => {
                info!("Memory engine initialized");
                Some(Arc::new(engine))
            }
            Err(e) => {
                tracing::warn!(error = %e, "Memory engine not available — running without memory");
                None
            }
        };

        // Initialize skill registry
        let skills = match SkillRegistry::open(SkillConfig::default()) {
            Ok(reg) => {
                info!("Skill registry initialized");
                Some(Arc::new(reg))
            }
            Err(e) => {
                tracing::warn!(error = %e, "Skill registry not available");
                None
            }
        };

        // Initialize usage tracker
        let usage = match ohagent_core::usage_tracker::UsageTracker::open(
            "~/.ohagent/usage.db", None,
        ) {
            Ok(t) => {
                info!("Usage tracker initialized");
                Some(Arc::new(t))
            }
            Err(e) => {
                tracing::warn!(error = %e, "Usage tracker not available");
                None
            }
        };

        // Initialize WhatsApp adapter (if configured)
        let whatsapp = match WhatsAppAdapter::from_env() {
            Ok(wa) => {
                info!("WhatsApp adapter initialized");
                Some(Arc::new(wa))
            }
            Err(e) => {
                tracing::debug!(error = %e, "WhatsApp not configured, skipping");
                None
            }
        };

        // Initialize Slack adapter (if configured)
        let slack = match SlackAdapter::from_env() {
            Ok(sl) => {
                info!("Slack adapter initialized");
                Some(Arc::new(sl))
            }
            Err(e) => {
                tracing::debug!(error = %e, "Slack not configured, skipping");
                None
            }
        };

        // Initialize API auth (key from env or generated)
        let auth_config = auth::AuthConfig::from_env();

        // Initialize rate limiter
        let rate_limiter = Arc::new(rate_limiter::RateLimiter::new(
            rate_limiter::RateLimitConfig::from_env(),
        ));

        // Initialize Prometheus metrics
        let prom_metrics = Arc::new(
            metrics::Metrics::new()
                .expect("Failed to register Prometheus metrics"),
        );

        // Build system prompt with skills loaded once at startup.
        // AGENTS.md rules are re-read per-request in assemble() for project switching.
        let system_prompt_builder = {
            let skills_list: Vec<SkillPrompt> = skills
                .as_ref()
                .map(|reg| {
                    reg.list("default", None, 100)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|s| SkillPrompt {
                            id: s.id,
                            name: s.name.clone(),
                            trigger: s.triggers.join(", "),
                            instructions: s.instructions,
                        })
                        .collect()
                })
                .unwrap_or_default();

            let persistent = PersistentInstructions {
                skills: skills_list,
                tenant_overrides: None,
            };

            if persistent.skills.is_empty() {
                info!("SystemPromptBuilder: no skills loaded — rules-only mode");
                Some(SystemPromptBuilder::new(persistent.skills, persistent.tenant_overrides))
            } else {
                info!(
                    skills = persistent.skills.len(),
                    "SystemPromptBuilder initialized"
                );
                Some(SystemPromptBuilder::new(persistent.skills, persistent.tenant_overrides))
            }
        };

        // Initialize session store for daemon restart persistence
        let session_store = match ohagent_core::session_store::SessionStore::open(
            &shellexpand::tilde("~/.ohagent/sessions.db").to_string(),
        ) {
            Ok(ss) => {
                let cleaned = ss.cleanup_stale(30).unwrap_or(0);
                let active = ss.list_active().unwrap_or_default();
                info!(
                    active_sessions = active.len(),
                    stale_cleaned = cleaned,
                    "Session store initialized"
                );
                if !active.is_empty() {
                    for s in &active {
                        tracing::debug!(
                            tenant = %s.tenant_id,
                            session = %s.session_hash,
                            messages = s.message_count,
                            "Recovered active session"
                        );
                    }
                }
                Some(Arc::new(ss))
            }
            Err(e) => {
                tracing::warn!(error = %e, "Session store not available");
                None
            }
        };

        // Initialize plugin pipeline
        let plugin_config_path = shellexpand::tilde("~/.ohagent/plugins.toml").to_string();
        let plugin_config: ohagent_plugins::PluginConfig =
            match std::fs::read_to_string(&plugin_config_path) {
                Ok(content) => toml::from_str(&content).unwrap_or_default(),
                Err(_) => {
                    tracing::debug!("No plugins.toml found — plugin pipeline disabled");
                    ohagent_plugins::PluginConfig::default()
                }
            };
        let plugin_dir = shellexpand::tilde("~/.ohagent/plugins").to_string();
        let mut plugin_manager = PluginManager::new(
            std::path::PathBuf::from(&plugin_dir),
            plugin_config,
        );
        let loaded = plugin_manager.load_all();
        if loaded > 0 {
            info!(loaded, "Plugin pipeline initialized");
        }

        Ok(Self {
            health_port,
            enable_telegram,
            shutdown: Arc::new(tokio::sync::Notify::new()),
            bridge,
            memory,
            skills,
            usage,
            message_log,
            router,
            start_time: chrono::Utc::now(),
            keys_path,
            vault,
            auth_config,
            rate_limiter,
            metrics: prom_metrics,
            system_prompt_builder,
            session_store,
            tool_registry: Some(tool_registry),
            push: push.clone(),
            scheduler: Some(Arc::new(ohagent_core::scheduler::Scheduler::new(push))),
            whatsapp,
            slack,
            mcp_pool,
            plugin_manager: Arc::new(StdMutex::new(plugin_manager)),
        })
    }

    /// Start the API + health check HTTP server.
    async fn start_api_server(&self) -> Result<()> {
        let port = self.health_port;
        let shutdown = Arc::clone(&self.shutdown);

        let api_state = api::ApiState {
            bridge: Arc::clone(&self.bridge),
            memory: self.memory.clone(),
            skills: self.skills.clone(),
            usage: self.usage.clone(),
            message_log: self.message_log.clone(),
            model_router: self.router.clone(),
            system_prompt_builder: self.system_prompt_builder.clone(),
            session_store: self.session_store.clone(),
            tool_registry: self.tool_registry.clone(),
            push: self.push.clone(),
            scheduler: self.scheduler.clone(),
            plugin_manager: Some(Arc::clone(&self.plugin_manager)),
            start_time: self.start_time,
            keys_path: self.keys_path.clone(),
            vault: Arc::clone(&self.vault),
            auth_state: auth::AuthState {
                config: self.auth_config.clone(),
            },
            metrics_state: metrics::MetricsState {
                metrics: Arc::clone(&self.metrics),
            },
            webhook_state: webhooks::WebhookState {
                whatsapp: self.whatsapp.clone(),
                slack: self.slack.clone(),
            },
        };

        let app = api::router(api_state);

        tokio::spawn(async move {
            use std::net::SocketAddr;
            let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
            info!("API server listening on http://{addr}");

            let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    shutdown.notified().await;
                    info!("API server shutting down");
                })
                .await
                .unwrap();
        });

        Ok(())
    }

    /// Start the Telegram gateway.
    async fn start_telegram(&self) -> Result<()> {
        let mut adapter = TelegramAdapter::from_env().map_err(|e| {
            anyhow::anyhow!(
                "Failed to initialize Telegram adapter: {e}. \
                 Set TELEGRAM_BOT_TOKEN or disable with --telegram=false."
            )
        })?;

        if let Some(ref skills) = self.skills {
            adapter = adapter.with_skills(Arc::clone(skills));
        }
        if let Some(ref r) = self.router {
            adapter = adapter.with_router(Arc::clone(r));
        }
        if let Some(ref u) = self.usage {
            adapter = adapter.with_usage(Arc::clone(u));
        }
        if let Some(ref log) = self.message_log {
            adapter = adapter.with_message_log(Arc::clone(log));
        }
        if let Some(ref ss) = self.session_store {
            adapter = adapter.with_session_store(Arc::clone(ss));
        }
        if let Some(ref push) = self.push {
            adapter = adapter.with_push(Arc::clone(push));
        }
        if let Some(ref mem) = self.memory {
            adapter = adapter.with_memory(Arc::clone(mem));
        }
        let pm = Arc::clone(&self.plugin_manager);
        adapter = adapter.with_plugin_manager(pm);

        // Also attach model router reference for the /model command
        // (the router is stored inside the bridge, we also pass it directly)
        // The router and usage tracker are stored in Daemon fields added for this purpose.
        // They are passed to the gateway's Dispatcher via the adapter's `start` method.

        info!("Telegram adapter configured, starting bot...");

        let bridge = Arc::clone(&self.bridge);
        tokio::spawn(async move {
            if let Err(e) = adapter.start(bridge).await {
                tracing::error!(error = %e, "Telegram gateway crashed");
            }
        });

        Ok(())
    }

    /// Run the daemon main loop.
    async fn run(self) -> Result<()> {
        info!(
            "ohAgent daemon v{} starting...",
            option_env!("CARGO_PKG_VERSION").unwrap_or("0.1.0")
        );

        // Start API server (health + REST)
        self.start_api_server().await?;

        // Start Telegram gateway (if enabled)
        if self.enable_telegram {
            match self.start_telegram().await {
                Ok(()) => info!("Telegram gateway started"),
                Err(e) => tracing::warn!(error = %e, "Telegram gateway not started"),
            }
        }

        // Start skills cron (creation, evaluation, curation)
        self.start_skills_cron().await;

        info!("ohAgent daemon ready");

        // Wait for shutdown signal
        self.wait_for_shutdown().await;

        info!("ohAgent daemon stopped");
        Ok(())
    }

    /// Spawn a periodic background task for skill lifecycle management.
    ///
    /// - Every 10 minutes: scan conversations, propose new skills
    /// - Every 5 minutes: evaluate existing skills, promote/demote
    /// - Every 30 minutes: curate (merge, prune, enforce limits)
    async fn start_skills_cron(&self) {
        let memory = self.memory.clone();
        let skills = self.skills.clone();
        let shutdown = Arc::clone(&self.shutdown);

        if memory.is_none() || skills.is_none() {
            tracing::info!("Skills cron disabled — memory or skills not available");
            return;
        }

        tokio::spawn(async move {
            let memory = memory.unwrap();
            let skills = skills.unwrap();
            let config = SkillConfig::default();

            // Use two intervals: short for eval, longer for creation & curation
            let mut eval_tick = tokio::time::interval(std::time::Duration::from_secs(300)); // 5 min
            let mut create_tick = tokio::time::interval(std::time::Duration::from_secs(600)); // 10 min
            // Curate every other eval cycle (≈10 min) for simplicity
            let mut curate_tick = tokio::time::interval(std::time::Duration::from_secs(600));

            loop {
                tokio::select! {
                    _ = shutdown.notified() => {
                        info!("Skills cron shutting down");
                        break;
                    }
                    _ = eval_tick.tick() => {
                        // Evaluate all tenants with skills
                        if let Ok(tenants) = skills.all_tenants() {
                            for tid in &tenants {
                                if let Err(e) = ohagent_skills::evaluator::periodic_evaluation(&skills, tid, &config) {
                                    tracing::warn!(tenant_id=%tid, error=%e, "Skill evaluation failed");
                                }
                            }
                        }
                    }
                    _ = create_tick.tick() => {
                        // Propose new skills for tenants with recent conversations
                        let sample_tenants = ["default"];
                        for tid in &sample_tenants {
                            if let Err(e) = ohagent_skills::creator::propose_skills(
                                &skills, &memory, tid, &config,
                            ) {
                                tracing::debug!(tenant_id=%tid, error=%e, "Skill creation skipped");
                            }
                        }
                    }
                    _ = curate_tick.tick() => {
                        if let Ok(tenants) = skills.all_tenants() {
                            for tid in &tenants {
                                match ohagent_skills::curator::curate(&skills, tid, &config) {
                                    Ok(report) => {
                                        tracing::info!(
                                            tenant_id=%tid,
                                            merged=report.merged,
                                            pruned=report.pruned,
                                            "Curator pass complete"
                                        );
                                    }
                                    Err(e) => {
                                        tracing::warn!(tenant_id=%tid, error=%e, "Curation failed");
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        info!("Skills cron started (eval: 5min, create: 10min, curate: 10min)");
    }

    /// Wait for SIGTERM or SIGINT, then trigger graceful shutdown.
    async fn wait_for_shutdown(&self) {
        let shutdown = Arc::clone(&self.shutdown);

        tokio::select! {
            _ = signal::ctrl_c() => {
                info!("Received SIGINT (Ctrl+C), initiating shutdown");
            }
            result = async {
                #[cfg(unix)]
                {
                    use tokio::signal::unix::{signal, SignalKind};
                    let mut sigterm = signal(SignalKind::terminate()).unwrap();
                    sigterm.recv().await;
                    info!("Received SIGTERM, initiating shutdown");
                }
            } => {
                let _ = result;
            }
        }

        shutdown.notify_waiters();
    }
}

/// Main entry point for ohAgent daemon.
pub async fn run() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(&cli.log_level)),
        )
        .init();

    let daemon = Daemon::new(cli.health_port, cli.telegram)?;
    daemon.run().await
}

#[tokio::main]
async fn main() -> Result<()> {
    run().await
}
