use crate::{
    auth::Principal,
    model::{CancelRequest, ProcessInput, StartRequest},
    BackendError, EventSink, State,
};
use axum::{
    extract::{rejection::JsonRejection, DefaultBodyLimit, Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    response::{
        sse::{Event as SseEvent, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{convert::Infallible, time::Duration};

pub fn router(state: State) -> Router {
    Router::new()
        .route("/api/aaas/config", get(config))
        .route("/api/aaas/processes", get(processes).post(create_process))
        .route("/api/aaas/processes/{id}", put(update_process))
        .route("/api/aaas/runs", get(runs).post(start))
        .route("/api/aaas/runs/{id}", get(run))
        .route("/api/aaas/runs/{id}/cancel", post(cancel))
        .route("/api/aaas/runs/{id}/events", get(events))
        .route("/api/aaas/runs/{id}/artifact", get(artifact))
        .layer(DefaultBodyLimit::max(120_000))
        .layer(axum::middleware::map_response(
            |mut response: Response| async move {
                response.headers_mut().insert(
                    "cache-control",
                    axum::http::HeaderValue::from_static("no-store"),
                );
                response
            },
        ))
        .with_state(state)
}
async fn config(AxumState(s): AxumState<State>) -> Json<Value> {
    Json(
        json!({"auth":s.public_auth,"pilot_available":s.authorizer.is_some() && s.runtime.is_some()}),
    )
}
fn bearer(headers: &HeaderMap) -> Result<String, BackendError> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .filter(|v| !v.is_empty() && v.len() <= 16_384)
        .ok_or(BackendError::Unauthorized)?;
    Ok(token.into())
}
async fn principal(s: &State, h: &HeaderMap) -> Result<Principal, BackendError> {
    s.authorizer
        .as_ref()
        .ok_or(BackendError::Unavailable)?
        .authorize(&bearer(h)?)
        .await
}
async fn processes(
    AxumState(s): AxumState<State>,
    h: HeaderMap,
) -> Result<Json<Value>, BackendError> {
    let p = principal(&s, &h).await?;
    Ok(Json(
        json!({"processes":s.store.processes(&p.tenant)?,"template":{"id":"analytical-brief","roles":["coordinator","analyst","evidence_checker","reviewer"],"timeout_seconds":300}}),
    ))
}
async fn create_process(
    AxumState(s): AxumState<State>,
    h: HeaderMap,
    body: Result<Json<ProcessInput>, JsonRejection>,
) -> Result<Response, BackendError> {
    let p = principal(&s, &h).await?;
    let Json(input) = body.map_err(|_| BackendError::Invalid)?;
    Ok((
        StatusCode::CREATED,
        Json(s.store.process(&p.tenant, input, None)?),
    )
        .into_response())
}
async fn update_process(
    AxumState(s): AxumState<State>,
    Path(id): Path<String>,
    h: HeaderMap,
    body: Result<Json<ProcessInput>, JsonRejection>,
) -> Result<Json<crate::Process>, BackendError> {
    let p = principal(&s, &h).await?;
    let Json(input) = body.map_err(|_| BackendError::Invalid)?;
    Ok(Json(s.store.process(&p.tenant, input, Some(&id))?))
}
async fn runs(AxumState(s): AxumState<State>, h: HeaderMap) -> Result<Json<Value>, BackendError> {
    let p = principal(&s, &h).await?;
    Ok(Json(json!({"runs":s.store.runs(&p.tenant)?})))
}
async fn run(
    AxumState(s): AxumState<State>,
    Path(id): Path<String>,
    h: HeaderMap,
) -> Result<Json<crate::Run>, BackendError> {
    let p = principal(&s, &h).await?;
    Ok(Json(s.store.run(&p.tenant, &id)?))
}
async fn start(
    AxumState(s): AxumState<State>,
    h: HeaderMap,
    body: Result<Json<StartRequest>, JsonRejection>,
) -> Result<Response, BackendError> {
    let p = principal(&s, &h).await?;
    let runtime = s.runtime.clone().ok_or(BackendError::Unavailable)?;
    let Json(req) = body.map_err(|_| BackendError::Invalid)?;
    let key = h
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .ok_or(BackendError::Invalid)?;
    let permit = s.runtime_slots.clone().try_acquire_owned();
    let (run, execution) = if permit.is_ok() {
        s.store.start(&p.tenant, key, &req)?
    } else {
        (
            s.store
                .repeat(&p.tenant, key, &req)?
                .ok_or(BackendError::Unavailable)?,
            None,
        )
    };
    let status = if let Some(execution) = execution {
        let permit = permit.map_err(|_| BackendError::Unavailable)?;
        let sink = EventSink {
            store: s.store.clone(),
            tenant: p.tenant,
            run: run.id.clone(),
            epoch: execution.producer_epoch.clone(),
        };
        tokio::spawn(async move {
            let _permit = permit;
            let worker_sink = sink.clone();
            let mut task =
                tokio::spawn(async move { runtime.execute(execution, worker_sink).await });
            let result = tokio::time::timeout(Duration::from_secs(300), &mut task).await;
            match result {
                Ok(_) => {
                    if let Ok(run) = sink.store.run(&sink.tenant, &sink.run) {
                        if run.status == "cancellation_requested" {
                            // Producer exited without an explicit engine cancellation confirmation.
                            let _ = sink.interrupt();
                        } else if !run.terminal() {
                            let _ = sink.fail("runtime_incomplete");
                        }
                    }
                }
                Err(_) => {
                    task.abort();
                    let _ = sink.interrupt();
                }
            }
        });
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(run)).into_response())
}
async fn cancel(
    AxumState(s): AxumState<State>,
    Path(id): Path<String>,
    h: HeaderMap,
    body: Result<Json<CancelRequest>, JsonRejection>,
) -> Result<Json<crate::Run>, BackendError> {
    let p = principal(&s, &h).await?;
    let Json(req) = body.map_err(|_| BackendError::Invalid)?;
    let run = s
        .store
        .update(&p.tenant, &id, None, "run.cancellation_requested", |run| {
            if run.version != req.version {
                return Err(BackendError::Conflict);
            }
            if run.terminal() || run.status == "cancellation_requested" {
                return Err(BackendError::Conflict);
            }
            run.status = "cancellation_requested".into();
            Ok(None)
        })?;
    Ok(Json(run))
}
async fn artifact(
    AxumState(s): AxumState<State>,
    Path(id): Path<String>,
    h: HeaderMap,
) -> Result<Response, BackendError> {
    let p = principal(&s, &h).await?;
    Ok((
        [
            ("content-type", "text/markdown; charset=utf-8"),
            ("content-disposition", "attachment; filename=brief.md"),
            ("x-content-type-options", "nosniff"),
            ("cache-control", "no-store"),
        ],
        s.store.artifact(&p.tenant, &id)?,
    )
        .into_response())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    #[serde(default)]
    after: u64,
}
async fn events(
    AxumState(s): AxumState<State>,
    Path(id): Path<String>,
    h: HeaderMap,
    query: Result<Query<Cursor>, axum::extract::rejection::QueryRejection>,
) -> Result<Response, BackendError> {
    let p = principal(&s, &h).await?;
    let token = bearer(&h)?;
    let Query(cursor) = query.map_err(|_| BackendError::Cursor)?;
    // Authorize ownership and reject unavailable cursors before committing SSE headers.
    s.store.events(&p.tenant, &id, cursor.after)?;
    let permit = s
        .stream_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| BackendError::Unavailable)?;
    let stream = async_stream::stream! {
        let _permit = permit;
        let mut after=cursor.after;
        loop {
            let auth=s.authorizer.as_ref().expect("authorized above").authorize(&token).await;
            if !matches!(auth,Ok(ref principal) if principal.tenant==p.tenant) {
                yield Ok::<SseEvent,Infallible>(SseEvent::default().event("auth_required").data("{\"code\":\"unauthorized\"}"));
                break;
            }
            match s.store.events(&p.tenant,&id,after) {
                Ok((batch,done))=> {
                    let full=batch.len()==64;
                    for event in batch {
                        after=event.run_seq;
                        let data=serde_json::to_string(&event).unwrap_or_else(|_|"{}".into());
                        yield Ok(SseEvent::default().id(after.to_string()).event("journal").data(data));
                    }
                    if done {break;}
                    if full {tokio::task::yield_now().await;} else {tokio::time::sleep(Duration::from_millis(500)).await;}
                }
                Err(error)=> {
                    yield Ok(SseEvent::default().event(error.code()).data(json!({"code":error.code()}).to_string()));
                    break;
                }
            }
        }
    };
    Ok(Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(10))
                .text("heartbeat"),
        )
        .into_response())
}
