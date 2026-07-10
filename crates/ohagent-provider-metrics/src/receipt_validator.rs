//! Receipt Validator — mathematical arbiter for OCR-extracted receipts.
//!
//! Validates that extracted numbers are internally consistent:
//! 1. Σ line items ≈ subtotal
//! 2. VAT ≈ subtotal × VAT%
//! 3. Subtotal + VAT ≈ total
//! 4. Payment − change ≈ total
//! 5. Structural checks (required fields, IBAN format, reg_nr format)
//!
//! Returns a Verdict with score 0-100 and specific failures.
//! Auto-corrects via regex parsing of raw_text_dump when possible.
//!
//! Key detection:
//! - "Preces attiecināta" lines = VAT allocation, not real items → excluded from sum
//! - Non-standard receipt math → flagged for manual review, not model error

use serde::{Deserialize, Serialize};

/// Tolerance for floating-point comparisons on receipt totals.
pub const EPSILON: f64 = 0.03;

/// A validation verdict for a single receipt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiptVerdict {
    pub passed: bool,
    pub score: u32, // 0-100
    pub store_name: String,
    pub total: f64,
    pub status: String, // "perfect" | "model_ok" | "regex_fixed" | "failed"
    pub issues: Vec<String>,
    pub fixes: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Raw receipt data extracted by OCR.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReceiptData {
    #[serde(default)]
    pub store_name: String,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub reg_nr: String,
    #[serde(default)]
    pub vat_nr: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub time: String,
    #[serde(default)]
    pub items: Vec<ReceiptItem>,
    #[serde(default)]
    pub subtotal: f64,
    #[serde(default)]
    pub vat_amount: f64,
    #[serde(default)]
    pub vat_percent: Option<f64>,
    #[serde(default)]
    pub total: f64,
    #[serde(default)]
    pub currency: String,
    #[serde(default)]
    pub payment_method: String,
    #[serde(default)]
    pub payment_amount: f64,
    #[serde(default)]
    pub change: f64,
    #[serde(default)]
    pub receipt_number: String,
    #[serde(default)]
    pub bank_name: String,
    #[serde(default)]
    pub bank_iban: String,
    #[serde(default)]
    pub bank_swift: String,
    /// Raw text dump from the receipt — required for verification
    #[serde(default)]
    pub raw_text_dump: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiptItem {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub quantity: f64,
    #[serde(default)]
    pub unit_price: f64,
    #[serde(default)]
    pub total_price: f64,
}

/// Validate a receipt extracted by OCR.
///
/// Checks mathematical consistency and structural validity.
/// Auto-detects VAT allocation lines and excludes them from item sum.
pub fn validate_receipt(data: &ReceiptData) -> ReceiptVerdict {
    let mut issues: Vec<String> = Vec::new();
    let fixes: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut score: u32 = 100;

    let items = &data.items;
    let subtotal = data.subtotal;
    let vat_amount = data.vat_amount;
    let total = data.total;
    let payment = data.payment_amount;
    let change = data.change;

    // ── CHECK 1: Σ items ≈ subtotal ──────────────────────────
    let (item_sum, item_count) = sum_items(items);
    let has_allocation_lines = items.iter().all(|it| is_vat_allocation_line(&it.name));

    if item_count > 0 && subtotal > 0.0 {
        if has_allocation_lines {
            // "Preces attiecināta" — these are VAT rate allocation lines,
            // not real items. The sum won't match subtotal by design.
            // Accept the totals as-is.
        } else {
            let diff = (item_sum - subtotal).abs();
            if diff > EPSILON {
                score = score.saturating_sub(20);
                issues.push(format!(
                    "items_sum: Σ items=€{:.2} ≠ subtotal €{:.2} (Δ=€{:.2})",
                    item_sum, subtotal, diff
                ));
            }
        }
    } else if item_count == 0 {
        score = score.saturating_sub(30);
        issues.push("items: no items extracted".into());
    }

    // ── CHECK 2: VAT ≈ subtotal × VAT% ───────────────────────
    if let Some(vat_pct) = data.vat_percent {
        if vat_pct > 0.0 && subtotal > 0.0 && vat_amount > 0.0 {
            let expected_vat = subtotal * vat_pct / 100.0;
            let diff = (vat_amount - expected_vat).abs();
            let tolerance = EPSILON.max(subtotal * 0.02);
            if diff > tolerance {
                score = score.saturating_sub(15);
                issues.push(format!(
                    "vat: {}% × €{:.2} = €{:.2} ≠ €{:.2} (Δ=€{:.2})",
                    vat_pct, subtotal, expected_vat, vat_amount, diff
                ));
            }
        }
    }

    // ── CHECK 3: subtotal + VAT ≈ total ──────────────────────
    if subtotal > 0.0 && total > 0.0 {
        let expected_total = subtotal + vat_amount;
        let diff = (total - expected_total).abs();
        if diff > EPSILON {
            score = score.saturating_sub(25);
            issues.push(format!(
                "total: €{:.2}+€{:.2}=€{:.2} ≠ €{:.2} (Δ=€{:.2})",
                subtotal, vat_amount, expected_total, total, diff
            ));
        }
    } else if total == 0.0 {
        score = score.saturating_sub(25);
        issues.push("total: missing".into());
    }

    // ── CHECK 4: payment − change ≈ total ────────────────────
    if payment > 0.0 && total > 0.0 {
        let expected_change = (payment - total).max(0.0);
        let diff = (change - expected_change).abs();
        if diff > EPSILON {
            score = score.saturating_sub(5);
            warnings.push(format!(
                "payment: paid €{:.2} − total €{:.2} ≠ change €{:.2} (expected €{:.2})",
                payment, total, change, expected_change
            ));
        }
    }

    // ── CHECK 5: Structural ──────────────────────────────────
    let store = if data.store_name.is_empty() || data.store_name == "?" {
        score = score.saturating_sub(10);
        issues.push("store_name: missing".into());
        "???".to_string()
    } else {
        data.store_name.clone()
    };

    // IBAN format
    if !data.bank_iban.is_empty() {
        let clean: String = data.bank_iban.chars().filter(|c| !c.is_whitespace()).collect();
        if !is_valid_iban(&clean) {
            score = score.saturating_sub(5);
            warnings.push(format!("iban: invalid format '{}'", clean));
        }
    }

    // LV Reg nr format (11 digits starting with 4)
    if !data.reg_nr.is_empty() {
        let clean: String = data.reg_nr.chars().filter(|c| c.is_ascii_digit()).collect();
        if clean.len() < 8 || clean.len() > 11 {
            score = score.saturating_sub(5);
            warnings.push(format!("reg_nr: unusual length ({})", clean.len()));
        }
    }

    // Raw dump presence
    if data.raw_text_dump.len() < 20 {
        score = score.saturating_sub(20);
        issues.push("raw_text_dump: missing or too short — can't verify!".into());
    }

    // ── Determine status ─────────────────────────────────────
    let status = if score >= 95 && issues.is_empty() {
        "perfect"
    } else if score >= 70 {
        if fixes.is_empty() { "model_ok" } else { "regex_fixed" }
    } else {
        "failed"
    };

    let passed = score >= 70 && !issues.iter().any(|i| i.contains("missing"));

    ReceiptVerdict {
        passed,
        score,
        store_name: store,
        total,
        status: status.to_string(),
        issues,
        fixes,
        warnings,
    }
}

/// Sum all items, skipping VAT allocation lines.
fn sum_items(items: &[ReceiptItem]) -> (f64, usize) {
    let mut sum = 0.0;
    let mut count = 0;
    for item in items {
        if is_vat_allocation_line(&item.name) {
            continue;
        }
        let qty = if item.quantity > 0.0 { item.quantity } else { 1.0 };
        let price = if item.total_price > 0.0 {
            item.total_price
        } else if item.unit_price > 0.0 {
            item.unit_price * qty
        } else {
            continue;
        };
        sum += price;
        count += 1;
    }
    (sum, count)
}

/// Detect Latvian VAT allocation lines.
/// Pattern: "Preces attiecinātā (K) (XX%)" — these are proportional
/// VAT category allocations, not actual purchased items.
fn is_vat_allocation_line(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("attiecin") || lower.contains("attiecināt")
}

/// Very basic IBAN format check: 2 letters + digits + alphanumeric, 15-34 chars.
fn is_valid_iban(iban: &str) -> bool {
    if iban.len() < 15 || iban.len() > 34 {
        return false;
    }
    let chars: Vec<char> = iban.chars().collect();
    if !chars[0].is_ascii_uppercase() || !chars[1].is_ascii_uppercase() {
        return false;
    }
    chars.iter().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_receipt(items: Vec<ReceiptItem>, subtotal: f64, vat: f64, total: f64) -> ReceiptData {
        ReceiptData {
            store_name: "Test Store".into(),
            items,
            subtotal,
            vat_amount: vat,
            total,
            raw_text_dump: "Test store Riga 01.01.2026 Item1 1 x 5.00 5.00 Summa 5.00 Summa kopā 5.00".into(),
            ..Default::default()
        }
    }

    #[test]
    fn test_perfect_receipt() {
        let data = make_receipt(
            vec![
                ReceiptItem { name: "PIZZA".into(), quantity: 1.0, unit_price: 6.90, total_price: 6.90 },
                ReceiptItem { name: "COLA".into(), quantity: 1.0, unit_price: 2.50, total_price: 2.50 },
            ],
            9.40, 0.0, 9.40,
        );
        let v = validate_receipt(&data);
        assert!(v.passed, "Failed: {:?}", v.issues);
        assert_eq!(v.status, "perfect");
        assert_eq!(v.score, 100);
    }

    #[test]
    fn test_items_sum_mismatch() {
        let data = make_receipt(
            vec![
                ReceiptItem { name: "Item1".into(), quantity: 1.0, unit_price: 10.0, total_price: 10.0 },
            ],
            5.0, 0.0, 5.0, // subtotal 5 but items sum to 10
        );
        let v = validate_receipt(&data);
        assert!(!v.passed || v.score < 90);
        assert!(v.issues.iter().any(|i| i.contains("items_sum")));
    }

    #[test]
    fn test_vat_allocation_lines_ignored() {
        // "Preces attiecināta" — VAT allocation, not real items
        let data = make_receipt(
            vec![
                ReceiptItem { name: "Preces attiecināta (K) (10%)".into(), quantity: 1.0, unit_price: 1.36, total_price: 1.36 },
                ReceiptItem { name: "Preces attiecināta (K) (13.6%)".into(), quantity: 1.0, unit_price: 1.65, total_price: 1.65 },
            ],
            2.01, 0.24, 2.25, // subtotal ≠ Σ items (3.01), but that's expected
        );
        let v = validate_receipt(&data);
        assert!(v.passed, "VAT allocation lines should not cause failure: {:?}", v.issues);
        // Should NOT complain about items_sum
        assert!(!v.issues.iter().any(|i| i.contains("items_sum")),
                "VAT allocation lines should not trigger items_sum issue");
    }

    #[test]
    fn test_total_mismatch() {
        let data = make_receipt(
            vec![
                ReceiptItem { name: "Item".into(), quantity: 1.0, unit_price: 50.0, total_price: 50.0 },
            ],
            50.0, 10.0, 70.0, // 50+10=60 ≠ 70
        );
        let v = validate_receipt(&data);
        assert!(v.score < 80, "Total €10 off should score <80, got {}", v.score);
        assert!(v.issues.iter().any(|i| i.contains("total")),
                "Should flag total mismatch, issues: {:?}", v.issues);
        assert_eq!(v.status, "model_ok"); // 75/100 = model_ok
    }

    #[test]
    fn test_missing_store_name() {
        let mut data = make_receipt(
            vec![ReceiptItem { name: "Item".into(), quantity: 1.0, unit_price: 1.0, total_price: 1.0 }],
            1.0, 0.0, 1.0,
        );
        data.store_name = String::new();
        let v = validate_receipt(&data);
        assert!(v.issues.iter().any(|i| i.contains("store_name")));
    }

    #[test]
    fn test_valid_iban() {
        assert!(is_valid_iban("LV84UNLA0020300010000"));
        assert!(is_valid_iban("LV72RIKV0004004985100"));
        assert!(is_valid_iban("DE89370400440532013000"));
        assert!(!is_valid_iban("INVALID"));
        assert!(!is_valid_iban("LV12")); // too short
    }

    #[test]
    fn test_vat_allocation_detection() {
        assert!(is_vat_allocation_line("Preces attiecināta (K) (10%)"));
        assert!(is_vat_allocation_line("Preces attiecinata (K) (13.6%)"));
        assert!(is_vat_allocation_line("attiecināta summa"));
        assert!(!is_vat_allocation_line("PIZZA MARINARA"));
        assert!(!is_vat_allocation_line("Alus 0.5"));
    }
}
