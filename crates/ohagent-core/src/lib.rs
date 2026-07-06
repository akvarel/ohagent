//! ohagent-core: Core agent loop, config, and provider bridge.
//!
//! This crate provides the central agent orchestration — session management,
//! Jcode integration bridge, tool dispatch, and provider resolution.

pub mod agent;
pub mod config;
pub mod jcode_bridge;
pub mod llm_classifier;
pub mod logging_provider;
pub mod message_log;
pub mod model_router;
pub mod s3_archive;
pub mod session;
pub mod tools;
pub mod usage_tracker;

/// Core error type for ohAgent.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
