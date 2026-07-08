//! Message dispatch — routes incoming messages to JcodeBridge sessions.
//!
//! Handles the full lifecycle: receive → check pairing → get/create session →
//! send "thinking" → process → send response.

use std::sync::{Arc, Mutex};
use std::fs;
use tracing::{error, info, warn};

use crate::adapter::{FileAttachment, IncomingMessage, OutgoingMessage};
use crate::i18n::I18n;
use crate::pairing::PairingManager;
use crate::session::SessionManager;
use ohagent_core::message_log::MessageLog;
use ohagent_core::model_router::ModelRouter;
use ohagent_core::push::PushService;
use ohagent_core::session_store::SessionStore;
use ohagent_core::usage_tracker::UsageTracker;
use ohagent_memory::engine::MemoryEngine;
use ohagent_memory::models::MemoryEntry;
use ohagent_skills::registry::SkillRegistry;
use ohagent_plugins::PluginManager;
use ohagent_provider_metrics::{GeminiOcrClient, GeminiOcrConfig};
use std::sync::Mutex as StdMutex;

/// Encode a file attachment to (media_type, base64_data) tuple suitable for Jcode.
fn encode_attachment(att: &FileAttachment) -> Result<(String, String), std::io::Error> {
    let data = fs::read(&att.local_path)?;
    let mime = att
        .mime_type
        .clone()
        .unwrap_or_else(|| guess_mime_from_path(&att.local_path));
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
    Ok((mime, b64))
}

fn guess_mime_from_path(path: &str) -> String {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "json" => "application/json",
        "csv" => "text/csv",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// The central dispatcher that every platform adapter calls into.
pub struct Dispatcher {
    session_manager: Arc<SessionManager>,
    pairing_manager: Arc<PairingManager>,
    skills: Option<Arc<SkillRegistry>>,
    router: Option<Arc<Mutex<ModelRouter>>>,
    usage: Option<Arc<UsageTracker>>,
    message_log: Option<Arc<MessageLog>>,
    session_store: Option<Arc<SessionStore>>,
    push: Option<Arc<PushService>>,
    memory: Option<Arc<MemoryEngine>>,
    plugin_manager: Option<Arc<StdMutex<PluginManager>>>,
    gemini_ocr: Option<GeminiOcrClient>,
}

impl Dispatcher {
    pub fn new(
        session_manager: Arc<SessionManager>,
        pairing_manager: Arc<PairingManager>,
    ) -> Self {
        Self {
            session_manager,
            pairing_manager,
            skills: None,
            router: None,
            usage: None,
            message_log: None,
            session_store: None,
            push: None,
            memory: None,
            plugin_manager: None,
            gemini_ocr: None,
        }
    }

    /// Set the skill registry for skill-related commands.
    pub fn with_skills(mut self, skills: Arc<SkillRegistry>) -> Self {
        self.skills = Some(skills);
        self
    }

    /// Set the model router for model-related commands.
    pub fn with_router(mut self, router: Arc<Mutex<ModelRouter>>) -> Self {
        self.router = Some(router);
        self
    }

    /// Set the usage tracker for recording API calls.
    pub fn with_usage(mut self, usage: Arc<UsageTracker>) -> Self {
        self.usage = Some(usage);
        self
    }

    /// Set the message log for the /logging command.
    pub fn with_message_log(mut self, log: Arc<MessageLog>) -> Self {
        self.message_log = Some(log);
        self
    }

    /// Set the session store for /new persistence.
    pub fn with_session_store(mut self, store: Arc<SessionStore>) -> Self {
        self.session_store = Some(store);
        self
    }

    /// Set the push service for pairing registration.
    pub fn with_push(mut self, push: Arc<PushService>) -> Self {
        self.push = Some(push);
        self
    }

    /// Set the memory engine for /remember, /recall, /forget commands.
    pub fn with_memory(mut self, memory: Arc<MemoryEngine>) -> Self {
        self.memory = Some(memory);
        self
    }

    /// Set the plugin manager for message filtering.
    pub fn with_plugin_manager(mut self, pm: Arc<StdMutex<PluginManager>>) -> Self {
        self.plugin_manager = Some(pm);
        self
    }

    /// Set the Gemini OCR client for /ocr photo processing.
    pub fn with_gemini_ocr(mut self, client: GeminiOcrClient) -> Self {
        self.gemini_ocr = Some(client);
        self
    }

    /// Handle an incoming message from any platform.
    ///
    /// Returns the response to send back, or None if no response is expected.
    pub async fn handle_message(
        &self,
        msg: IncomingMessage,
    ) -> Option<OutgoingMessage> {
        let i18n = I18n::new(msg.lang);

        // ── Intercept: /ocr with photo → Gemini OCR pipeline ──
        if msg.text.trim_start().starts_with("/ocr") {
            if let Some(ref att) = msg.attachment {
                if let Some(ref gemini) = self.gemini_ocr {
                    let mime = att.mime_type.as_deref().unwrap_or("image/jpeg");
                    match fs::read(&att.local_path) {
                        Ok(bytes) => {
                            info!(size = bytes.len(), "OCR request via Telegram");
                            match gemini.extract_receipts(&bytes, mime).await {
                                Ok(results) => {
                                    let total = results.len();
                                    let passed = results.iter().filter(|(_, v)| v.passed).count();
                                    let mut text = format!("📊 *Extracted {total} receipts* ({passed} passed)\n");
                                    for (i, (_receipt, verdict)) in results.iter().enumerate() {
                                        let icon = if verdict.passed { "✅" } else { "❌" };
                                        text.push_str(&format!(
                                            "\n{icon} #{} *{}* — {}/100\n   Total: €{:.2}",
                                            i + 1, verdict.store_name, verdict.score, verdict.total
                                        ));
                                        if !verdict.issues.is_empty() {
                                            text.push_str(&format!("\n   ⚠️  {}", verdict.issues.join("; ")));
                                        }
                                    }
                                    return Some(OutgoingMessage { chat_id: msg.chat_id.clone(), text, markdown: true, inline_keyboard: None });
                                }
                                Err(e) => {
                                    return Some(OutgoingMessage {
                                        chat_id: msg.chat_id.clone(),
                                        text: format!("OCR failed: {e}"),
                                        markdown: false,
                inline_keyboard: None,
                                    });
                                }
                            }
                        }
                        Err(e) => {
                            return Some(OutgoingMessage {
                                chat_id: msg.chat_id.clone(),
                                text: format!("Failed to read photo: {e}"),
                                markdown: false,
                inline_keyboard: None,
                            });
                        }
                    }
                } else {
                    return Some(OutgoingMessage {
                        chat_id: msg.chat_id.clone(),
                        text: "OCR is not configured. Set GOOGLE_API_KEY.".into(),
                        markdown: false,
                inline_keyboard: None,
                    });
                }
            }
        }

        // Check pairing
        if !self.pairing_manager.is_paired(&msg.user_id) {
            return Some(OutgoingMessage {
                chat_id: msg.chat_id.clone(),
                text: i18n.t("not_paired"),
                markdown: false,
                inline_keyboard: None,
            });
        }

        // Build session key: "platform:chat_id"
        let session_key = format!("{}:{}", msg.platform, msg.chat_id);

        // Get or create session
        let session = match self
            .session_manager
            .get_or_create(&session_key, &msg.tenant_id)
            .await
        {
            Ok(session) => session,
            Err(e) => {
                error!(
                    session_key = %session_key,
                    error = %e,
                    "Failed to create session"
                );
                return Some(OutgoingMessage {
                    chat_id: msg.chat_id.clone(),
                    text: i18n.tf("error", &[("error", &e.to_string())]),
                    markdown: false,
                inline_keyboard: None,
                });
            }
        };

        info!(
            session_key = %session_key,
            session_id = %session.session_id,
            user = %msg.user_id,
            text_len = msg.text.len(),
            "Dispatching message"
        );

        // Process attachment if present — read file, base64-encode
        let images: Vec<(String, String)> = if let Some(ref att) = msg.attachment {
            match encode_attachment(att) {
                Ok(img) => vec![img],
                Err(e) => {
                    error!(
                        attachment_path = %att.local_path,
                        error = %e,
                        "Failed to encode attachment"
                    );
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };

        // ── Plugin pipeline: redact PII/secrets ──
        let msg_text = if let Some(ref pm) = self.plugin_manager {
            let mut pipeline = pm.lock().unwrap();
            let mut plugin_msg = ohagent_plugins::PluginMessage::new(
                msg.text.clone(),
                msg.tenant_id.clone(),
                msg.platform.clone(),
            );
            match pipeline.run_pipeline(plugin_msg) {
                Ok(Some(processed)) => {
                    if !processed.redaction_log.is_empty() {
                        info!(
                            redactions = processed.redaction_log.len(),
                            "Plugin pipeline redacted sensitive data"
                        );
                    }
                    processed.text
                }
                Ok(None) => {
                    warn!("Plugin pipeline blocked the message");
                    return Some(OutgoingMessage {
                        chat_id: msg.chat_id.clone(),
                        text: "Message blocked by security policy.".into(),
                        markdown: false,
                inline_keyboard: None,
                    });
                }
                Err(e) => {
                    warn!(error = %e, "Plugin pipeline error — passing through");
                    msg.text.clone()
                }
            }
        } else {
            msg.text.clone()
        };

        // Send message: route through tool-augmented path when:
        // a) no attachments, and b) bridge has registered tools
        let send_result = if images.is_empty() {
            session.send_message_with_tools(&msg_text).await
        } else {
            session.send_message_with_images(&msg.text, images).await
                .map(|_| String::new())
        };

        match send_result {
            Ok(response_text) => {
                info!(session_key = %session_key, "Message processed");
                if !response_text.is_empty() {
                    tracing::debug!(
                        session_key = %session_key,
                        response_len = response_text.len(),
                        "Tool-augmented response"
                    );
                    // TODO: return response to user when streaming is wired
                }

                // Record usage (rough tok estimate: ~4 chars/tok)
                if let Some(ref usage) = self.usage {
                    let estimated_input_tokens = (msg.text.len() as u32 / 4).max(1);
                    let model_id = "auto"; // model routed by bridge
                    let model_display = "Auto-selected";
                    let caps = vec!["general_chat".to_string()];
                    let _ = usage.record(
                        &msg.tenant_id,
                        &session.session_id,
                        model_id,
                        model_display,
                        &caps,
                        estimated_input_tokens,
                        estimated_input_tokens / 2, // rough output estimate
                        0, // duration unknown
                    );
                }

                None // No explicit response needed for now; streaming will come later.
            }
            Err(e) => {
                error!(
                    session_key = %session_key,
                    error = %e,
                    "Message processing failed"
                );
                Some(OutgoingMessage {
                    chat_id: msg.chat_id.clone(),
                    text: i18n.tf("error", &[("error", &e.to_string())]),
                    markdown: false,
                inline_keyboard: None,
                })
            }
        }
    }

    /// Handle a command (starts with `/`).
    ///
    /// Commands bypass the normal message pipeline.
    pub async fn handle_command(
        &self,
        msg: IncomingMessage,
        command: &str,
        args: &str,
    ) -> Option<OutgoingMessage> {
        let i18n = I18n::new(msg.lang);

        match command {
            "start" => {
                Some(OutgoingMessage {
                    chat_id: msg.chat_id.clone(),
                    text: i18n.t("greeting"),
                    markdown: false,
                inline_keyboard: None,
                })
            }

            "pair" => {
                let code = self
                    .pairing_manager
                    .generate_code(&msg.user_id, &msg.platform);
                Some(OutgoingMessage {
                    chat_id: msg.chat_id.clone(),
                    text: i18n.tf("pairing_code_sent", &[
                        ("code", &code),
                        ("minutes", "10"),
                    ]),
                    markdown: true,
                inline_keyboard: None,
                })
            }

            "confirm" => {
                let code = args.trim();
                if code.is_empty() {
                    return Some(OutgoingMessage {
                        chat_id: msg.chat_id.clone(),
                        text: "Usage: /confirm <CODE>".into(),
                        markdown: false,
                inline_keyboard: None,
                    });
                }
                match self.pairing_manager.confirm_code(
                    &msg.user_id,
                    code,
                    msg.lang.as_str(),
                ) {
                    Ok(ref paired) => {
                        // Register for push notifications
                        if let Some(ref push) = self.push {
                            push.register(&paired.tenant_id, &msg.chat_id);
                        }
                        Some(OutgoingMessage {
                            chat_id: msg.chat_id.clone(),
                            text: i18n.t("pairing_success"),
                            markdown: false,
                inline_keyboard: None,
                        })
                    }
                    Err(e) => Some(OutgoingMessage {
                        chat_id: msg.chat_id.clone(),
                        text: e,
                        markdown: false,
                inline_keyboard: None,
                    }),
                }
            }

            "help" => {
                Some(OutgoingMessage {
                    chat_id: msg.chat_id.clone(),
                    text: i18n.t("help"),
                    markdown: true,
                inline_keyboard: None,
                })
            }

            "new" => {
                let session_key = format!("{}:{}", msg.platform, msg.chat_id);
                self.session_manager.reset(&session_key).await;
                // Also clear persistent session
                if let Some(ref ss) = self.session_store {
                    let _ = ss.delete_all_for_tenant(&msg.tenant_id);
                }
                Some(OutgoingMessage {
                    chat_id: msg.chat_id.clone(),
                    text: i18n.t("new_session"),
                    markdown: false,
                inline_keyboard: None,
                })
            }

            "lang" => {
                // Toggle language: en → lv → ru → en
                let new_lang = match msg.lang.as_str() {
                    "lv" => "ru",
                    "ru" => "en",
                    _ => "lv",
                };
                // Update the pairing record
                if let Some(mut user) = self.pairing_manager.get(&msg.user_id) {
                    user.lang = new_lang.to_string();
                    // Re-insert updated user
                    self.pairing_manager
                        .confirm_code(&msg.user_id, "__skip__", new_lang)
                        .ok();
                }
                let new_i18n = I18n::new(crate::i18n::Lang::from_code(Some(new_lang)));
                Some(OutgoingMessage {
                    chat_id: msg.chat_id.clone(),
                    text: new_i18n.t("lang_changed"),
                    markdown: false,
                inline_keyboard: None,
                })
            }

            "stop" => {
                let session_key = format!("{}:{}", msg.platform, msg.chat_id);
                if let Some(session) = self.session_manager.get(&session_key) {
                    session.interrupt().await;
                    info!(
                        session_key = %session_key,
                        "Agent task interrupted"
                    );
                }
                Some(OutgoingMessage {
                    chat_id: msg.chat_id.clone(),
                    text: i18n.t("task_stopped"),
                    markdown: false,
                inline_keyboard: None,
                })
            }

            "model" => {
                match &self.router {
                    Some(router) => {
                        let args_trim = args.trim();
                        let parts: Vec<&str> =
                            args_trim.split_whitespace().collect();
                        let tenant = &msg.tenant_id;

                        match parts.first().copied() {
                            Some("set") => {
                                // /model set <capability> <model_id>
                                if parts.len() < 3 {
                                    return Some(OutgoingMessage {
                                        chat_id: msg.chat_id.clone(),
                                        text: "Usage: /model set <capability> <model_id>\n\
                                               Capabilities: coding, reasoning, analysis, general_chat, creative_writing, image_gen, video_gen\n\
                                               Use /model to see available models."
                                            .into(),
                                        markdown: false,
                inline_keyboard: None,
                                    });
                                }
                                let cap = parts[1];
                                let model_id = parts[2];

                                let mut r = router.lock().unwrap();
                                match r.set_pref(tenant, cap, model_id) {
                                    Ok(()) => Some(OutgoingMessage {
                                        chat_id: msg.chat_id.clone(),
                                        text: format!(
                                            "*Preference saved:* `{cap}` → `{model_id}`",
                                        ),
                                        markdown: true,
                inline_keyboard: None,
                                    }),
                                    Err(e) => Some(OutgoingMessage {
                                        chat_id: msg.chat_id.clone(),
                                        text: format!("Error: {e}"),
                                        markdown: false,
                inline_keyboard: None,
                                    }),
                                }
                            }

                            Some("clear") => {
                                // /model clear [capability]
                                let cap = parts.get(1).copied();
                                let mut r = router.lock().unwrap();
                                match r.clear_pref(tenant, cap) {
                                    Ok(()) => {
                                        let msg_text = match cap {
                                            Some(c) => format!(
                                                "*Cleared* preference for `{c}`"
                                            ),
                                            None => "*Cleared* all your model preferences."
                                                .into(),
                                        };
                                        Some(OutgoingMessage {
                                            chat_id: msg.chat_id.clone(),
                                            text: msg_text,
                                            markdown: true,
                inline_keyboard: None,
                                        })
                                    }
                                    Err(e) => Some(OutgoingMessage {
                                        chat_id: msg.chat_id.clone(),
                                        text: format!("Error: {e}"),
                                        markdown: false,
                inline_keyboard: None,
                                    }),
                                }
                            }

                            Some("list") | None => {
                                let r = router.lock().unwrap();
                                let models = r.list_models().to_vec();
                                let available: Vec<String> = models
                                    .iter()
                                    .filter(|m| std::env::var(&m.api_key_env).is_ok())
                                    .map(|m| format!("• *{}* ({}) — {}", m.display, m.cost_tier, m.capabilities.join(", ")))
                                    .collect();

                                let prefs = r.list_prefs(tenant);
                                drop(r);

                                let mut text = "*Model Router Status*\n\n".to_string();
                                text.push_str(&format!("{} models loaded, {} available\n",
                                    models.len(), available.len()));

                                // Show user's preferences
                                if !prefs.is_empty() {
                                    text.push_str("\n*Your preferences:*\n");
                                    let mut sorted: Vec<_> = prefs.iter().collect();
                                    sorted.sort_by_key(|(k, _)| *k);
                                    for (cap, model) in sorted {
                                        text.push_str(&format!("• `{cap}` → `{model}`\n"));
                                    }
                                    text.push_str("\nTo clear: /model clear <capability>\n");
                                    text.push_str("To clear all: /model clear\n");
                                } else {
                                    text.push_str("\n*No personal preferences set.*\n");
                                    text.push_str("Set with: /model set <capability> <model_name>\n");
                                }

                                text.push_str("\n*Available models:*\n");
                                text.push_str(&available.join("\n"));
                                text.push_str("\n\nModels are auto-selected based on your task type.");

                                Some(OutgoingMessage {
                                    chat_id: msg.chat_id.clone(),
                                    text,
                                    markdown: true,
                inline_keyboard: None,
                                })
                            }

                            _ => Some(OutgoingMessage {
                                chat_id: msg.chat_id.clone(),
                                text: "Usage: /model [set|clear|list]\n\
                                       /model — show models & preferences\n\
                                       /model set <capability> <model_id> — set preferred model\n\
                                       /model clear [capability] — clear preference(s)\n\
                                       /model list — list preferences"
                                    .into(),
                                markdown: false,
                inline_keyboard: None,
                            }),
                        }
                    }
                    None => Some(OutgoingMessage {
                        chat_id: msg.chat_id.clone(),
                        text: "Model router is not configured. Using default provider.".into(),
                        markdown: false,
                inline_keyboard: None,
                    }),
                }
            }

            "skills" => {
                // List active skills for this tenant
                match &self.skills {
                    Some(skills) => {
                        match skills.list(&msg.tenant_id, None, 20) {
                            Ok(list) if list.is_empty() => {
                                Some(OutgoingMessage {
                                    chat_id: msg.chat_id.clone(),
                                    text: i18n.t("skills_none"),
                                    markdown: false,
                inline_keyboard: None,
                                })
                            }
                            Ok(list) => {
                                let mut text = i18n.t("skills_header");
                                for s in &list {
                                    text.push_str(&format!(
                                        "\n• *{}* (v{}) — {} — {:.0}%",
                                        s.name,
                                        s.version,
                                        s.status,
                                        s.quality_score * 100.0,
                                    ));
                                }
                                Some(OutgoingMessage {
                                    chat_id: msg.chat_id.clone(),
                                    text,
                                    markdown: true,
                inline_keyboard: None,
                                })
                            }
                            Err(e) => Some(OutgoingMessage {
                                chat_id: msg.chat_id.clone(),
                                text: i18n.tf("error", &[("error", &e.to_string())]),
                                markdown: false,
                inline_keyboard: None,
                            }),
                        }
                    }
                    None => Some(OutgoingMessage {
                        chat_id: msg.chat_id.clone(),
                        text: i18n.t("skills_unavailable"),
                        markdown: false,
                inline_keyboard: None,
                    }),
                }
            }

            "skill" => {
                let skill_name = args.trim();
                if skill_name.is_empty() {
                    return Some(OutgoingMessage {
                        chat_id: msg.chat_id.clone(),
                        text: "Usage: /skill <name>".into(),
                        markdown: false,
                inline_keyboard: None,
                    });
                }
                match &self.skills {
                    Some(skills) => {
                        match skills.find_by_name(&msg.tenant_id, skill_name) {
                            Ok(Some(s)) => {
                                let triggers = s.triggers.join(", ");
                                let tags = s.tags.join(", ");
                                let text = format!(
                                    "*{name}* (v{ver})\n\
                                     Status: {status}\n\
                                     Quality: {quality:.0}%\n\
                                     Used: {used} times\n\
                                     \n{desc}\n\
                                     \nTriggers: {triggers}\n\
                                     Tags: {tags}",
                                    name = s.name,
                                    ver = s.version,
                                    status = s.status,
                                    quality = s.quality_score * 100.0,
                                    used = s.use_count,
                                    desc = s.description,
                                    triggers = triggers,
                                    tags = tags,
                                );
                                Some(OutgoingMessage {
                                    chat_id: msg.chat_id.clone(),
                                    text,
                                    markdown: true,
                inline_keyboard: None,
                                })
                            }
                            Ok(None) => Some(OutgoingMessage {
                                chat_id: msg.chat_id.clone(),
                                text: i18n.tf("skill_not_found", &[("name", skill_name)]),
                                markdown: false,
                inline_keyboard: None,
                            }),
                            Err(e) => Some(OutgoingMessage {
                                chat_id: msg.chat_id.clone(),
                                text: i18n.tf("error", &[("error", &e.to_string())]),
                                markdown: false,
                inline_keyboard: None,
                            }),
                        }
                    }
                    None => Some(OutgoingMessage {
                        chat_id: msg.chat_id.clone(),
                        text: i18n.t("skills_unavailable"),
                        markdown: false,
                inline_keyboard: None,
                    }),
                }
            }

            "skilluse" => {
                let skill_name = args.trim();
                if skill_name.is_empty() {
                    return Some(OutgoingMessage {
                        chat_id: msg.chat_id.clone(),
                        text: "Usage: /skilluse <name>".into(),
                        markdown: false,
                inline_keyboard: None,
                    });
                }
                match &self.skills {
                    Some(skills) => {
                        match skills.find_by_name(&msg.tenant_id, skill_name) {
                            Ok(Some(s)) => {
                                match ohagent_skills::evaluator::record_success(
                                    skills,
                                    &s.id,
                                    &format!("{}:{}", msg.platform, msg.chat_id),
                                    &msg.tenant_id,
                                    None,
                                ) {
                                    Ok(()) => Some(OutgoingMessage {
                                        chat_id: msg.chat_id.clone(),
                                        text: i18n.tf("skill_used", &[("name", &s.name)]),
                                        markdown: false,
                inline_keyboard: None,
                                    }),
                                    Err(e) => Some(OutgoingMessage {
                                        chat_id: msg.chat_id.clone(),
                                        text: i18n.tf("error", &[("error", &e.to_string())]),
                                        markdown: false,
                inline_keyboard: None,
                                    }),
                                }
                            }
                            Ok(None) => Some(OutgoingMessage {
                                chat_id: msg.chat_id.clone(),
                                text: i18n.tf("skill_not_found", &[("name", skill_name)]),
                                markdown: false,
                inline_keyboard: None,
                            }),
                            Err(e) => Some(OutgoingMessage {
                                chat_id: msg.chat_id.clone(),
                                text: i18n.tf("error", &[("error", &e.to_string())]),
                                markdown: false,
                inline_keyboard: None,
                            }),
                        }
                    }
                    None => Some(OutgoingMessage {
                        chat_id: msg.chat_id.clone(),
                        text: i18n.t("skills_unavailable"),
                        markdown: false,
                inline_keyboard: None,
                    }),
                }
            }

            "logging" => {
                match &self.message_log {
                    Some(log) => {
                        let sub = args.trim();
                        match sub {
                            "on" => {
                                let _ = log.set_enabled(&msg.tenant_id, true);
                                Some(OutgoingMessage {
                                    chat_id: msg.chat_id.clone(),
                                    text: i18n.t("logging_on"),
                                    markdown: false,
                inline_keyboard: None,
                                })
                            }
                            "off" => {
                                let _ = log.set_enabled(&msg.tenant_id, false);
                                Some(OutgoingMessage {
                                    chat_id: msg.chat_id.clone(),
                                    text: i18n.t("logging_off"),
                                    markdown: false,
                inline_keyboard: None,
                                })
                            }
                            _ => {
                                let enabled = log.is_enabled_for(&msg.tenant_id);
                                Some(OutgoingMessage {
                                    chat_id: msg.chat_id.clone(),
                                    text: if enabled {
                                        i18n.t("logging_status_on")
                                    } else {
                                        i18n.t("logging_status_off")
                                    },
                                    markdown: false,
                inline_keyboard: None,
                                })
                            }
                        }
                    }
                    None => Some(OutgoingMessage {
                        chat_id: msg.chat_id.clone(),
                        text: "Message logging is not configured.".into(),
                        markdown: false,
                inline_keyboard: None,
                    }),
                }
            }

            "remember" => {
                let content = args.trim();
                if content.is_empty() {
                    return Some(OutgoingMessage {
                        chat_id: msg.chat_id.clone(),
                        text: "Usage: /remember <text to remember>".into(),
                        markdown: false,
                inline_keyboard: None,
                    });
                }
                match &self.memory {
                    Some(memory) => {
                        use chrono::Utc;
                        let now = Utc::now();
                        let entry = MemoryEntry {
                            id: uuid::Uuid::new_v4().to_string(),
                            tenant_id: msg.tenant_id.clone(),
                            session_id: format!("{}:{}", msg.platform, msg.chat_id),
                            content: content.to_string(),
                            source: ohagent_memory::models::MemorySource::Explicit,
                            importance: 0.5,
                            tags: vec!["manual".into(), "telegram".into()],
                            embedding: None,
                            created_at: now,
                            last_accessed_at: now,
                            access_count: 0,
                        };
                        match memory.remember(entry) {
                            Ok(saved) => Some(OutgoingMessage {
                                chat_id: msg.chat_id.clone(),
                                text: format!("*Remembered:* _{content}_\nID: `{}`", saved.id),
                                markdown: true,
                inline_keyboard: None,
                            }),
                            Err(e) => Some(OutgoingMessage {
                                chat_id: msg.chat_id.clone(),
                                text: format!("Failed to remember: {e}"),
                                markdown: false,
                inline_keyboard: None,
                            }),
                        }
                    }
                    None => Some(OutgoingMessage {
                        chat_id: msg.chat_id.clone(),
                        text: "Memory engine is not configured.".into(),
                        markdown: false,
                inline_keyboard: None,
                    }),
                }
            }

            "recall" | "search" => {
                let query = args.trim();
                if query.is_empty() {
                    return Some(OutgoingMessage {
                        chat_id: msg.chat_id.clone(),
                        text: "Usage: /recall <search query>".into(),
                        markdown: false,
                inline_keyboard: None,
                    });
                }
                match &self.memory {
                    Some(memory) => match memory.search(&msg.tenant_id, query) {
                        Ok(results) if results.is_empty() => Some(OutgoingMessage {
                            chat_id: msg.chat_id.clone(),
                            text: format!("_No memories found for \"{query}\"._"),
                            markdown: true,
                inline_keyboard: None,
                        }),
                        Ok(results) => {
                            let mut text = format!("*Found {} memories:*\n", results.len());
                            for (i, r) in results.iter().take(10).enumerate() {
                                text.push_str(&format!(
                                    "\n{}. {} _(score: {:.2})_\n  `{}`",
                                    i + 1,
                                    &r.entry.content[..r.entry.content.len().min(200)],
                                    r.combined_score,
                                    r.entry.id,
                                ));
                            }
                            Some(OutgoingMessage {
                                chat_id: msg.chat_id.clone(),
                                text,
                                markdown: true,
                inline_keyboard: None,
                            })
                        }
                        Err(e) => Some(OutgoingMessage {
                            chat_id: msg.chat_id.clone(),
                            text: format!("Memory search failed: {e}"),
                            markdown: false,
                inline_keyboard: None,
                        }),
                    },
                    None => Some(OutgoingMessage {
                        chat_id: msg.chat_id.clone(),
                        text: "Memory engine is not configured.".into(),
                        markdown: false,
                inline_keyboard: None,
                    }),
                }
            }

            "forget" => {
                let id = args.trim();
                if id.is_empty() {
                    return Some(OutgoingMessage {
                        chat_id: msg.chat_id.clone(),
                        text: "Usage: /forget <memory ID>".into(),
                        markdown: false,
                inline_keyboard: None,
                    });
                }
                match &self.memory {
                    Some(memory) => match memory.forget(id) {
                        Ok(()) => Some(OutgoingMessage {
                            chat_id: msg.chat_id.clone(),
                            text: format!("*Forgotten:* `{id}`"),
                            markdown: true,
                inline_keyboard: None,
                        }),
                        Err(e) => Some(OutgoingMessage {
                            chat_id: msg.chat_id.clone(),
                            text: format!("Failed to forget: {e}"),
                            markdown: false,
                inline_keyboard: None,
                        }),
                    },
                    None => Some(OutgoingMessage {
                        chat_id: msg.chat_id.clone(),
                        text: "Memory engine is not configured.".into(),
                        markdown: false,
                inline_keyboard: None,
                    }),
                }
            }

            "ocr" => {
                // /ocr — process receipt photo via Gemini OCR pipeline
                let text: String = "📸 *Receipt OCR*\n\nSend me a photo of your receipts and I'll extract the data.\n\nJust attach a photo with caption `/ocr` or send the photo right after this message.".into();
                Some(OutgoingMessage {
                    chat_id: msg.chat_id.clone(),
                    text,
                    markdown: true,
                inline_keyboard: None,
                })
            }

            _ => {
                // Unknown command — treat as regular message
                None
            }
        }
    }
}
