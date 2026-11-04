// Synthetic observations only. No customer documents or personal data.
fn meter_sheet() -> Extraction {
    let rows: Vec<Value> = (0..14)
        .map(|index| json!({
            "row_id": format!("observation-{}", index + 1),
            "unit_number": ((index / 2) + 1).to_string(),
            "meter_number": null,
            "owner_name": null,
            "reading_value": format!("{}.25", 100 + index),
            "reading_date": null,
            "reading_date_text": if index % 2 == 0 { "30.09" } else { "31.10" },
            "evidence": [
                {"field":"reading_value","page":1,"quote":format!("{}.25", 100 + index)},
                {"field":"reading_date_text","page":1,"quote":if index % 2 == 0 { "30.09" } else { "31.10" }}
            ],
            "warnings": ["Year, meter identifier and owner are not visible."]
        }))
        .collect();
    serde_json::from_value(json!({
        "document_type":"meter_reading",
        "fields": {"invoice_number":null,"issue_date":null,"currency":null,"total_amount":null,
            "supplier_name":null,"supplier_registration_number":null,"meter_number":null,
            "reading_value":null,"reading_date":null,"property_address":"Synthetic building","unit_number":null},
        "meter_readings": rows,
        "evidence": [], "warnings": [], "model":"untrusted-attribution"
    })).unwrap()
}

#[test]
fn fourteen_observations_preserve_partial_dates_and_order() {
    let sheet = meter_sheet();
    assert!(validate_extraction(&sheet).is_ok());
    let value = serde_json::to_value(&sheet).unwrap();
    let rows = value["meter_readings"].as_array().unwrap();
    assert_eq!(rows.len(), 14);
    for (index, row) in rows.iter().enumerate() {
        assert_eq!(row["row_id"], format!("observation-{}", index + 1));
        assert_eq!(row["unit_number"], ((index / 2) + 1).to_string());
        assert_eq!(row["reading_value"], format!("{}.25", 100 + index));
        assert!(row["reading_date"].is_null());
        assert_eq!(
            row["reading_date_text"],
            if index % 2 == 0 { "30.09" } else { "31.10" }
        );
    }
}

#[test]
fn legacy_without_array_still_reads_and_keeps_scalar_reading() {
    let mut legacy = serde_json::to_value(extraction()).unwrap();
    legacy.as_object_mut().unwrap().remove("meter_readings");
    legacy["document_type"] = json!("meter_reading");
    legacy["fields"]["reading_value"] = json!("12.300");
    let result: Extraction = serde_json::from_value(legacy).unwrap();
    assert!(result.meter_readings.is_empty());
    assert_eq!(result.fields.reading_value.as_deref(), Some("12.300"));
    assert!(validate_extraction(&result).is_ok());
}

#[test]
fn every_row_property_is_required_and_unknown_properties_are_rejected() {
    let sheet = serde_json::to_value(meter_sheet()).unwrap();
    let row = sheet["meter_readings"][0].as_object().unwrap();
    assert_eq!(row.len(), 9);
    for name in row.keys() {
        let mut missing = sheet.clone();
        missing["meter_readings"][0]
            .as_object_mut()
            .unwrap()
            .remove(name);
        assert!(
            serde_json::from_value::<Extraction>(missing).is_err(),
            "missing {name}"
        );
    }
    let mut unknown = sheet.clone();
    unknown["meter_readings"][0]["action"] = json!("record");
    assert!(serde_json::from_value::<Extraction>(unknown).is_err());
    let mut wrong = sheet.clone();
    wrong["meter_readings"][0]["reading_value"] = json!(12.3);
    assert!(serde_json::from_value::<Extraction>(wrong).is_err());
    let mut null_array = sheet;
    null_array["meter_readings"] = Value::Null;
    assert!(serde_json::from_value::<Extraction>(null_array).is_err());
}

#[test]
fn rows_reject_invalid_ids_duplicates_overlimit_and_scalar_conflicts() {
    let sheet = meter_sheet();
    for id in ["", " ", "row/1", "row\n1", "dzīvoklis", &"r".repeat(65)] {
        let mut value = sheet.clone();
        value.meter_readings[0].row_id = id.into();
        assert!(validate_extraction(&value).is_err(), "invalid id {id:?}");
    }
    let mut duplicate = sheet.clone();
    duplicate.meter_readings[1].row_id = duplicate.meter_readings[0].row_id.clone();
    assert!(validate_extraction(&duplicate).is_err());
    let mut limit = sheet.clone();
    limit.meter_readings = (0..100)
        .map(|index| {
            let mut row = sheet.meter_readings[0].clone();
            row.row_id = format!("row-{index}");
            row
        })
        .collect();
    assert!(validate_extraction(&limit).is_ok());
    let mut row = limit.meter_readings[0].clone();
    row.row_id = "row-100".into();
    limit.meter_readings.push(row);
    assert!(validate_extraction(&limit).is_err());
    for kind in [
        DocumentType::Invoice,
        DocumentType::Receipt,
        DocumentType::Other,
    ] {
        let mut value = sheet.clone();
        value.document_type = kind;
        assert!(validate_extraction(&value).is_err());
    }
    for name in [
        "meter_number",
        "unit_number",
        "reading_value",
        "reading_date",
    ] {
        let mut value = serde_json::to_value(&sheet).unwrap();
        value["fields"][name] = json!(if name == "reading_date" {
            "2026-10-31"
        } else {
            "123"
        });
        let value: Extraction = serde_json::from_value(value).unwrap();
        assert!(
            validate_extraction(&value).is_err(),
            "scalar conflict {name}"
        );
    }
}

#[test]
fn rows_validate_decimals_dates_uncertainty_and_evidence_bounds() {
    let sheet = meter_sheet();
    for decimal in [
        "-1",
        "NaN",
        "inf",
        "1e2",
        "1,2",
        "",
        " 1",
        ".2",
        "1.",
        "1.1234567890123",
        &"1".repeat(41),
    ] {
        let mut value = sheet.clone();
        value.meter_readings[0].reading_value = Some(decimal.into());
        assert!(validate_extraction(&value).is_err(), "decimal {decimal}");
    }
    for date in ["30.09", "2026-09", "2026-02-30", "2026-9-01", "0000-00-00"] {
        let mut value = sheet.clone();
        value.meter_readings[0].reading_date = Some(date.into());
        assert!(validate_extraction(&value).is_err(), "date {date}");
    }
    let mut valid = sheet.clone();
    valid.meter_readings[0].reading_value = Some("0.000000000001".into());
    valid.meter_readings[0].reading_date = Some("2024-02-29".into());
    valid.meter_readings[0].reading_date_text = Some("29.02.2024".into());
    assert!(validate_extraction(&valid).is_ok());
    for partial in ["30.09", "30.09.", "30/09", "30-09"] {
        let mut invented = sheet.clone();
        invented.meter_readings[0].reading_date = Some("2026-09-30".into());
        invented.meter_readings[0].reading_date_text = Some(partial.into());
        assert!(
            validate_extraction(&invented).is_err(),
            "invented year for {partial}"
        );
    }
    let mut unknown = sheet.clone();
    unknown.meter_readings[0].reading_value = None;
    unknown.meter_readings[0].evidence.remove(0);
    assert!(validate_extraction(&unknown).is_ok());
    unknown.meter_readings[0].warnings.clear();
    assert!(validate_extraction(&unknown).is_err());
    for field in [
        "owner_name",
        "reading_date",
        "property_address",
        "row_id",
        "made_up",
    ] {
        let mut value = sheet.clone();
        value.meter_readings[0].evidence[0].field = field.into();
        assert!(validate_extraction(&value).is_err(), "evidence {field}");
    }
    for page in [0, 10_001] {
        let mut value = sheet.clone();
        value.meter_readings[0].evidence[0].page = page;
        assert!(validate_extraction(&value).is_err());
    }
    for quote in ["".to_string(), "x".repeat(2049), "bad\0quote".into()] {
        let mut value = sheet.clone();
        value.meter_readings[0].evidence[0].quote = quote;
        assert!(validate_extraction(&value).is_err());
    }
    let mut value = sheet.clone();
    value.meter_readings[0].evidence = vec![value.meter_readings[0].evidence[0].clone(); 65];
    assert!(validate_extraction(&value).is_err());
    let mut value = sheet.clone();
    value.meter_readings[0].warnings = vec!["uncertain".into(); 33];
    assert!(validate_extraction(&value).is_err());
    let mut value = sheet.clone();
    value.meter_readings[0].warnings = vec!["x".repeat(2049)];
    assert!(validate_extraction(&value).is_err());
    for name in [
        "unit_number",
        "meter_number",
        "owner_name",
        "reading_date_text",
    ] {
        for text in ["".to_string(), "x".repeat(2049), "bad\0text".into()] {
            let mut value = serde_json::to_value(&sheet).unwrap();
            value["meter_readings"][0][name] = json!(text);
            let value: Extraction = serde_json::from_value(value).unwrap();
            assert!(validate_extraction(&value).is_err(), "text {name}");
        }
    }
}
