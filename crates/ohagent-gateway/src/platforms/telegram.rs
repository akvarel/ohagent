//! Telegram platform adapter.
//!
//! Uses teloxide for the Telegram Bot API.
//! Requires TELEGRAM_BOT_TOKEN environment variable (injected by Vault agent).

use std::sync::{Arc, Mutex};
use teloxide::{
    dispatching::UpdateFilterExt,
    prelude::*,
    types::{ChatId, Message, ParseMode, Update},
    utils::command::BotCommands,
};
use tracing::{info, warn};

use crate::adapter::{FileAttachment, IncomingMessage, OutgoingMessage, PlatformAdapter};
use crate::dispatch::Dispatcher;
use crate::i18n::Lang;
use crate::pairing::PairingManager;
use crate::session::SessionManager;
use ohagent_core::jcode_bridge::JcodeBridge;
use ohagent_core::message_log::MessageLog;
use ohagent_core::model_router::ModelRouter;
use ohagent_core::push::PushService;
use ohagent_core::session_store::SessionStore;
use ohagent_core::usage_tracker::UsageTracker;
use ohagent_memory::engine::MemoryEngine;
use ohagent_plugins::PluginManager;
use std::sync::Mutex as StdMutex;
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
    #[command(description = "Remember something")]
    Remember(String),
    #[command(description = "Search memories")]
    Recall(String),
    #[command(description = "Forget a memory by ID")]
    Forget(String),
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
    router: Option<Arc<Mutex<ModelRouter>>>,
    usage: Option<Arc<UsageTracker>>,
    message_log: Option<Arc<MessageLog>>,
    session_store: Option<Arc<SessionStore>>,
    push: Option<Arc<PushService>>,
    memory: Option<Arc<MemoryEngine>>,
    plugin_manager: Option<Arc<StdMutex<PluginManager>>>,
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
            usage: None,
            message_log: None,
            session_store: None,
            push: None,
            memory: None,
            plugin_manager: None,
        })
    }

    /// Create with an explicit token.
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            bot_token: token.into(),
            skills: None,
            router: None,
            usage: None,
            message_log: None,
            session_store: None,
            push: None,
            memory: None,
            plugin_manager: None,
        }
    }

    /// Attach a skill registry for skill commands.
    pub fn with_skills(mut self, skills: Arc<SkillRegistry>) -> Self {
        self.skills = Some(skills);
        self
    }

    /// Attach a model router for model commands.
    pub fn with_router(mut self, router: Arc<Mutex<ModelRouter>>) -> Self {
        self.router = Some(router);
        self
    }

    /// Attach a usage tracker for recording API calls.
    pub fn with_usage(mut self, usage: Arc<UsageTracker>) -> Self {
        self.usage = Some(usage);
        self
    }

    /// Attach a message log for the /logging command.
    pub fn with_message_log(mut self, log: Arc<MessageLog>) -> Self {
        self.message_log = Some(log);
        self
    }

    /// Attach a session store for /new persistence.
    pub fn with_session_store(mut self, store: Arc<SessionStore>) -> Self {
        self.session_store = Some(store);
        self
    }

    /// Attach a push service for pairing registration.
    pub fn with_push(mut self, push: Arc<PushService>) -> Self {
        self.push = Some(push);
        self
    }

    /// Attach the memory engine for /remember, /recall, /forget.
    pub fn with_memory(mut self, memory: Arc<MemoryEngine>) -> Self {
        self.memory = Some(memory);
        self
    }

    /// Attach the plugin manager for message filtering.
    pub fn with_plugin_manager(mut self, pm: Arc<StdMutex<PluginManager>>) -> Self {
        self.plugin_manager = Some(pm);
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
        if let Some(ref usage) = self.usage {
            dispatcher_builder = dispatcher_builder.with_usage(Arc::clone(usage));
        }
        if let Some(ref log) = self.message_log {
            dispatcher_builder = dispatcher_builder.with_message_log(Arc::clone(log));
        }
        if let Some(ref ss) = self.session_store {
            dispatcher_builder = dispatcher_builder.with_session_store(Arc::clone(ss));
        }
        if let Some(ref push) = self.push {
            dispatcher_builder = dispatcher_builder.with_push(Arc::clone(push));
        }
        if let Some(ref mem) = self.memory {
            dispatcher_builder = dispatcher_builder.with_memory(Arc::clone(mem));
        }
        if let Some(ref pm) = self.plugin_manager {
            dispatcher_builder = dispatcher_builder.with_plugin_manager(Arc::clone(pm));
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
    let user = msg.from.as_ref();
    let user_id = user
        .map(|u| u.id.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let chat_id = msg.chat.id.to_string();

    let lang = Lang::from_code(
        user.and_then(|u| u.language_code.as_deref()),
    );

    // ── Handle photo messages ──
    let attachment = if let Some(photos) = msg.photo() {
        // Take the largest photo (last in array)
        let largest = photos.last();
        match largest {
            Some(photo) => {
                match download_telegram_file(&bot, &photo.file.id, "photo.jpg").await {
                    Ok(local_path) => {
                        info!(chat_id = %chat_id, path = %local_path, "Photo received and saved");
                        Some(FileAttachment {
                            local_path,
                            file_name: Some("photo.jpg".into()),
                            mime_type: Some("image/jpeg".into()),
                            size_bytes: photo.file.size as u64,
                        })
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to download photo");
                        None
                    }
                }
            }
            None => None,
        }
    // ── Handle document messages ──
    } else if let Some(doc) = msg.document() {
        let file_name = doc.file_name.clone().unwrap_or_else(|| "document.bin".into());
        match download_telegram_file(&bot, &doc.file.id, &file_name).await {
            Ok(local_path) => {
                info!(chat_id = %chat_id, path = %local_path, name = %file_name, "Document received and saved");
                Some(FileAttachment {
                    local_path,
                    file_name: Some(file_name),
                    mime_type: doc.mime_type.as_ref().map(|m| m.essence_str().to_string()),
                    size_bytes: doc.file.size as u64,
                })
            }
            Err(e) => {
                warn!(error = %e, "Failed to download document");
                None
            }
        }
    } else {
        None
    };

    // ── Handle text (may exist alongside photo/document caption) ──
    let caption = msg.caption().unwrap_or("").to_string();
    let text_body = msg.text().unwrap_or("").to_string();
    let text = if !text_body.is_empty() {
        text_body
    } else if !caption.is_empty() {
        caption
    } else if attachment.is_some() {
        "[photo/file]".to_string()
    } else {
        return Ok(());
    };

    info!(
        user_id = %user_id,
        chat_id = %chat_id,
        text_len = text.len(),
        has_attachment = attachment.is_some(),
        "Telegram message received"
    );

    let incoming = IncomingMessage {
        chat_id: chat_id.clone(),
        user_id: user_id.clone(),
        tenant_id: format!("telegram_{user_id}"),
        text,
        lang,
        platform: "telegram".into(),
        attachment,
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

/// Download a file from Telegram's servers and save locally.
async fn download_telegram_file(
    bot: &Bot,
    file_id: &str,
    file_name: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    use teloxide::net::Download;
    use std::io::Write as _;

    let file = bot.get_file(file_id).await?;
    let upload_dir = shellexpand::tilde("~/.ohagent/uploads").to_string();
    std::fs::create_dir_all(&upload_dir)?;

    let safe_name = file_name.replace(['/', '\\', ' '], "_");
    let path = format!("{}/{}", upload_dir, safe_name);

    let mut dest = std::fs::File::create(&path)?;
    let mut stream = bot.download_file_stream(&file.path);
    // Collect all bytes via .bytes_stream() or iterate
    // teloxide's download_file_stream returns Stream<Item = Result<Bytes, ...>>
    use futures::StreamExt;
    while let Some(chunk) = stream.next().await {
        let data = chunk?;
        std::io::Write::write_all(&mut dest, &data)?;
    }
    dest.flush()?;

    Ok(path)
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
        Command::Remember(content) => ("remember", content.as_str()),
        Command::Recall(query) => ("recall", query.as_str()),
        Command::Forget(id) => ("forget", id.as_str()),
    };

    let incoming = IncomingMessage {
        chat_id: chat_id.clone(),
        user_id,
        tenant_id: format!("telegram_{}", msg.chat.id),
        text: format!("/{command} {args}"),
        lang,
        platform: "telegram".into(),
        attachment: None,
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
