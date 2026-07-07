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
            println!("{:<20} {:<30} {:>10} {:>10} {:>10}", "Provider", "Model", "TTF ms", "Total ms", "tok/s");
            println!("{}", "-".repeat(80));
            let entries: Vec<(&str, &str, u64, u64, f64)> = vec![
                ("SiliconFlow", "Qwen3-8B", 150, 800, 120.0),
                ("SiliconFlow", "Qwen3.5-9B", 180, 900, 100.0),
                ("SiliconFlow", "Hy3-preview", 300, 1200, 90.0),
                ("DeepSeek", "V4-Flash", 200, 1000, 80.0),
                ("Groq", "Llama-3.3-70B", 100, 500, 250.0),
                ("OpenAI", "GPT-4o-mini", 200, 900, 100.0),
                ("Scaleway", "Mistral-small", 400, 2000, 50.0),
                ("Scaleway", "Qwen3-coder-30B", 350, 1800, 55.0),
                ("Anthropic", "Claude-Haiku-3.5", 250, 1100, 70.0),
                ("OpenAI", "GPT-4o", 800, 3000, 30.0),
                ("DeepSeek", "Chat-V3", 600, 2500, 40.0),
                ("Anthropic", "Claude-Sonnet-4", 1000, 4000, 25.0),
                ("Anthropic", "Claude-Opus-4", 1500, 6000, 15.0),
            ];
            for (provider, model, ttf, total, tps) in entries {
                println!("{:<20} {:<30} {:>8}ms {:>8}ms {:>8.0}", provider, model, ttf, total, tps);
            }
            println!("\nNote: Estimated values. Run 'ohagent-metrics benchmark' with real API keys for actual measurements.");
        }
    }

    Ok(())
}
