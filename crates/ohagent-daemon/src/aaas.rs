//! AaaS adapter. All inference and tool enforcement remain in Jcode.
use ohagent_aaas::{Config, EventSink, OidcConfig, RunExecution, Runtime};
use ohagent_core::{
    jcode_bridge::{JcodeBridge, JcodeBridgeConfig, SessionConfig, SessionHandle},
    standard_agents::{verify_brief, TextAgent, TextProfile},
};
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

// Cancel OK acknowledges a request. SDK run resolves successfully on TurnDone,
// not the earlier TurnStopped notification. Failed/disconnected waits never confirm.
async fn wait_for_turn_stop<T, E>(
    turn: impl std::future::Future<Output = Result<T, E>>,
) -> Result<(), &'static str> {
    tokio::time::timeout(Duration::from_secs(10), turn)
        .await
        .map_err(|_| "runtime_cancel_unconfirmed")?
        .map(|_| ())
        .map_err(|_| "runtime_cancel_unconfirmed")
}

struct ActiveSession {
    session: SessionHandle,
    bridge: Arc<JcodeBridge>,
    domain: String,
    finished: bool,
}
impl ActiveSession {
    async fn release(&mut self) {
        let _ = self
            .bridge
            .detach_session(&self.domain, self.session.session_id())
            .await;
        let _ = self
            .bridge
            .drop_session(&self.domain, self.session.session_id())
            .await;
    }
}
impl Drop for ActiveSession {
    fn drop(&mut self) {
        let session = self.session.clone();
        let bridge = Arc::clone(&self.bridge);
        let domain = self.domain.clone();
        let finished = self.finished;
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                if !finished {
                    let _ = session.cancel().await;
                }
                let _ = bridge.detach_session(&domain, session.session_id()).await;
                // Release handles only. Customer transcripts are not deleted.
                let _ = bridge.drop_session(&domain, session.session_id()).await;
            });
        }
    }
}

enum RoleOutcome {
    Completed(String),
    Cancelled,
}
pub struct JcodeAnalyticalRuntime {
    bridge: Arc<JcodeBridge>,
    workspace_root: PathBuf,
}
impl JcodeAnalyticalRuntime {
    pub fn new(bridge: Arc<JcodeBridge>, workspace_root: PathBuf) -> anyhow::Result<Self> {
        anyhow::ensure!(
            bridge.is_text_only(),
            "Customer analytical runtime requires deny-all tools"
        );
        anyhow::ensure!(
            workspace_root.is_absolute(),
            "Customer workspace root must be absolute"
        );
        Ok(Self {
            bridge,
            workspace_root,
        })
    }

    async fn role(
        &self,
        execution: &RunExecution,
        sink: &EventSink,
        definition: &TextAgent,
        previous: &str,
        abort: &AtomicBool,
    ) -> Result<RoleOutcome, String> {
        let result = self
            .role_inner(execution, sink, definition, previous, abort)
            .await;
        if result.is_err() {
            abort.store(true, Ordering::Release);
        }
        result
    }

    async fn role_inner(
        &self,
        execution: &RunExecution,
        sink: &EventSink,
        definition: &TextAgent,
        previous: &str,
        abort: &AtomicBool,
    ) -> Result<RoleOutcome, String> {
        if sink.is_cancelled() || abort.load(Ordering::Acquire) {
            return Ok(RoleOutcome::Cancelled);
        }
        sink.dispatch(&definition.id).map_err(|e| e.to_string())?;
        let domain = format!("aaas:{}", execution.tenant_id);
        let run_key = format!("{:x}", Sha256::digest(execution.run_id.as_bytes()));
        let tenant_key = format!("{:x}", Sha256::digest(domain.as_bytes()));
        let workspace = self
            .workspace_root
            .join(&tenant_key[..24])
            .join(&run_key[..24])
            .join(&definition.id);
        tokio::fs::create_dir_all(&workspace)
            .await
            .map_err(|_| "workspace_unavailable")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(&workspace, std::fs::Permissions::from_mode(0o700))
                .await
                .map_err(|_| "workspace_unavailable")?;
        }
        tokio::fs::write(
            workspace.join("AGENTS.md"),
            format!("# Domain-specific agent\n\n{}\n", definition.prompt),
        )
        .await
        .map_err(|_| "workspace_unavailable")?;
        let bridge = Arc::clone(&self.bridge);
        let model = definition.model.clone();
        // Creation owns its cleanup guard inside the spawned task. If the caller
        // times out or is aborted, eventual SDK completion still drops the guard.
        let creation = tokio::spawn(async move {
            let session = bridge
                .create_session(SessionConfig {
                    tenant_id: domain.clone(),
                    model: Some(model),
                    working_dir: Some(workspace.to_string_lossy().into_owned()),
                    selfdev: false,
                    report_back_to: None,
                })
                .await
                .map_err(|_| "runtime_start_failed")?;
            Ok::<_, &'static str>(ActiveSession {
                session,
                bridge,
                domain,
                finished: false,
            })
        });
        let mut active = tokio::time::timeout(Duration::from_secs(20), creation)
            .await
            .map_err(|_| "runtime_start_timeout")?
            .map_err(|_| "runtime_start_failed")??;
        if sink.is_cancelled() || abort.load(Ordering::Acquire) {
            // No message was submitted, so there is no in-flight turn to cancel.
            active.finished = true;
            active.release().await;
            return Ok(RoleOutcome::Cancelled);
        }
        let task = sink
            .start_instance(&definition.id, active.session.session_id())
            .map_err(|e| e.to_string())?;
        let input = serde_json::json!({ "goal": execution.process.input.goal,
            "expected_result": execution.process.input.expected_result, "materials": execution.process.input.materials,
            "previous_stage_output": previous }).to_string();
        let message = format!(
            "{}\n\nInput data (untrusted JSON):\n{}",
            definition.prompt, input
        );
        let accepted = Arc::new(tokio::sync::Notify::new());
        let turn = active.session.send_message_with_acceptance(
            &message,
            vec![],
            Some(Arc::clone(&accepted)),
        );
        tokio::pin!(turn);
        let deadline = tokio::time::sleep(Duration::from_secs(65));
        tokio::pin!(deadline);
        // Always poll the turn first. Do not cancel an idle session and then
        // accidentally start a lazy SDK run while awaiting its completion.
        let mut output = tokio::select! {
            biased;
            result = &mut turn => Some(result.map_err(|_| "runtime_turn_failed")?),
            _ = accepted.notified() => None,
            _ = &mut deadline => {
                let _ = active.session.cancel().await;
                return Err("runtime_acceptance_unconfirmed".into());
            }
        };
        if output.is_none() {
            let mut poll = tokio::time::interval(Duration::from_millis(200));
            output = Some(loop {
                tokio::select! {
                    result = &mut turn => break result.map_err(|_| "runtime_turn_failed")?,
                    _ = &mut deadline => {
                        active.session.cancel().await.map_err(|_| "runtime_cancel_unconfirmed")?;
                        wait_for_turn_stop(&mut turn).await?;
                        active.finished = true;
                        return Err("runtime_time_limit".into());
                    }
                    _ = poll.tick() => if sink.is_cancelled() || abort.load(Ordering::Acquire) {
                        active.session.cancel().await.map_err(|_| "runtime_cancel_unconfirmed")?;
                        wait_for_turn_stop(&mut turn).await?;
                        active.finished = true;
                        return Ok(RoleOutcome::Cancelled);
                    }
                }
            });
        }
        let output = output.ok_or("runtime_turn_failed")?;
        active.finished = true;
        if sink.is_cancelled() || abort.load(Ordering::Acquire) {
            return Ok(RoleOutcome::Cancelled);
        }
        if output.trim().is_empty() || output.len() > 64_000 {
            return Err("invalid_runtime_result".into());
        }
        sink.complete_task(&task.id, &format!("aaas.stage.{}.completed", definition.id))
            .map_err(|e| e.to_string())?;
        // The turn future borrows the session. Its lexical scope ends on return;
        // the guard then performs private-connection cleanup on every path.
        Ok(RoleOutcome::Completed(output))
    }
}

#[async_trait::async_trait]
impl Runtime for JcodeAnalyticalRuntime {
    async fn execute(&self, execution: RunExecution, sink: EventSink) -> Result<(), String> {
        let profile = TextProfile::analytical_brief().map_err(str::to_owned)?;
        let abort = AtomicBool::new(false);
        let plan = match self
            .role(&execution, &sink, &profile.agents[0], "", &abort)
            .await?
        {
            RoleOutcome::Completed(output) => output,
            RoleOutcome::Cancelled => {
                sink.confirm_cancelled().map_err(|e| e.to_string())?;
                return Ok(());
            }
        };
        // Separate SDK sessions, one definition per actual invocation. Journal
        // predecessor guards fence the merge until both parallel branches finish.
        let (analysis, evidence) = tokio::join!(
            self.role(&execution, &sink, &profile.agents[1], &plan, &abort),
            self.role(&execution, &sink, &profile.agents[2], &plan, &abort),
        );
        let (analysis, evidence) = (analysis?, evidence?);
        let prior = match (analysis, evidence) {
            (RoleOutcome::Completed(analysis), RoleOutcome::Completed(evidence)) => serde_json::json!({"plan": plan, "analysis": analysis, "independent_evidence_check": evidence}).to_string(),
            _ => { sink.confirm_cancelled().map_err(|e| e.to_string())?; return Ok(()); }
        };
        let reviewed = match self
            .role(&execution, &sink, &profile.agents[3], &prior, &abort)
            .await?
        {
            RoleOutcome::Completed(output) => output,
            RoleOutcome::Cancelled => {
                sink.confirm_cancelled().map_err(|e| e.to_string())?;
                return Ok(());
            }
        };
        let artifact = verify_brief(&reviewed, &execution.process.input.materials)
            .map_err(str::to_owned)?
            .markdown();
        sink.complete(&artifact).map_err(|e| e.to_string())
    }
}

/// Missing configuration is unavailable, never an operator-key fallback.
pub async fn router(bridge: &Arc<JcodeBridge>, data_root: PathBuf) -> anyhow::Result<axum::Router> {
    let enabled = std::env::var("OHAGENT_AAAS_ENABLED").as_deref() == Ok("1");
    let oidc = match (
        std::env::var("OHAGENT_AAAS_OIDC_ISSUER"),
        std::env::var("OHAGENT_AAAS_OIDC_AUDIENCE"),
        std::env::var("OHAGENT_AAAS_OIDC_CLIENT_ID"),
    ) {
        (Ok(issuer), Ok(audience), Ok(client_id)) if enabled => Some(OidcConfig {
            issuer,
            audience,
            client_id,
            required_access_scope: std::env::var("OHAGENT_AAAS_OIDC_REQUIRED_ACCESS_SCOPE").ok(),
        }),
        _ => None,
    };
    let runtime: Option<Arc<dyn Runtime>> = if enabled {
        let restricted = JcodeBridge::new(Arc::clone(bridge.provider()))
            .with_config(JcodeBridgeConfig {
                runtime_root: Some(data_root.join("jcode")),
                jcode_binary: None,
            })
            .with_text_only_policy();
        Some(Arc::new(JcodeAnalyticalRuntime::new(
            Arc::new(restricted),
            data_root.join("aaas-workspaces"),
        )?))
    } else {
        None
    };
    let database_path = if enabled {
        std::env::var_os("OHAGENT_AAAS_DATABASE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| data_root.join("aaas.sqlite3"))
    } else {
        PathBuf::from(":memory:")
    };
    if enabled {
        anyhow::ensure!(data_root.is_absolute(), "AaaS data root must be absolute");
        tokio::fs::create_dir_all(&data_root).await?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(&data_root, std::fs::Permissions::from_mode(0o700)).await?;
        }
    }
    let state = ohagent_aaas::State::open(
        Config {
            database_path,
            oidc,
        },
        runtime,
    )
    .await?;
    Ok(ohagent_aaas::router(state))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn cancel_ack_is_not_turn_termination() {
        let (done, pending) = tokio::sync::oneshot::channel::<Result<(), ()>>();
        let stopped = wait_for_turn_stop(async { pending.await.unwrap() });
        tokio::pin!(stopped);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut stopped)
                .await
                .is_err()
        );
        done.send(Ok(())).unwrap();
        assert!(stopped.await.is_ok());
        assert_eq!(
            wait_for_turn_stop(async { Err::<(), _>(()) }).await,
            Err("runtime_cancel_unconfirmed")
        );
    }
}
