//! Telegram platform adapter.
//!
//! Uses teloxide for the Telegram Bot API.
//! Requires TELEGRAM_BOT_TOKEN environment variable (injected by Vault agent).

use std::sync::Arc;
use teloxide::{
    dispatching::UpdateFilterExt,
    prelude::*,
    types::{ChatId, Message, ParseMode, Update},
    utils::command::BotCommands,
};
use tracing::{info, warn};

use crate::adapter::{IncomingMessage, OutgoingMessage, PlatformAdapter};
use crate::dispatch::Dispatcher;
use crate::i18n::Lang;
use crate::pairing::PairingManager;
use crate::session::SessionManager;
use ohagent_core::jcode_bridge::JcodeBridge;
use ohagent_core::model_router::ModelRouter;
use ohagent_skills::registry::SkillRegistry;

/// Helper: parse a string chat_id to teloxide's ChatId.
fn to_chat_id(s: &str) -> Result<ChatId, Box<dyn std::error::Error + Send + Sync>> {
    let id: i64 = s.parse().map_err(|e| format!("Invalid chat ID '{s}': {e}"))?;
    Ok(ChatId(id))
}

/// Telegram bot commands.
#[derive(BotCommands, Debug, Clone)]
#[command(
    rename_rule = "lowercase",
    description = "Available commands:"
)]
enum Command {
    #[command(description = "Start the bot")]
    Start,
    #[command(description = "Pair your account")]
    Pair,
    #[command(description = "Confirm pairing code")]
    Confirm(String),
    #[command(description = "Show help")]
    Help,
    #[command(description = "Start a new conversation")]
    New,
    #[command(description = "Change language")]
    Lang,
    #[command(description = "Stop current task")]
    Stop,
    #[command(description = "Check agent status")]
    Status,
    #[command(description = "List learned skills")]
    Skills,
    #[command(description = "Show skill details")]
    Skill(String),
    #[command(description = "Record a skill as used")]
    Skilluse(String),
    #[command(description = "Show/set active model")]
    Model,
}

/// Shared state accessible from all Telegram handlers.
#[derive(Clone)]
struct TelegramState {
    dispatcher: Arc<Dispatcher>,
}

/// The Telegram platform adapter.
pub struct TelegramAdapter {
    bot_token: String,
    skills: Option<Arc<SkillRegistry>>,
    router: Option<Arc<ModelRouter>>,
}

impl TelegramAdapter {
    /// Create a new Telegram adapter.
    ///
    /// The bot token is read from TELEGRAM_BOT_TOKEN environment variable,
    /// which should be injected by the Vault agent.
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let token = std::env::var("TELEGRAM_BOT_TOKEN")
            .map_err(|_| "TELEGRAM_BOT_TOKEN not set. Ensure Vault agent is running.")?;
        Ok(Self {
            bot_token: token,
            skills: None,
            router: None,
        })
    }

    /// Create with an explicit token.
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            bot_token: token.into(),
            skills: None,
            router: None,
        }
    }

    /// Attach a skill registry for skill commands.
    pub fn with_skills(mut self, skills: Arc<SkillRegistry>) -> Self {
        self.skills = Some(skills);
        self
    }

    /// Attach a model router for model commands.
    pub fn with_router(mut self, router: Arc<ModelRouter>) -> Self {
        self.router = Some(router);
        self
    }
}

#[async_trait::async_trait]
impl PlatformAdapter for TelegramAdapter {
    fn name(&self) -> &str {
        "telegram"
    }

    async fn start(
        &self,
        bridge: Arc<JcodeBridge>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let bot = Bot::new(&self.bot_token);

        let pairing_manager = Arc::new(PairingManager::new());
        let session_manager = Arc::new(SessionManager::new(bridge));
        let mut dispatcher_builder = Dispatcher::new(session_manager, pairing_manager);
        if let Some(ref skills) = self.skills {
            dispatcher_builder = dispatcher_builder.with_skills(Arc::clone(skills));
        }
        if let Some(ref router) = self.router {
            dispatcher_builder = dispatcher_builder.with_router(Arc::clone(router));
        }
        let dispatcher = Arc::new(dispatcher_builder);

        let state = TelegramState {
            dispatcher: dispatcher.clone(),
        };
        let _ = &state; // state is used by dptree handler closures

        info!("Telegram bot starting (long-polling mode)...");

        // Build the handler chain
        let handler = Update::filter_message()
            .branch(
                dptree::entry()
                    .filter_command::<Command>()
                    .endpoint(handle_command),
            )
            .branch(dptree::endpoint(handle_message));

        teloxide::dispatching::Dispatcher::builder(bot, handler)
            .default_handler(|_| async move {
                warn!("Unhandled Telegram update");
            })
            .enable_ctrlc_handler()
            .build()
            .dispatch()
            .await;

        info!("Telegram bot stopped");
        Ok(())
    }

    async fn send_message(
        &self,
        msg: OutgoingMessage,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let bot = Bot::new(&self.bot_token);
        let chat_id = to_chat_id(&msg.chat_id)?;

        let mut req = bot.send_message(chat_id, msg.text);
        if msg.markdown {
            req = req.parse_mode(ParseMode::MarkdownV2);
        }

        req.await?;
        Ok(())
    }

    async fn set_typing(
        &self,
        chat_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let bot = Bot::new(&self.bot_token);
        let chat_id = to_chat_id(chat_id)?;
        bot.send_chat_action(chat_id, teloxide::types::ChatAction::Typing)
            .await?;
        Ok(())
    }
}

/// Handle regular text messages (non-commands).
async fn handle_message(
    bot: Bot,
    msg: Message,
    state: TelegramState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let text = match msg.text() {
        Some(text) => text.to_string(),
        None => return Ok(()),
    };

    let user = msg.from.as_ref();
    let user_id = user
        .map(|u| u.id.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let chat_id = msg.chat.id.to_string();

    let lang = Lang::from_code(
        user.and_then(|u| u.language_code.as_deref()),
    );

    info!(
        user_id = %user_id,
        chat_id = %chat_id,
        text_len = text.len(),
        "Telegram message received"
    );

    let incoming = IncomingMessage {
        chat_id: chat_id.clone(),
        user_id: user_id.clone(),
        tenant_id: format!("telegram_{user_id}"),
        text: text.clone(),
        lang,
        platform: "telegram".into(),
    };

    // Send typing indicator
    let _ = bot
        .send_chat_action(
            to_chat_id(&chat_id)?,
            teloxide::types::ChatAction::Typing,
        )
        .await;

    // Dispatch
    if let Some(response) = state.dispatcher.handle_message(incoming).await {
        let mut req = bot
            .send_message(to_chat_id(&chat_id)?, response.text);
        if response.markdown {
            req = req.parse_mode(ParseMode::MarkdownV2);
        }
        req.await?;
    }

    Ok(())
}

/// Handle slash commands.
async fn handle_command(
    bot: Bot,
    msg: Message,
    cmd: Command,
    state: TelegramState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let user = msg.from.as_ref();
    let user_id = user
        .map(|u| u.id.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let chat_id = msg.chat.id.to_string();
    let lang = Lang::from_code(
        user.and_then(|u| u.language_code.as_deref()),
    );

    info!(
        user_id = %user_id,
        chat_id = %chat_id,
        command = ?cmd,
        "Telegram command received"
    );

    let (command, args) = match &cmd {
        Command::Start => ("start", ""),
        Command::Pair => ("pair", ""),
        Command::Confirm(code) => ("confirm", code.as_str()),
        Command::Help => ("help", ""),
        Command::New => ("new", ""),
        Command::Lang => ("lang", ""),
        Command::Stop => ("stop", ""),
        Command::Status => ("status", ""),
        Command::Skills => ("skills", ""),
        Command::Skill(name) => ("skill", name.as_str()),
        Command::Skilluse(name) => ("skilluse", name.as_str()),
        Command::Model => ("model", ""),
    };

    let incoming = IncomingMessage {
        chat_id: chat_id.clone(),
        user_id,
        tenant_id: format!("telegram_{}", msg.chat.id),
        text: format!("/{command} {args}"),
        lang,
        platform: "telegram".into(),
    };

    if let Some(response) = state
        .dispatcher
        .handle_command(incoming, command, args)
        .await
    {
        let mut req = bot
            .send_message(to_chat_id(&chat_id)?, response.text);
        if response.markdown {
            req = req.parse_mode(ParseMode::MarkdownV2);
        }
        req.await?;
    }

    Ok(())
}
