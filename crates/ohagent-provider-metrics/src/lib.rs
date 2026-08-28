//! ohagent-provider-metrics — price tracking, speed benchmarks, dynamic routing.
//!
//! Three subsystems:
//! 1. **Price Scraper** — fetches current pricing from provider APIs daily
//! 2. **Speed Benchmark** — measures real latency + throughput per model
//! 3. **Dynamic Router** — selects optimal provider based on price/speed/quality

pub mod benchmark;
pub mod discovery;
pub mod gemini_ocr;
pub mod models;
pub mod preclassifier;
pub mod receipt_validator;
pub mod router;
pub mod scraper;
pub mod store;
pub mod vision_consensus;

pub use benchmark::{BenchmarkConfig, SpeedBenchmark};
pub use discovery::known_prices;
pub use gemini_ocr::{GeminiOcrClient, GeminiOcrConfig};
pub use models::*;
pub use preclassifier::{PreClassifier, PreClassifierConfig};
pub use receipt_validator::{validate_receipt, ReceiptData, ReceiptItem, ReceiptVerdict};
pub use router::DynamicRouter;
pub use scraper::PriceScraper;
pub use store::MetricsStore;
pub use vision_consensus::{run_consensus, ConsensusResult, DisputedField};
