//! Prometheus metrics for ohAgent daemon.
//!
//! Exposes `/metrics` endpoint (OpenMetrics format).
//! Metrics are collected by Prometheus and visualized in Grafana.
//!
//! Key metrics:
//! - ohagent_requests_total — API request count by path/method/status
//! - ohagent_llm_calls_total — LLM call count by provider/model
//! - ohagent_llm_tokens_total — total tokens consumed
//! - ohagent_sessions_active — active session count
//! - ohagent_webhook_requests_total — webhook requests by platform

use axum::{
    extract::Request,
    middleware::Next,
    response::Response,
    Extension,
};
use prometheus::{
    register_counter_vec, register_gauge, Counter, CounterVec, Encoder, Gauge, TextEncoder,
};
use std::sync::Arc;
use std::time::Instant;

/// All collected metrics.
#[derive(Clone)]
pub struct Metrics {
    pub requests: CounterVec,
    pub llm_calls: CounterVec,
    pub llm_tokens: CounterVec,
    pub sessions_active: Gauge,
    pub webhook_requests: CounterVec,
    pub request_duration_seconds: prometheus::HistogramVec,
}

impl Metrics {
    /// Initialize and register all metrics.
    pub fn new() -> Result<Self, prometheus::Error> {
        let requests = register_counter_vec!(
            "ohagent_requests_total",
            "Total HTTP requests",
            &["path", "method", "status"]
        )?;

        let llm_calls = register_counter_vec!(
            "ohagent_llm_calls_total",
            "Total LLM API calls",
            &["provider", "model"]
        )?;

        let llm_tokens = register_counter_vec!(
            "ohagent_llm_tokens_total",
            "Total tokens consumed",
            &["provider", "type"] // type: prompt or completion
        )?;

        let sessions_active = register_gauge!(
            "ohagent_sessions_active",
            "Currently active Jcode sessions"
        )?;

        let webhook_requests = register_counter_vec!(
            "ohagent_webhook_requests_total",
            "Total webhook requests",
            &["platform", "status"]
        )?;

        let request_duration_seconds = prometheus::HistogramVec::new(
            prometheus::HistogramOpts::new(
                "ohagent_request_duration_seconds",
                "HTTP request duration in seconds",
            ),
            &["path"],
        )?;
        prometheus::register(Box::new(request_duration_seconds.clone()))?;

        Ok(Self {
            requests,
            llm_calls,
            llm_tokens,
            sessions_active,
            webhook_requests,
            request_duration_seconds,
        })
    }
}

/// Shared state for metrics middleware.
#[derive(Clone)]
pub struct MetricsState {
    pub metrics: Arc<Metrics>,
}

/// Axum middleware: record request count and duration.
/// State is passed via Extension to avoid type conflicts with handler state.
pub async fn metrics_middleware(
    Extension(state): Extension<MetricsState>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path().to_string();
    let method = request.method().to_string();
    let start = Instant::now();

    let response = next.run(request).await;

    let status = response.status().as_u16().to_string();
    let duration = start.elapsed().as_secs_f64();

    state
        .metrics
        .requests
        .with_label_values(&[&path, &method, &status])
        .inc();
    state
        .metrics
        .request_duration_seconds
        .with_label_values(&[&path])
        .observe(duration);

    response
}

/// GET /metrics — Prometheus scrape endpoint.
pub async fn metrics_handler(
    Extension(state): Extension<MetricsState>,
) -> Result<String, axum::http::StatusCode> {
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    let encoder = TextEncoder::new();
    encoder
        .encode(&metric_families, &mut buffer)
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    String::from_utf8(buffer).map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)
}
