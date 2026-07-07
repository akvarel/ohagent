//! ohagent-provider-metrics — price tracking, speed benchmarks, dynamic routing.
//!
//! Three subsystems:
//! 1. **Price Scraper** — fetches current pricing from provider APIs daily
//! 2. **Speed Benchmark** — measures real latency + throughput per model
//! 3. **Dynamic Router** — selects optimal provider based on price/speed/quality

pub mod models;
pub mod store;
pub mod scraper;
pub mod benchmark;
pub mod router;

pub use models::*;
pub use store::MetricsStore;
pub use scraper::PriceScraper;
pub use benchmark::{SpeedBenchmark, BenchmarkConfig};
pub use router::DynamicRouter;
