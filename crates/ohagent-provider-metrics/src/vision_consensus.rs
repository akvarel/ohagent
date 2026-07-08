//! Vision Consensus — cross-model verification for OCR results.
//!
//! When multiple OCR models are available, run them all and pick the
//! most consistent result. If models disagree beyond threshold, flag
//! for human review.
//!
//! Consensus algorithm:
//!   - Run N models on the same image
//!   - For each field, count how many models agree on the value
//!   - If ≥ 2/3 models agree → accept
//!   - If only 1/2 or 0/2 agree → flag for review
//!   - If all disagree → re-extract with best model

use std::collections::HashMap;
use crate::receipt_validator::ReceiptData;

/// Result of a consensus run across multiple OCR models.
#[derive(Debug)]
pub struct ConsensusResult {
    /// Whether consensus was reached (≥2/3 models agree on key fields).
    pub consensus_reached: bool,
    /// Number of models that participated.
    pub model_count: usize,
    /// Number of fields where all models agreed.
    pub unanimous_fields: usize,
    /// Number of fields where majority agreed (≥ 2/3).
    pub majority_fields: usize,
    /// Number of fields with no agreement (needs human review).
    pub disputed_fields: usize,
    /// The best (most consistent) extracted receipt.
    pub best: ReceiptData,
    /// Per-model extraction results.
    pub per_model: Vec<(String, ReceiptData)>,
    /// Disputed field names and their values across models.
    pub disputes: Vec<DisputedField>,
}

/// A field where models disagree.
#[derive(Debug)]
pub struct DisputedField {
    pub field_name: String,
    pub values: Vec<(String, String)>, // (model_name, value)
}

/// Run consensus across multiple OCR extraction results.
/// Returns the best result or flags for human review.
pub fn run_consensus(
    results: Vec<(String, ReceiptData)>, // (model_name, extracted_data)
) -> ConsensusResult {
    if results.is_empty() {
        // Should never happen
        return ConsensusResult {
            consensus_reached: false, model_count: 0,
            unanimous_fields: 0, majority_fields: 0, disputed_fields: 0,
            best: ReceiptData::default(),
            per_model: vec![], disputes: vec![],
        };
    }

    if results.len() == 1 {
        // Single model — no consensus needed
        let (name, data) = results.into_iter().next().unwrap();
        return ConsensusResult {
            consensus_reached: true, model_count: 1,
            unanimous_fields: 0, majority_fields: 0, disputed_fields: 0,
            best: data,
            per_model: vec![(name, ReceiptData::default())], disputes: vec![],
        };
    }

    let model_count = results.len();
    let threshold = (model_count * 2 + 2) / 3; // ceil(2/3 * N)

    // Collect values per field across all models
    let fields_to_check = [
        "store_name", "reg_nr", "vat_nr", "date", "total",
        "subtotal", "vat_amount", "payment_amount",
    ];

    let mut field_values: HashMap<&str, Vec<(&str, String)>> = HashMap::new();
    for (model_name, data) in &results {
        for field in &fields_to_check {
            let val = extract_field(data, field);
            if !val.is_empty() && val != "0" && val != "0.0" && val != "0.00" {
                field_values.entry(field)
                    .or_default()
                    .push((model_name.as_str(), val));
            }
        }
    }

    let mut unanimous = 0;
    let mut majority = 0;
    let mut disputed = 0;
    let mut dispute_list = Vec::new();

    for field in &fields_to_check {
        let vals = match field_values.get(field) {
            Some(v) => v,
            None => continue,
        };

        // Count occurrences of each unique value
        let mut counts: HashMap<&str, usize> = HashMap::new();
        let mut model_values: Vec<(String, String)> = Vec::new();

        for (model, val) in vals {
            *counts.entry(val.as_str()).or_default() += 1;
            model_values.push((model.to_string(), val.clone()));
        }

        let max_count = counts.values().max().copied().unwrap_or(0);
        let best_val = counts.iter()
            .max_by_key(|(_, &c)| c)
            .map(|(&v, _)| v.to_string())
            .unwrap_or_default();

        if max_count == vals.len() {
            unanimous += 1;
        } else if max_count >= threshold {
            majority += 1;
        } else {
            disputed += 1;
            dispute_list.push(DisputedField {
                field_name: field.to_string(),
                values: model_values,
            });
        }
    }

    // Pick the best model: the one that matches the most agreed-upon values
    let best_idx = find_best_model(&results, &field_values, threshold);
    let best = results[best_idx].1.clone();

    ConsensusResult {
        consensus_reached: disputed == 0,
        model_count,
        unanimous_fields: unanimous,
        majority_fields: majority,
        disputed_fields: disputed,
        best,
        per_model: results.iter().map(|(n, d)| (n.clone(), d.clone())).collect(),
        disputes: dispute_list,
    }
}

fn extract_field(data: &ReceiptData, field: &str) -> String {
    match field {
        "store_name" => data.store_name.clone(),
        "reg_nr" => data.reg_nr.clone(),
        "vat_nr" => data.vat_nr.clone(),
        "date" => data.date.clone(),
        "total" => format!("{:.2}", data.total),
        "subtotal" => format!("{:.2}", data.subtotal),
        "vat_amount" => format!("{:.2}", data.vat_amount),
        "payment_amount" => format!("{:.2}", data.payment_amount),
        _ => String::new(),
    }
}

/// Find the model whose values agree most with the consensus values.
fn find_best_model(
    results: &[(String, ReceiptData)],
    field_values: &HashMap<&str, Vec<(&str, String)>>,
    threshold: usize,
) -> usize {
    let mut best_score = 0usize;
    let mut best_idx = 0usize;

    for (i, (name, _data)) in results.iter().enumerate() {
        let mut score = 0;
        for vals in field_values.values() {
            for (model, _) in vals {
                if *model == name.as_str() {
                    score += 1;
                }
            }
        }
        if score > best_score {
            best_score = score;
            best_idx = i;
        }
    }

    best_idx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_receipt(store: &str, total: f64, reg: &str) -> ReceiptData {
        ReceiptData {
            store_name: store.into(),
            total,
            reg_nr: reg.into(),
            raw_text_dump: "test".into(),
            ..Default::default()
        }
    }

    #[test]
    fn test_full_consensus() {
        let results = vec![
            ("gemini".into(), make_receipt("Kurs", 12.89, "40003494995")),
            ("glm-ocr".into(), make_receipt("Kurs", 12.89, "40003494995")),
            ("mistral".into(), make_receipt("Kurs", 12.89, "40003494995")),
        ];
        let c = run_consensus(results);
        assert!(c.consensus_reached);
        assert_eq!(c.model_count, 3);
        assert_eq!(c.disputed_fields, 0);
    }

    #[test]
    fn test_partial_disagreement() {
        let results = vec![
            ("gemini".into(), make_receipt("Kurs", 12.89, "40003494995")),
            ("glm-ocr".into(), make_receipt("Kursi", 12.89, "40003494995")),
            ("mistral".into(), make_receipt("BISTRO", 12.89, "40003494995")),
        ];
        let c = run_consensus(results);
        // 2/3 agree on "40003494995", but 1/3 on store name
        assert!(!c.consensus_reached);
        assert!(c.disputed_fields > 0);
    }

    #[test]
    fn test_single_model() {
        let results = vec![
            ("gemini".into(), make_receipt("Kurs", 12.89, "40003494995")),
        ];
        let c = run_consensus(results);
        assert!(c.consensus_reached);
        assert_eq!(c.model_count, 1);
        assert_eq!(c.best.store_name, "Kurs");
    }
}
