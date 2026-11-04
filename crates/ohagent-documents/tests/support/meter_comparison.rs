#[tokio::test]
async fn row_candidates_keep_all_observations_and_first_success_not_majority() {
    let first = meter_sheet();
    let mut second = first.clone();
    second.meter_readings[13].reading_value = Some("999.000".into());
    let (svc, _) = service([
        good(first.clone()),
        good(second.clone()),
        good(second.clone()),
    ]);
    let result = svc
        .compare(svc.snapshot().unwrap(), request())
        .await
        .unwrap();
    assert!(result.comparison.complete);
    assert_eq!(
        result.comparison.disagreements,
        ["meter_readings[13].reading_value"]
    );
    assert_eq!(
        result.extraction.as_ref().unwrap().meter_readings,
        first.meter_readings
    );
    assert_eq!(
        result.extraction.as_ref().unwrap().model,
        "operator-a-version"
    );
    for (index, model) in result.comparison.results.iter().enumerate() {
        let extraction = model.extraction.as_ref().unwrap();
        assert_eq!(extraction.model, registry().models[index].model);
        assert_eq!(
            extraction.meter_readings,
            if index == 0 {
                &first.meter_readings
            } else {
                &second.meter_readings
            }
            .clone()
        );
        assert_eq!(extraction.meter_readings.len(), 14);
    }
}

#[tokio::test]
async fn row_disagreements_cover_every_scalar_exactly_without_normalization() {
    let first = meter_sheet();
    for (field, new_value) in [
        ("row_id", json!("renamed-row")),
        ("unit_number", json!("01")),
        ("meter_number", json!("meter-visible")),
        ("owner_name", json!("Synthetic owner")),
        ("reading_value", json!("100.250")),
        ("reading_date_text", Value::Null),
    ] {
        let mut changed = serde_json::to_value(&first).unwrap();
        changed["meter_readings"][0][field] = new_value;
        if field == "reading_date_text" {
            changed["meter_readings"][0]["evidence"]
                .as_array_mut()
                .unwrap()
                .remove(1);
        }
        let changed: Extraction = serde_json::from_value(changed).unwrap();
        let (svc, _) = service([good(first.clone()), good(changed), Outcome::Failed]);
        let result = svc
            .compare(svc.snapshot().unwrap(), request())
            .await
            .unwrap();
        let mut expected = vec![format!("meter_readings[0].{field}")];
        if field == "row_id" {
            expected.push("meter_readings.order".into());
        }
        expected.sort_unstable();
        assert_eq!(result.comparison.disagreements, expected, "field {field}");
    }
    let mut full_date = first.clone();
    full_date.meter_readings[0].reading_date_text = Some("2026-09-30".into());
    let mut normalized = full_date.clone();
    normalized.meter_readings[0].reading_date = Some("2026-09-30".into());
    let (svc, _) = service([good(full_date), good(normalized), Outcome::Failed]);
    let result = svc
        .compare(svc.snapshot().unwrap(), request())
        .await
        .unwrap();
    assert_eq!(
        result.comparison.disagreements,
        ["meter_readings[0].reading_date"]
    );
}

#[tokio::test]
async fn row_count_and_source_order_differences_are_not_hidden_or_reordered() {
    let first = meter_sheet();
    let mut reversed = first.clone();
    reversed.meter_readings.swap(0, 1);
    let mut shorter = first.clone();
    shorter.meter_readings.pop();
    let (svc, _) = service([
        good(first.clone()),
        good(reversed.clone()),
        good(shorter.clone()),
    ]);
    let result = svc
        .compare(svc.snapshot().unwrap(), request())
        .await
        .unwrap();
    let diffs = &result.comparison.disagreements;
    for expected in [
        "meter_readings.length",
        "meter_readings.order",
        "meter_readings[0].row_id",
        "meter_readings[0].reading_value",
        "meter_readings[0].reading_date_text",
        "meter_readings[13]",
        "meter_readings[13].reading_value",
        "meter_readings[13].owner_name",
    ] {
        assert!(
            diffs.iter().any(|path| path == expected),
            "missing {expected}: {diffs:?}"
        );
    }
    assert_eq!(
        result.extraction.unwrap().meter_readings,
        first.meter_readings
    );
    assert_eq!(
        result.comparison.results[1]
            .extraction
            .as_ref()
            .unwrap()
            .meter_readings,
        reversed.meter_readings
    );
    assert_eq!(
        result.comparison.results[2]
            .extraction
            .as_ref()
            .unwrap()
            .meter_readings,
        shorter.meter_readings
    );
}

#[tokio::test]
async fn row_evidence_warnings_are_preserved_without_becoming_scalar_disagreements() {
    let first = meter_sheet();
    let mut changed = first.clone();
    changed.meter_readings[0]
        .warnings
        .push("Different confidence warning.".into());
    changed.meter_readings[0].evidence[0].quote = "Synthetic quote variant".into();
    let (svc, _) = service([good(first.clone()), good(changed.clone()), Outcome::Failed]);
    let result = svc
        .compare(svc.snapshot().unwrap(), request())
        .await
        .unwrap();
    assert!(result.comparison.disagreements.is_empty());
    assert_eq!(
        result.comparison.results[1]
            .extraction
            .as_ref()
            .unwrap()
            .meter_readings,
        changed.meter_readings
    );
}

#[tokio::test]
async fn malformed_rows_fail_one_provider_without_discarding_good_rows() {
    let good_sheet = meter_sheet();
    let mut duplicate = good_sheet.clone();
    duplicate.meter_readings[1].row_id = duplicate.meter_readings[0].row_id.clone();
    let mut null_evidence = good_sheet.clone();
    null_evidence.meter_readings[0].evidence[0].field = "reading_date".into();
    let (svc, _) = service([
        good(duplicate),
        good(good_sheet.clone()),
        good(null_evidence),
    ]);
    let result = svc
        .compare(svc.snapshot().unwrap(), request())
        .await
        .unwrap();
    assert!(!result.comparison.complete);
    assert_eq!(
        result.extraction.as_ref().unwrap().model,
        "operator-b-version"
    );
    assert_eq!(
        result.extraction.unwrap().meter_readings,
        good_sheet.meter_readings
    );
    for index in [0, 2] {
        assert_eq!(
            result.comparison.results[index].error_code.as_deref(),
            Some("invalid_extraction")
        );
        assert!(result.comparison.results[index].extraction.is_none());
    }
}
