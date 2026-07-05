//! REST API for ohAgent dashboard and external integrations.
//!
//! Endpoints:
//! - GET  /health            — health check (existing)
//! - GET  /api/status        — full daemon status
//! - GET  /api/skills        — list skills
//! - GET  /api/skills/:id    — skill detail
//! - POST /api/skills/:id/record — record usage
//! - GET  /api/memory        — query memories
//! - GET  /api/memory/:id    — memory detail

use std::sync::Arc;
use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use ohagent_core::jcode_bridge::JcodeBridge;
use ohagent_memory::engine::MemoryEngine;
use ohagent_skills::evaluator;
use ohagent_skills::models::SkillStatus;
use ohagent_skills::registry::SkillRegistry;

/// Shared state for API handlers.
#[derive(Clone)]
pub struct ApiState {
    pub bridge: Arc<JcodeBridge>,
    pub memory: Option<Arc<MemoryEngine>>,
    pub skills: Option<Arc<SkillRegistry>>,
    pub start_time: chrono::DateTime<chrono::Utc>,
}

/// Build the full API router (includes /health).
pub fn router(state: ApiState) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route("/api/status", get(status_handler))
        .route("/api/skills", get(list_skills))
        .route("/api/skills/{id}", get(get_skill))
        .route("/api/skills/{id}/record", post(record_skill_usage))
        .route("/api/memory", get(query_memory))
        .route("/api/memory/{id}", get(get_memory))
        .with_state(state)
}

// ── Handlers ──

async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "service": "ohagent",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

#[derive(Serialize)]
struct StatusResponse {
    service: String,
    version: String,
    uptime_seconds: i64,
    provider: String,
    skills_count: usize,
    memory_count: usize,
    skills_enabled: bool,
    memory_enabled: bool,
}

async fn status_handler(State(state): State<ApiState>) -> Json<StatusResponse> {
    let uptime = (chrono::Utc::now() - state.start_time).num_seconds();

    let skills_count = state
        .skills
        .as_ref()
        .and_then(|s| s.all_tenants().ok())
        .map(|t| t.len())
        .unwrap_or(0);

    let memory_count = 0usize; // TODO: aggregate across tenants

    Json(StatusResponse {
        service: "ohagent".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        uptime_seconds: uptime,
        provider: state.bridge.provider_name(),
        skills_count,
        memory_count,
        skills_enabled: state.skills.is_some(),
        memory_enabled: state.memory.is_some(),
    })
}

// ── Skills ──

#[derive(Deserialize)]
struct SkillsQuery {
    tenant_id: Option<String>,
    status: Option<String>,
}

#[derive(Serialize)]
struct SkillSummary {
    id: String,
    name: String,
    status: String,
    version: String,
    quality_score: f32,
    use_count: u32,
    triggers: Vec<String>,
    tags: Vec<String>,
}

async fn list_skills(
    State(state): State<ApiState>,
    Query(params): Query<SkillsQuery>,
) -> Json<Vec<SkillSummary>> {
    let skills = match &state.skills {
        Some(s) => s,
        None => return Json(vec![]),
    };

    let tenant_id = params.tenant_id.unwrap_or_else(|| "default".into());
    let status_filter = params.status.and_then(|s| {
        match s.as_str() {
            "proposed" => Some(SkillStatus::Proposed),
            "active" => Some(SkillStatus::Active),
            "disabled" => Some(SkillStatus::Disabled),
            "retired" => Some(SkillStatus::Retired),
            _ => None,
        }
    });

    let list = match skills.list(&tenant_id, status_filter.as_ref(), 50) {
        Ok(l) => l,
        Err(_) => return Json(vec![]),
    };

    let summaries: Vec<SkillSummary> = list
        .into_iter()
        .map(|s| SkillSummary {
            id: s.id,
            name: s.name,
            status: s.status.to_string(),
            version: s.version,
            quality_score: s.quality_score,
            use_count: s.use_count,
            triggers: s.triggers,
            tags: s.tags,
        })
        .collect();

    Json(summaries)
}

#[derive(Serialize)]
struct SkillDetail {
    id: String,
    tenant_id: String,
    name: String,
    description: String,
    triggers: Vec<String>,
    instructions: String,
    version: String,
    status: String,
    origin: String,
    quality_score: f32,
    use_count: u32,
    success_count: u32,
    failure_count: u32,
    tags: Vec<String>,
    created_at: String,
    updated_at: String,
    last_used_at: Option<String>,
}

async fn get_skill(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Json<SkillDetail>, axum::http::StatusCode> {
    let skills = state.skills.as_ref().ok_or(axum::http::StatusCode::SERVICE_UNAVAILABLE)?;
    let skill = skills
        .get(&id)
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(axum::http::StatusCode::NOT_FOUND)?;

    Ok(Json(SkillDetail {
        id: skill.id,
        tenant_id: skill.tenant_id,
        name: skill.name,
        description: skill.description,
        triggers: skill.triggers,
        instructions: skill.instructions,
        version: skill.version,
        status: skill.status.to_string(),
        origin: skill.origin.to_string(),
        quality_score: skill.quality_score,
        use_count: skill.use_count,
        success_count: skill.success_count,
        failure_count: skill.failure_count,
        tags: skill.tags,
        created_at: skill.created_at.to_rfc3339(),
        updated_at: skill.updated_at.to_rfc3339(),
        last_used_at: skill.last_used_at.map(|t| t.to_rfc3339()),
    }))
}

#[derive(Deserialize)]
struct RecordUsageBody {
    success: Option<bool>,
    tenant_id: Option<String>,
}

async fn record_skill_usage(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Json(body): Json<RecordUsageBody>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    let skills = state.skills.as_ref().ok_or(axum::http::StatusCode::SERVICE_UNAVAILABLE)?;
    let tenant_id = body.tenant_id.unwrap_or_else(|| "default".into());
    let success = body.success.unwrap_or(true);
    let session_id = "api";

    if success {
        evaluator::record_success(skills, &id, session_id, &tenant_id, None)
            .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    } else {
        evaluator::record_failure(skills, &id, session_id, &tenant_id, None)
            .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    Ok(Json(serde_json::json!({"ok": true})))
}

// ── Memory ──

#[derive(Deserialize)]
struct MemoryQuery {
    tenant_id: Option<String>,
    #[serde(default)]
    q: String,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    20
}

#[derive(Serialize)]
struct MemorySummary {
    id: String,
    tenant_id: String,
    session_id: String,
    content: String,
    source: String,
    importance: f32,
    created_at: String,
    access_count: u32,
    tags: Vec<String>,
}

async fn query_memory(
    State(state): State<ApiState>,
    Query(params): Query<MemoryQuery>,
) -> Json<Vec<MemorySummary>> {
    let memory = match &state.memory {
        Some(m) => m,
        None => return Json(vec![]),
    };

    let tenant_id = params.tenant_id.unwrap_or_else(|| "default".into());

    let entries = if params.q.is_empty() {
        memory.list(&tenant_id, None, params.limit).unwrap_or_default()
    } else {
        memory.search(&tenant_id, &params.q).unwrap_or_default()
            .into_iter()
            .map(|r| r.entry)
            .collect()
    };

    let summaries: Vec<MemorySummary> = entries
        .into_iter()
        .map(|e| MemorySummary {
            id: e.id,
            tenant_id: e.tenant_id,
            session_id: e.session_id,
            content: e.content,
            source: e.source.to_string(),
            importance: e.importance,
            created_at: e.created_at.to_rfc3339(),
            access_count: e.access_count,
            tags: e.tags,
        })
        .collect();

    Json(summaries)
}

async fn get_memory(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Json<MemorySummary>, axum::http::StatusCode> {
    let memory = state.memory.as_ref().ok_or(axum::http::StatusCode::SERVICE_UNAVAILABLE)?;
    let entry = memory
        .get(&id)
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(axum::http::StatusCode::NOT_FOUND)?;

    Ok(Json(MemorySummary {
        id: entry.id,
        tenant_id: entry.tenant_id,
        session_id: entry.session_id,
        content: entry.content,
        source: entry.source.to_string(),
        importance: entry.importance,
        created_at: entry.created_at.to_rfc3339(),
        access_count: entry.access_count,
        tags: entry.tags,
    }))
}
