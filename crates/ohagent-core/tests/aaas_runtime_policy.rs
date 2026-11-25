use ohagent_core::jcode_bridge::JcodeBridge;
use std::sync::Arc;

#[test]
fn customer_analysis_bridge_has_an_explicit_deny_all_tool_policy() {
    let bridge = JcodeBridge::new(Arc::new(jcode_base::provider::MultiProvider::new()))
        .with_text_only_policy();
    assert!(bridge.is_text_only());
}

#[test]
fn existing_gateway_bridge_preserves_its_tool_policy() {
    let bridge = JcodeBridge::new(Arc::new(jcode_base::provider::MultiProvider::new()));
    assert!(!bridge.is_text_only());
}
