//! ohagent-aggregator-core — open-source API key management and billing.
//!
//! # License
//!
//! Apache 2.0. This is the FREE open-source core.
//! The proprietary plugin (`ohagent-aggregator.so`) wraps this with:
//! - License validation (HMAC-SHA256, similar to PII plugin)
//! - Dynamic routing integration (DynamicRouter from provider-metrics)
//! - Markup tier pricing (Starter/Pro/Enterprise)
//! - Monthly subscription enforcement

pub mod apikeys;
pub mod billing;
pub mod store;

pub use apikeys::{ApiKey, ApiKeyManager, MarkupTier};
pub use billing::BillingTracker;
pub use store::AggregatorStore;
