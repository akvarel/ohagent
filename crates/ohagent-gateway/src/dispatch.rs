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
use ohagent_core::model_router::ModelRouter;
use ohagent_skills::registry::SkillRegistry;

/// The central dispatcher that every platform adapter calls into.
pub struct Dispatcher {
    session_manager: Arc<SessionManager>,
    pairing_manager: Arc<PairingManager>,
    skills: Option<Arc<SkillRegistry>>,
    router: Option<Arc<ModelRouter>>,
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
        }
    }

    /// Set the skill registry for skill-related commands.
    pub fn with_skills(mut self, skills: Arc<SkillRegistry>) -> Self {
        self.skills = Some(skills);
        self
    }

    /// Set the model router for model-related commands.
    pub fn with_router(mut self, router: Arc<ModelRouter>) -> Self {
        self.router = Some(router);
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

            "model" => {
                match &self.router {
                    Some(router) => {
                        let _diag = router.diagnostics();
                        let models = router.list_models();
                        let available: Vec<String> = models
                            .iter()
                            .filter(|m| std::env::var(&m.api_key_env).is_ok())
                            .map(|m| format!("• *{}* ({}) — {}", m.display, m.cost_tier, m.capabilities.join(", ")))
                            .collect();

                        let mut text = "*Model Router Status*\n\n".to_string();
                        text.push_str(&format!("{} models loaded, {} available\n\n",
                            models.len(), available.len()));
                        text.push_str("*Available models:*\n");
                        text.push_str(&available.join("\n"));
                        text.push_str("\n\nModels are auto-selected based on your task type.");

                        Some(OutgoingMessage {
                            chat_id: msg.chat_id.clone(),
                            text,
                            markdown: true,
                        })
                    }
                    None => Some(OutgoingMessage {
                        chat_id: msg.chat_id.clone(),
                        text: "Model router is not configured. Using default provider.".into(),
                        markdown: false,
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
                                })
                            }
                            Err(e) => Some(OutgoingMessage {
                                chat_id: msg.chat_id.clone(),
                                text: i18n.tf("error", &[("error", &e.to_string())]),
                                markdown: false,
                            }),
                        }
                    }
                    None => Some(OutgoingMessage {
                        chat_id: msg.chat_id.clone(),
                        text: i18n.t("skills_unavailable"),
                        markdown: false,
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
                                })
                            }
                            Ok(None) => Some(OutgoingMessage {
                                chat_id: msg.chat_id.clone(),
                                text: i18n.tf("skill_not_found", &[("name", skill_name)]),
                                markdown: false,
                            }),
                            Err(e) => Some(OutgoingMessage {
                                chat_id: msg.chat_id.clone(),
                                text: i18n.tf("error", &[("error", &e.to_string())]),
                                markdown: false,
                            }),
                        }
                    }
                    None => Some(OutgoingMessage {
                        chat_id: msg.chat_id.clone(),
                        text: i18n.t("skills_unavailable"),
                        markdown: false,
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
                                    }),
                                    Err(e) => Some(OutgoingMessage {
                                        chat_id: msg.chat_id.clone(),
                                        text: i18n.tf("error", &[("error", &e.to_string())]),
                                        markdown: false,
                                    }),
                                }
                            }
                            Ok(None) => Some(OutgoingMessage {
                                chat_id: msg.chat_id.clone(),
                                text: i18n.tf("skill_not_found", &[("name", skill_name)]),
                                markdown: false,
                            }),
                            Err(e) => Some(OutgoingMessage {
                                chat_id: msg.chat_id.clone(),
                                text: i18n.tf("error", &[("error", &e.to_string())]),
                                markdown: false,
                            }),
                        }
                    }
                    None => Some(OutgoingMessage {
                        chat_id: msg.chat_id.clone(),
                        text: i18n.t("skills_unavailable"),
                        markdown: false,
                    }),
                }
            }

            _ => {
                // Unknown command — treat as regular message
                None
            }
        }
    }
}
