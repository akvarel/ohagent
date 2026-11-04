use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine};
use ohagent_documents::*;
use serde_json::{json, Value};
include!("support/meter_rows.rs");
include!("support/meter_comparison.rs");
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

fn registry() -> ModelRegistry {
    ModelRegistry {
        revision: 1,
        models: ["a", "b", "c"]
            .iter()
            .map(|id| ModelSlot {
                id: (*id).into(),
                label: format!("Operator {id}"),
                model: format!("operator-{id}-version"),
                enabled: true,
            })
            .collect(),
    }
}
fn request() -> ExtractRequest {
    ExtractRequest {
        file_name: "handwritten.png".into(),
        media_type: "image/png".into(),
        content_base64: STANDARD.encode(b"\x89PNG\r\n\x1a\nfixture"),
    }
}
fn extraction() -> Extraction {
    Extraction {
        document_type: DocumentType::Invoice,
        fields: DocumentFields {
            total_amount: Some("12.30".into()),
            currency: Some("EUR".into()),
            ..Default::default()
        },
        evidence: vec![],
        warnings: vec![],
        meter_readings: vec![],
        model: "untrusted-attribution".into(),
    }
}
#[derive(Clone)]
enum Outcome {
    Good(Box<Extraction>),
    Failed,
    Invalid,
    Pending,
    Panic,
}
fn good(value: Extraction) -> Outcome {
    Outcome::Good(Box::new(value))
}

struct Fixture {
    outcome: Outcome,
    calls: Arc<AtomicUsize>,
    barrier: Option<Arc<tokio::sync::Barrier>>,
}
#[async_trait]
impl VisionProvider for Fixture {
    async fn extract(&self, _: ExtractRequest) -> Result<Extraction, DocumentError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(barrier) = &self.barrier {
            barrier.wait().await;
        }
        match &self.outcome {
            Outcome::Good(value) => Ok((**value).clone()),
            Outcome::Failed => Err(DocumentError::Transport),
            Outcome::Invalid => {
                let mut value = extraction();
                value.fields.total_amount = Some("not-a-decimal".into());
                Ok(value)
            }
            Outcome::Pending => std::future::pending().await,
            Outcome::Panic => panic!("synthetic provider panic"),
        }
    }
}
fn factories(
    outcomes: [Outcome; 3],
    calls: Arc<AtomicUsize>,
    barrier: Option<Arc<tokio::sync::Barrier>>,
) -> HashMap<String, Arc<dyn ModelProviderFactory>> {
    ["a", "b", "c"]
        .into_iter()
        .zip(outcomes)
        .map(|(id, outcome)| {
            let provider: Arc<dyn VisionProvider> = Arc::new(Fixture {
                outcome,
                calls: calls.clone(),
                barrier: barrier.clone(),
            });
            let factory: Arc<dyn ModelProviderFactory> =
                Arc::new(move |_: &str| Ok(provider.clone()));
            (id.into(), factory)
        })
        .collect()
}
fn service(outcomes: [Outcome; 3]) -> (ComparisonService, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    (
        ComparisonService::in_memory(registry(), factories(outcomes, calls.clone(), None)).unwrap(),
        calls,
    )
}

#[test]
fn registry_requires_three_unique_operator_slots() {
    let slot = registry().models.remove(0);
    assert!(ModelRegistry {
        revision: 1,
        models: vec![slot.clone()]
    }
    .validate()
    .is_err());
    assert!(ModelRegistry {
        revision: 1,
        models: vec![slot.clone(), slot.clone(), slot]
    }
    .validate()
    .is_err());
    let mut invalid = registry();
    invalid.revision = 0;
    assert!(invalid.validate().is_err());
    invalid = registry();
    invalid.models[0].model = "\n".into();
    assert!(invalid.validate().is_err());
    invalid = registry();
    invalid.models[0].label = "x".repeat(129);
    assert!(invalid.validate().is_err());
    invalid = registry();
    invalid.models[0].id = "../other".into();
    assert!(invalid.validate().is_err());
    let unknown = serde_json::json!({"revision":1,"models":[],"api_key":"not-permitted"});
    assert!(serde_json::from_value::<ModelRegistry>(unknown).is_err());
}

#[tokio::test]
async fn three_calls_run_concurrently_and_preserve_server_attribution() {
    let calls = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(tokio::sync::Barrier::new(3));
    let svc = ComparisonService::in_memory(
        registry(),
        factories(
            [good(extraction()), good(extraction()), good(extraction())],
            calls.clone(),
            Some(barrier),
        ),
    )
    .unwrap();
    let result = svc
        .compare_with_timeout(svc.snapshot().unwrap(), request(), Duration::from_secs(1))
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert!(result.comparison.complete);
    assert!(result.comparison.disagreements.is_empty());
    assert_eq!(result.extraction.unwrap().model, "operator-a-version");
    for (index, result) in result.comparison.results.iter().enumerate() {
        assert_eq!(result.status, ModelStatus::Succeeded);
        assert_eq!(
            result.extraction.as_ref().unwrap().model,
            registry().models[index].model
        );
        assert_eq!(result.error_code, None);
    }
}

#[tokio::test]
async fn exact_type_and_null_string_field_differences_are_not_majority_authority() {
    let mut second = extraction();
    second.document_type = DocumentType::Receipt;
    second.fields.total_amount = Some("12.3".into());
    second.fields.invoice_number = Some("B".into());
    let (svc, _) = service([good(extraction()), good(second.clone()), good(second)]);
    let result = svc
        .compare(svc.snapshot().unwrap(), request())
        .await
        .unwrap();
    assert_eq!(
        result.comparison.disagreements,
        [
            "document_type",
            "fields.invoice_number",
            "fields.total_amount"
        ]
    );
    assert!(result.comparison.complete);
    // The first success stays the editable candidate even when two models disagree with it.
    assert_eq!(
        result.extraction.unwrap().fields.total_amount.as_deref(),
        Some("12.30")
    );
}

#[tokio::test]
async fn failures_and_disabled_slots_are_visible_without_false_unanimity() {
    let (svc, calls) = service([Outcome::Failed, good(extraction()), good(extraction())]);
    let mut policy = svc.snapshot().unwrap();
    policy.models[2].enabled = false;
    let updated = svc.update(policy).unwrap();
    let result = svc.compare(updated, request()).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(!result.comparison.complete);
    assert!(result.comparison.disagreements.is_empty());
    assert_eq!(result.extraction.unwrap().model, "operator-b-version");
    assert_eq!(result.comparison.results[0].status, ModelStatus::Failed);
    assert_eq!(
        result.comparison.results[0].error_code.as_deref(),
        Some("provider_failed")
    );
    assert!(result.comparison.results[0].extraction.is_none());
    assert_eq!(result.comparison.results[2].status, ModelStatus::Disabled);
    assert_eq!(result.comparison.results[2].error_code, None);
}

#[tokio::test]
async fn all_failed_invalid_and_panicking_providers_never_produce_a_candidate() {
    let (svc, _) = service([Outcome::Failed, Outcome::Invalid, Outcome::Panic]);
    let result = svc
        .compare(svc.snapshot().unwrap(), request())
        .await
        .unwrap();
    assert!(result.extraction.is_none());
    assert!(!result.comparison.complete);
    assert!(result
        .comparison
        .results
        .iter()
        .all(|r| r.status == ModelStatus::Failed
            && r.extraction.is_none()
            && r.error_code.is_some()));
    assert_eq!(
        result.comparison.results[1].error_code.as_deref(),
        Some("invalid_extraction")
    );
}

#[tokio::test]
async fn timeout_retains_successes_and_does_not_wait_for_summed_deadlines() {
    let (svc, _) = service([good(extraction()), Outcome::Pending, Outcome::Pending]);
    let start = std::time::Instant::now();
    let result = svc
        .compare_with_timeout(
            svc.snapshot().unwrap(),
            request(),
            Duration::from_millis(40),
        )
        .await
        .unwrap();
    assert!(start.elapsed() < Duration::from_millis(300));
    assert!(result.extraction.is_some());
    assert_eq!(
        result.comparison.results[1].error_code.as_deref(),
        Some("provider_timeout")
    );
    assert_eq!(
        result.comparison.results[2].error_code.as_deref(),
        Some("provider_timeout")
    );
}

#[tokio::test]
async fn disabling_all_is_permitted_but_fails_closed_without_provider_calls() {
    let (svc, calls) = service([good(extraction()), good(extraction()), good(extraction())]);
    let mut policy = svc.snapshot().unwrap();
    for model in &mut policy.models {
        model.enabled = false;
    }
    let policy = svc.update(policy).unwrap();
    assert_eq!(policy.revision, 2);
    assert_eq!(
        svc.compare(policy, request()).await.unwrap_err(),
        RegistryError::AllDisabled
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn snapshot_and_optimistic_update_isolate_inflight_policy_and_versions() {
    let (svc, _) = service([good(extraction()), good(extraction()), good(extraction())]);
    let original = svc.snapshot().unwrap();
    let mut updated = original.clone();
    updated.models[0].model = "operator-a-next-version".into();
    updated.models[1].enabled = false;
    let updated = svc.update(updated).unwrap();
    assert_eq!(
        svc.update(original.clone()).unwrap_err(),
        RegistryError::Conflict
    );
    let inflight = svc.compare(original, request()).await.unwrap();
    let next = svc
        .compare(svc.snapshot().unwrap(), request())
        .await
        .unwrap();
    assert_eq!(inflight.comparison.policy_revision, 1);
    assert_eq!(inflight.extraction.unwrap().model, "operator-a-version");
    assert!(inflight.comparison.complete);
    assert_eq!(next.comparison.policy_revision, updated.revision);
    assert_eq!(next.extraction.unwrap().model, "operator-a-next-version");
    assert!(!next.comparison.complete);
}

#[test]
fn update_cannot_add_remove_or_substitute_a_connection_profile() {
    let (svc, _) = service([Outcome::Failed, Outcome::Failed, Outcome::Failed]);
    let mut policy = svc.snapshot().unwrap();
    policy.models[0].id = "attacker-endpoint".into();
    assert_eq!(svc.update(policy).unwrap_err(), RegistryError::Invalid);
    let mut policy = svc.snapshot().unwrap();
    policy.models.pop();
    assert_eq!(svc.update(policy).unwrap_err(), RegistryError::Invalid);
    assert_eq!(svc.snapshot().unwrap(), registry());
}

#[tokio::test]
async fn comparison_result_size_bound_applies_after_json_escaping() {
    let mut huge = extraction();
    huge.evidence = (0..64)
        .map(|_| FieldEvidence {
            field: "total_amount".into(),
            page: 1,
            quote: "\\".repeat(2048),
        })
        .collect();
    assert!(
        validate_extraction(&huge).is_ok(),
        "individually bounded texts may still expand in JSON"
    );
    let (svc, _) = service([good(huge), good(extraction()), good(extraction())]);
    let result = svc
        .compare(svc.snapshot().unwrap(), request())
        .await
        .unwrap();
    assert_eq!(
        result.comparison.results[0].error_code.as_deref(),
        Some("response_too_large")
    );
    assert_eq!(result.extraction.unwrap().model, "operator-b-version");
    let mut large = extraction();
    large.evidence = (0..64)
        .map(|_| FieldEvidence {
            field: "total_amount".into(),
            page: 1,
            quote: "\\x".repeat(1024),
        })
        .collect();
    let (svc, _) = service([good(large.clone()), good(large.clone()), good(large)]);
    let result = svc
        .compare(svc.snapshot().unwrap(), request())
        .await
        .unwrap();
    assert!(result.comparison.complete);
    assert!(serde_json::to_vec(&result).unwrap().len() <= MAX_COMPARISON_BYTES);
}

#[tokio::test]
async fn active_fanout_keeps_snapshot_when_registry_changes_before_completion() {
    struct Gated {
        started: Arc<tokio::sync::Semaphore>,
        release: Arc<tokio::sync::Semaphore>,
    }
    #[async_trait]
    impl VisionProvider for Gated {
        async fn extract(&self, _: ExtractRequest) -> Result<Extraction, DocumentError> {
            self.started.add_permits(1);
            self.release.acquire().await.unwrap().forget();
            Ok(extraction())
        }
    }
    let started = Arc::new(tokio::sync::Semaphore::new(0));
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    let provider: Arc<dyn VisionProvider> = Arc::new(Gated {
        started: started.clone(),
        release: release.clone(),
    });
    let factories = ["a", "b", "c"]
        .into_iter()
        .map(|id| {
            let provider = provider.clone();
            let factory: Arc<dyn ModelProviderFactory> =
                Arc::new(move |_: &str| Ok(provider.clone()));
            (id.into(), factory)
        })
        .collect();
    let svc = Arc::new(ComparisonService::in_memory(registry(), factories).unwrap());
    let running = svc.clone();
    let task = tokio::spawn(async move {
        running
            .compare(running.snapshot().unwrap(), request())
            .await
            .unwrap()
    });
    tokio::time::timeout(Duration::from_secs(1), started.acquire_many(3))
        .await
        .unwrap()
        .unwrap()
        .forget();
    let mut changed = svc.snapshot().unwrap();
    changed.models[0].model = "next-version-during-fanout".into();
    changed.models[1].enabled = false;
    svc.update(changed).unwrap();
    release.add_permits(3);
    let result = task.await.unwrap();
    assert_eq!(result.comparison.policy_revision, 1);
    assert!(result.comparison.complete);
    assert_eq!(result.extraction.unwrap().model, "operator-a-version");
    assert_eq!(svc.snapshot().unwrap().revision, 2);
}

#[test]
fn atomic_file_persists_reloads_and_never_contains_connection_secrets() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("models.json");
    let calls = Arc::new(AtomicUsize::new(0));
    let make = || {
        factories(
            [Outcome::Failed, Outcome::Failed, Outcome::Failed],
            calls.clone(),
            None,
        )
    };
    let first = ComparisonService::with_file(registry(), make(), path.clone()).unwrap();
    let second = ComparisonService::with_file(registry(), make(), path.clone()).unwrap();
    let mut proposed = first.snapshot().unwrap();
    proposed.models[0].model = "operator-next".into();
    proposed.models[2].enabled = false;
    let changed = first.update(proposed).unwrap();
    assert_eq!(second.snapshot().unwrap(), changed);
    assert_eq!(
        second.update(registry()).unwrap_err(),
        RegistryError::Conflict
    );
    drop(first);
    drop(second);
    let restarted = ComparisonService::with_file(registry(), make(), path.clone()).unwrap();
    assert_eq!(restarted.snapshot().unwrap(), changed);
    let stored = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        serde_json::from_str::<ModelRegistry>(&stored).unwrap(),
        changed
    );
    for forbidden in [
        "base_url",
        "api_key",
        "api_key_env",
        "content_base64",
        "tenant_id",
    ] {
        assert!(!stored.contains(forbidden));
    }
    assert_eq!(
        std::fs::read_dir(directory.path()).unwrap().count(),
        2,
        "only registry and stable lock remain"
    );
}

#[test]
fn malformed_oversized_or_locked_file_fails_closed_without_replacing_it() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("models.json");
    let calls = Arc::new(AtomicUsize::new(0));
    let svc = ComparisonService::with_file(
        registry(),
        factories(
            [Outcome::Failed, Outcome::Failed, Outcome::Failed],
            calls,
            None,
        ),
        path.clone(),
    )
    .unwrap();
    std::fs::write(&path, "not-json").unwrap();
    assert_eq!(svc.snapshot().unwrap_err(), RegistryError::Invalid);
    assert_eq!(
        svc.update(registry()).unwrap_err(),
        RegistryError::Unavailable
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "not-json");
    std::fs::write(&path, vec![b' '; 16 * 1024 + 1]).unwrap();
    assert_eq!(svc.snapshot().unwrap_err(), RegistryError::Invalid);
    std::fs::write(&path, serde_json::to_vec(&registry()).unwrap()).unwrap();
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(directory.path().join("models.json.lock"))
        .unwrap();
    lock.lock().unwrap();
    assert_eq!(svc.snapshot().unwrap_err(), RegistryError::Unavailable);
    drop(lock);
    assert_eq!(svc.snapshot().unwrap(), registry());
}
