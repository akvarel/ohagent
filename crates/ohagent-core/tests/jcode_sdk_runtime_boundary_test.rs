use ohagent_core::jcode_bridge::{JcodeBridge, JcodeBridgeConfig, SessionConfig};
use std::path::PathBuf;
use std::sync::Arc;

fn bridge() -> JcodeBridge {
    let provider = ohagent_core::providers::create_mock_provider();
    JcodeBridge::new(provider).with_config(JcodeBridgeConfig {
        runtime_root: Some(PathBuf::from("/var/lib/ohagent/jcode-runtimes")),
        jcode_binary: Some(PathBuf::from("/usr/local/bin/jcode-test")),
    })
}

#[test]
fn runtime_path_is_hashed_and_does_not_expose_raw_tenant_id() {
    let bridge = bridge();
    let path = bridge
        .runtime_home_for_tenant("tenant/../../secret")
        .expect("tenant runtime path");
    let rendered = path.to_string_lossy();

    assert!(rendered.starts_with("/var/lib/ohagent/jcode-runtimes/"));
    assert!(!rendered.contains("tenant"));
    assert!(!rendered.contains("secret"));
    assert!(!rendered.contains(".."));
}

#[test]
fn session_config_requires_explicit_tenant_id_and_rejects_private_internals() {
    let bridge = bridge();
    let unsupported = SessionConfig {
        tenant_id: "tenant-a".to_string(),
        model: None,
        working_dir: Some("/srv/tenant-a/workspace".to_string()),
        selfdev: true,
        report_back_to: Some("parent-session".to_string()),
    };

    let error = bridge.validate_session_config(&unsupported).unwrap_err().to_string();
    assert!(error.contains("selfdev"));
    assert!(error.contains("report_back_to"));
}

#[test]
fn workspace_must_be_safe_absolute_path_without_traversal() {
    let bridge = bridge();
    let unsafe_config = SessionConfig {
        tenant_id: "tenant-a".to_string(),
        model: None,
        working_dir: Some("../escape".to_string()),
        selfdev: false,
        report_back_to: None,
    };

    let error = bridge.validate_session_config(&unsafe_config).unwrap_err().to_string();
    assert!(error.contains("working_dir"));
}
