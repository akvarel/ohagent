//! ohagent-gateway: Messaging gateway for ohAgent.
//!
//! Provides platform adapters for messaging platforms.
//! Telegram first, then Discord, Slack, etc.

pub mod adapter;
pub mod dispatch;
pub mod pairing;
pub mod session;

pub mod platforms {
    //! Platform-specific adapters.
    pub mod telegram;
    // TODO: discord, slack, whatsapp
}

/// Gateway result type.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
