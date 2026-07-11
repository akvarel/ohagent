//! ohagent-core: Core agent loop, config, and provider bridge.
//!
//! This crate provides the central agent orchestration — session management,
//! Jcode integration bridge, tool dispatch, and provider resolution.

pub mod agent;
pub mod agent_runner;
pub mod builtin_tools;
pub mod config;
pub mod context_estimator;
pub mod copilot_acp;
pub mod jcode_bridge;
pub mod llm_classifier;
pub mod logging_provider;
pub mod message_log;
pub mod model_router;
pub mod pricing;
pub mod push;
pub mod s3_archive;
pub mod scheduler;
pub mod session;
pub mod session_store;
pub mod tools;
pub mod usage_tracker;
pub mod vault;
pub mod version_check;

/// Core error type for ohAgent.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
