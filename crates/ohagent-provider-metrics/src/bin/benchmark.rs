//! ohagent-provider-metrics — CLI benchmark tool.
//!
//! Usage:
//!   cargo run -p ohagent-provider-metrics -- benchmark --provider deepseek --model deepseek-v4-flash --api-key $KEY
//!   cargo run -p ohagent-provider-metrics -- scrape
//!   cargo run -p ohagent-provider-metrics -- route --capabilities chat --prompt-tokens 1000 --output-tokens 2000 --tier balanced

use ohagent_provider_metrics::{
    MetricsStore, PriceScraper, SpeedBenchmark, BenchmarkConfig, DynamicRouter,
    RouterConfig, QualityTier,
};

#[derive(Debug, clap::Parser)]
#[command(name = "ohagent-metrics")]
struct Cli {
    #[arg(long, default_value = "~/.ohagent/metrics.db")]
    db_path: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Scrape provider prices and store in DB
    Scrape,
    /// Run speed benchmarks
    Benchmark {
        #[arg(long)]
        provider: String,
        #[arg(long)]
        model: String,
        #[arg(long)]
        api_key: String,
        #[arg(long)]
        api_base: String,
        #[arg(long, default_value = "3")]
        samples: u32,
    },
    /// Route a task
    Route {
        #[arg(long, default_value = "chat")]
        capabilities: String,
        #[arg(long, default_value = "1000")]
        prompt_tokens: u64,
        #[arg(long, default_value = "2000")]
        output_tokens: u64,
        #[arg(long, default_value = "balanced")]
        tier: String,
    },
    /// Show estimated speed comparison
    SpeedCompare,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = <Cli as clap::Parser>::parse();
    let store = MetricsStore::open(shellexpand::tilde(&cli.db_path).to_string())?;

    match cli.command {
        Command::Scrape => {
            let scraper = PriceScraper::new();
            let count = scraper.scrape_all(&store).await?;
            println!("Scraped {count} prices");
        }
        Command::Benchmark { provider, model, api_key, api_base, samples } => {
            let bench = SpeedBenchmark::new();
            let config = BenchmarkConfig { provider: provider.clone(), model_id: model.clone(), api_key, api_base, samples };
            println!("Running benchmark: {} / {} ({} samples)...", config.provider, config.model_id, config.samples);
            let result = bench.run(&config).await;
            store.upsert_speed(&result)?;
            println!("TTF: {}ms, Total: {}ms, TPS: {:.1}, P95: {}ms, Error: {:?}",
                result.ttf_ms, result.total_latency_ms, result.tokens_per_second,
                result.p95_latency_ms, result.error);
        }
        Command::Route { capabilities, prompt_tokens, output_tokens, tier } => {
            let router = DynamicRouter::new(store);
            let caps: Vec<&str> = capabilities.split(',').map(|s| s.trim()).collect();
            let config = RouterConfig {
                quality_tier: match tier.as_str() {
                    "budget" => QualityTier::Budget,
                    "balanced" => QualityTier::Balanced,
                    "performance" => QualityTier::Performance,
                    "quality" => QualityTier::Quality,
                    _ => QualityTier::Balanced,
                },
                ..Default::default()
            };
            let decision = router.route(&caps, prompt_tokens, output_tokens, &config)?;
            println!("{}", serde_json::to_string_pretty(&decision)?);
        }
        Command::SpeedCompare => {
            println!("Estimated provider speed comparison (tokens/sec, lower latency = better):");
            println!("⚠️ = deprecated model, migrate to replacement");
            println!("{:<20} {:<30} {:>10} {:>10} {:>10} {:>10}", "Provider", "Model", "TTF ms", "Total ms", "tok/s", "Status");
            println!("{}", "-".repeat(95));
            let entries: Vec<(&str, &str, u64, u64, f64, &str)> = vec![
                ("Groq",         "Llama-3.3-70B",      100,  500,   250.0, "≈ LPU"),
                ("SiliconFlow",  "Qwen3-8B",            150,  800,   120.0, "≈ small"),
                ("OpenAI",       "GPT-4o-mini",         200,  900,   100.0, "≈ docs"),
                ("SiliconFlow",  "Hy3-preview",         300, 1200,    90.0, "≈ MoE"),
                ("SiliconFlow",  "GLM-5.2",             500, 2000,    58.0, "≈ agentic"),
                ("DeepSeek",     "Chat-V3",            1889, 1889,    55.3, "⚠️ deprecated→V4-Flash"),
                ("DeepSeek",     "V4-Flash",           2221, 2221,    46.0, "✅ measured"),
                ("DeepSeek",     "Reasoner-R1",        3432, 3432,    31.2, "⚠️ deprecated→V4-Pro"),
                ("DeepSeek",     "V4-Pro",             6506, 6506,    18.3, "✅ measured"),
                ("Scaleway",     "Qwen3-coder-30B",      536,  536,   169.4, "✅ measured"),
                ("Scaleway",     "Mistral-small",         844,  844,   138.7, "✅ measured"),
                ("Scaleway",     "Llama-3.3-70B",         943,  943,    71.0, "✅ measured"),
                ("Anthropic",    "Claude-Haiku-3.5",    250, 1100,    70.0, "≈ docs"),
                ("OpenAI",       "GPT-4o",              800, 3000,    30.0, "≈ docs"),
                ("Anthropic",    "Claude-Sonnet-4",    1000, 4000,    25.0, "≈ docs"),
                ("Anthropic",    "Claude-Opus-4",      1500, 6000,    15.0, "≈ docs"),
            ];
            for (provider, model, ttf, total, tps, status) in entries {
                println!("{:<20} {:<30} {:>8}ms {:>8}ms {:>8.0} {:>15}", provider, model, ttf, total, tps, status);
            }
            println!("\nNote: Estimated values. Run 'ohagent-metrics benchmark' with real API keys for actual measurements.");
        }
    }

    Ok(())
}
