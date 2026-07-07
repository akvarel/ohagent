use axum::{extract::State, Json};
use serde::Serialize;

use crate::api::ApiState;

/// GET /api/plugins/audit — recent PII/secret redaction events
#[derive(Debug, Serialize)]
pub struct AuditLogResponse {
    pub total: usize,
    pub entries: Vec<AuditEntry>,
}

#[derive(Debug, Serialize)]
pub struct AuditEntry {
    pub plugin: String,
    pub field: String,
    pub original_bytes: u32,
    pub replacement_bytes: u32,
    pub timestamp: i64,
}

pub async fn plugin_audit_handler(State(state): State<ApiState>) -> Json<AuditLogResponse> {
    let entries: Vec<AuditEntry> = state
        .plugin_manager
        .as_ref()
        .map(|pm| {
            pm.lock().unwrap()
                .audit_log()
                .into_iter()
                .map(|e| AuditEntry {
                    plugin: e.plugin,
                    field: e.field,
                    original_bytes: e.original_length,
                    replacement_bytes: e.replacement_length,
                    timestamp: e.timestamp,
                })
                .collect()
        })
        .unwrap_or_default();

    let total = entries.len();
    Json(AuditLogResponse { total, entries })
}

/// DELETE /api/plugins/audit — clear the audit log
pub async fn plugin_audit_clear_handler(State(state): State<ApiState>) -> Json<serde_json::Value> {
    if let Some(ref pm) = state.plugin_manager {
        pm.lock().unwrap().clear_audit_log();
        tracing::info!("Audit log cleared");
    }
    Json(serde_json::json!({"status": "ok", "cleared": true}))
}
