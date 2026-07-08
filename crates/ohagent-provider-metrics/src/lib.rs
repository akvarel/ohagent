//! ohagent-provider-metrics — price tracking, speed benchmarks, dynamic routing.
//!
//! Three subsystems:
//! 1. **Price Scraper** — fetches current pricing from provider APIs daily
//! 2. **Speed Benchmark** — measures real latency + throughput per model
//! 3. **Dynamic Router** — selects optimal provider based on price/speed/quality

pub mod models;
pub mod store;
pub mod scraper;
pub mod discovery;
pub mod benchmark;
pub mod router;
pub mod preclassifier;
pub mod receipt_validator;
pub mod gemini_ocr;

pub use models::*;
pub use store::MetricsStore;
pub use scraper::PriceScraper;
pub use discovery::known_prices;
pub use benchmark::{SpeedBenchmark, BenchmarkConfig};
pub use router::DynamicRouter;
pub use preclassifier::{PreClassifier, PreClassifierConfig};
pub use receipt_validator::{validate_receipt, ReceiptData, ReceiptItem, ReceiptVerdict};
pub use gemini_ocr::{GeminiOcrClient, GeminiOcrConfig};
