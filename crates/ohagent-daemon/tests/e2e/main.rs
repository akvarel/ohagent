//! E2E tests for ohAgent daemon — runs Gherkin .feature files against a live daemon.
//!
//! Starts the daemon as a subprocess, runs scenarios, stops it.
//! Feature files are in `tests/e2e/features/`.

mod common;

use cucumber::{given, then, when, World};
use std::sync::Mutex;

static DAEMON: Mutex<Option<std::process::Child>> = Mutex::new(None);

/// Shared test world — holds the HTTP response for assertions.
#[derive(Debug, Default, World)]
struct OhAgentWorld {
    /// Last HTTP response status
    status: u16,
    /// Last HTTP response body as text
    body: String,
    /// Last HTTP response content-type header
    content_type: String,
}

#[given("the daemon is running")]
async fn daemon_running(_w: &mut OhAgentWorld) {
    // Start daemon once, shared across all scenarios
    let mut guard = DAEMON.lock().unwrap();
    if guard.is_none() {
        *guard = Some(common::start_daemon());
    }
    drop(guard);
    let healthy = common::wait_for_healthy().await;
    assert!(healthy, "Daemon did not become healthy");
}

#[given("the daemon is running on port 9090")]
async fn daemon_running_port(_w: &mut OhAgentWorld) {
    let mut guard = DAEMON.lock().unwrap();
    if guard.is_none() {
        *guard = Some(common::start_daemon());
    }
    drop(guard);
    let healthy = common::wait_for_healthy().await;
    assert!(healthy, "Daemon not healthy on port");
}

#[given("the daemon is running without Vault configured")]
async fn daemon_no_vault(_w: &mut OhAgentWorld) {
    let mut guard = DAEMON.lock().unwrap();
    if guard.is_none() {
        *guard = Some(common::start_daemon());
    }
    drop(guard);
    let healthy = common::wait_for_healthy().await;
    assert!(healthy, "Daemon did not become healthy");
}

#[when(expr = "I GET {word}")]
async fn get_endpoint(w: &mut OhAgentWorld, path: String) {
    let client = common::client();
    let url = format!("{}{path}", common::base_url());
    match client.get(&url).send().await {
        Ok(resp) => {
            w.status = resp.status().as_u16();
            w.content_type = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            w.body = resp.text().await.unwrap_or_default();
        }
        Err(e) => {
            w.status = 0;
            w.body = format!("HTTP error: {e}");
        }
    }
}

#[when(regex = r"^I POST (/[\w/]+) with:$")]
async fn post_endpoint(w: &mut OhAgentWorld, path: String, body: String) {
    let client = common::client();
    let url = format!("{}{path}", common::base_url());
    let body = body.trim().to_string();
    match client
        .post(&url)
        .header("Content-Type", "application/json")
        .body(body.clone())
        .send()
        .await
    {
        Ok(resp) => {
            w.status = resp.status().as_u16();
            w.content_type = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            w.body = resp.text().await.unwrap_or_default();
        }
        Err(e) => {
            w.status = 0;
            w.body = format!("HTTP error: {e}");
        }
    }
}

#[when("I make a TCP connection to port 9090")]
async fn tcp_connect(w: &mut OhAgentWorld) {
    match tokio::net::TcpStream::connect(format!("127.0.0.1:{}", common::TEST_PORT)).await {
        Ok(_) => {
            w.status = 200;
            w.body = "connected".into();
        }
        Err(e) => {
            w.status = 0;
            w.body = format!("Connection failed: {e}");
        }
    }
}

#[then(expr = "the response status is {int}")]
fn check_status(w: &mut OhAgentWorld, expected: u16) {
    assert_eq!(
        w.status, expected,
        "Expected status {expected}, got {}. Body: {}",
        w.status, w.body
    );
}

#[then(expr = "the response status is {int} or {int}")]
fn check_status_or(w: &mut OhAgentWorld, a: u16, b: u16) {
    assert!(
        w.status == a || w.status == b,
        "Expected status {a} or {b}, got {}. Body: {}",
        w.status,
        w.body
    );
}

#[then("the connection succeeds")]
fn connection_succeeds(w: &mut OhAgentWorld) {
    assert_eq!(w.status, 200, "Connection failed: {}", w.body);
}

#[then(regex = r#"^the response body contains "([^"]+)"$"#)]
fn body_contains(w: &mut OhAgentWorld, expected: String) {
    assert!(
        w.body.contains(&expected),
        "Body does not contain '{expected}'. Body: {}",
        w.body
    );
}

#[then(regex = "^the response body contains '([^']+)' array$")]
fn body_contains_array(w: &mut OhAgentWorld, _key: String) {
    let val: serde_json::Value = serde_json::from_str(&w.body).expect("Body is not valid JSON");
    // Check that the body is a JSON array (top-level or within an object)
    if let Some(data) = val.get("data") {
        assert!(data.is_array(), "data is not an array: {data}");
    } else {
        assert!(val.is_array(), "Body is not a JSON array: {val}");
    }
}

#[then("the response body is a JSON array")]
fn body_is_array(w: &mut OhAgentWorld) {
    let val: serde_json::Value = serde_json::from_str(&w.body).expect("Body is not valid JSON");
    assert!(val.is_array(), "Body is not a JSON array: {val}");
}

#[then(regex = r#"^the response body is a JSON object with "([^"]+)": "([^"]*)"$"#)]
fn body_object_with_key_value(w: &mut OhAgentWorld, key: String, expected_val: String) {
    let val: serde_json::Value = serde_json::from_str(&w.body).expect("Body is not valid JSON");
    let actual = val.get(&key).and_then(|v| v.as_str()).unwrap_or("");
    assert_eq!(
        actual, expected_val,
        "Expected {key}: {expected_val}, got: {actual}. Body: {}",
        w.body
    );
}

#[then(expr = "the response content type is {string}")]
fn check_content_type(w: &mut OhAgentWorld, expected: String) {
    assert!(
        w.content_type.starts_with(&expected),
        "Expected content-type '{expected}', got '{}'. Body: {}",
        w.content_type,
        w.body
    );
}

#[then("the response content is not empty")]
fn content_not_empty(w: &mut OhAgentWorld) {
    let val: serde_json::Value = serde_json::from_str(&w.body).expect("Body is not valid JSON");
    let choices = val
        .get("choices")
        .and_then(|c| c.as_array())
        .expect("choices is not an array");
    assert!(!choices.is_empty(), "choices array is empty");
    let content = choices[0]
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("");
    assert!(!content.is_empty(), "Response content is empty");
}

/// Run all feature files.
#[tokio::main]
async fn main() {
    // Run scenarios (cucumber handles exit codes via panics on failure)
    OhAgentWorld::cucumber()
        .max_concurrent_scenarios(1) // Only one daemon at a time
        .fail_on_skipped()
        .run("tests/e2e/features")
        .await;

    // Cleanup: stop daemon and any leftover processes
    if let Some(mut child) = DAEMON.lock().unwrap().take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let _ = std::process::Command::new("pkill")
        .args(["-f", "ohagent-daemon.*19090"])
        .output();
}
