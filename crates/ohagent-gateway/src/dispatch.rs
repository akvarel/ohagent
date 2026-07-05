//! Message dispatch — routes incoming messages to JcodeBridge sessions.
//!
//! Handles the full lifecycle: receive → check pairing → get/create session →
//! send "thinking" → process → send response.

use std::sync::Arc;
use tracing::{error, info};

use crate::adapter::{IncomingMessage, OutgoingMessage};
use crate::i18n::I18n;
use crate::pairing::PairingManager;
use crate::session::SessionManager;

/// The central dispatcher that every platform adapter calls into.
pub struct Dispatcher {
    session_manager: Arc<SessionManager>,
    pairing_manager: Arc<PairingManager>,
}

impl Dispatcher {
    pub fn new(
        session_manager: Arc<SessionManager>,
        pairing_manager: Arc<PairingManager>,
    ) -> Self {
        Self {
            session_manager,
            pairing_manager,
        }
    }

    /// Handle an incoming message from any platform.
    ///
    /// Returns the response to send back, or None if no response is expected.
    pub async fn handle_message(
        &self,
        msg: IncomingMessage,
    ) -> Option<OutgoingMessage> {
        let i18n = I18n::new(msg.lang);

        // Check pairing
        if !self.pairing_manager.is_paired(&msg.user_id) {
            return Some(OutgoingMessage {
                chat_id: msg.chat_id.clone(),
                text: i18n.t("not_paired"),
                markdown: false,
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

        // Send the message to the agent
        match session.send_message(&msg.text).await {
            Ok(()) => {
                info!(session_key = %session_key, "Message processed");
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
                })
            }

            "confirm" => {
                let code = args.trim();
                if code.is_empty() {
                    return Some(OutgoingMessage {
                        chat_id: msg.chat_id.clone(),
                        text: "Usage: /confirm <CODE>".into(),
                        markdown: false,
                    });
                }
                match self.pairing_manager.confirm_code(
                    &msg.user_id,
                    code,
                    msg.lang.as_str(),
                ) {
                    Ok(_) => Some(OutgoingMessage {
                        chat_id: msg.chat_id.clone(),
                        text: i18n.t("pairing_success"),
                        markdown: false,
                    }),
                    Err(e) => Some(OutgoingMessage {
                        chat_id: msg.chat_id.clone(),
                        text: e,
                        markdown: false,
                    }),
                }
            }

            "help" => {
                Some(OutgoingMessage {
                    chat_id: msg.chat_id.clone(),
                    text: i18n.t("help"),
                    markdown: true,
                })
            }

            "new" => {
                let session_key = format!("{}:{}", msg.platform, msg.chat_id);
                self.session_manager.reset(&session_key).await;
                Some(OutgoingMessage {
                    chat_id: msg.chat_id.clone(),
                    text: i18n.t("new_session"),
                    markdown: false,
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
                })
            }

            "stop" => {
                // TODO: Interrupt the current session
                Some(OutgoingMessage {
                    chat_id: msg.chat_id.clone(),
                    text: i18n.t("task_stopped"),
                    markdown: false,
                })
            }

            _ => {
                // Unknown command — treat as regular message
                None
            }
        }
    }
}
