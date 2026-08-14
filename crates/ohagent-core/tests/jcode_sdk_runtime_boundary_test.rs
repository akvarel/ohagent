use jcode_provider_core::Provider;
use ohagent_core::jcode_bridge::{JcodeBridge, JcodeBridgeConfig, SessionConfig};
use std::path::PathBuf;
use std::sync::Arc;

fn bridge() -> JcodeBridge {
    let provider: Arc<dyn Provider> = Arc::new(jcode_base::provider::MultiProvider::new());
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
fn same_tenant_reuses_runtime_path_and_different_tenants_are_isolated() {
    let bridge = bridge();
    let a1 = bridge.runtime_home_for_tenant("tenant-a").unwrap();
    let a2 = bridge.runtime_home_for_tenant("tenant-a").unwrap();
    let b = bridge.runtime_home_for_tenant("tenant-b").unwrap();

    assert_eq!(a1, a2);
    assert_ne!(a1, b);
}

#[test]
fn session_scope_keys_include_tenant_boundary() {
    let bridge = bridge();
    let a = bridge.session_scope_key("tenant-a", "session-1").unwrap();
    let b = bridge.session_scope_key("tenant-b", "session-1").unwrap();

    assert_ne!(a, b);
    assert!(bridge.session_scope_key("", "session-1").is_err());
    assert!(bridge.session_scope_key("tenant-a", "").is_err());
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

    let error = bridge
        .validate_session_config(&unsupported)
        .unwrap_err()
        .to_string();
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

    let error = bridge
        .validate_session_config(&unsafe_config)
        .unwrap_err()
        .to_string();
    assert!(error.contains("working_dir"));
}
