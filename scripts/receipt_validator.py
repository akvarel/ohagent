"""Receipt Validator — mathematical arbiter for OCR-extracted receipts.

Validates that extracted numbers are internally consistent:
  1. Σ line items ≈ subtotal
  2. VAT ≈ subtotal × VAT%
  3. Subtotal + VAT ≈ total
  4. Payment - change ≈ total
  5. Structural checks (required fields, IBAN format, reg_nr format)

Returns a Verdict with score 0-100 and specific failures.
"""

import json, re, math
from dataclasses import dataclass, field
from typing import Optional

EPSILON = 0.02  # Allow €0.02 rounding tolerance

@dataclass
class ValidationIssue:
    field: str
    expected: str
    got: str
    severity: str  # "error" | "warning"

@dataclass
class Verdict:
    passed: bool
    score: int  # 0-100
    store_name: str
    total_claimed: float
    issues: list = field(default_factory=list)
    checks: dict = field(default_factory=dict)

    def summary(self):
        lines = [
            f"{'✅' if self.passed else '❌'} {self.store_name} — {self.score}/100",
            f"   Total: €{self.total_claimed}",
        ]
        for ck, cv in self.checks.items():
            icon = "✅" if cv.get("ok") else "❌"
            lines.append(f"   {icon} {ck}: {cv.get('detail','')}")
        for issue in self.issues:
            lines.append(f"   ⚠️  [{issue.severity}] {issue.field}: {issue.expected} (got: {issue.got})")
        return "\n".join(lines)


def validate_receipt(data: dict) -> Verdict:
    """Validate a single extracted receipt."""
    issues = []
    checks = {}

    store = str(data.get("store_name", "???"))
    items = data.get("items", []) or []
    subtotal = _to_float(data.get("subtotal"))
    vat_amount = _to_float(data.get("vat_amount"))
    vat_pct = _to_float(data.get("vat_percent"))
    total = _to_float(data.get("total"))
    payment = _to_float(data.get("payment_amount"))
    change = _to_float(data.get("change"))
    currency = str(data.get("currency", "")).upper()
    reg_nr = str(data.get("reg_nr", ""))
    vat_nr = str(data.get("vat_nr", ""))
    iban = str(data.get("bank_iban", ""))
    date = str(data.get("date", ""))
    raw = str(data.get("raw_text_dump", ""))

    score = 100

    # ── CHECK 1: Σ items ≈ subtotal ────────────────────
    item_sum = 0.0
    item_count = 0
    for item in items:
        qty = _to_float(item.get("quantity", 1))
        price = _to_float(item.get("total_price") or item.get("unit_price", 0))
        if price > 0:
            item_sum += qty * price
            item_count += 1

    if item_count > 0 and subtotal > 0:
        diff = abs(item_sum - subtotal)
        ok = diff <= EPSILON
        checks["items_sum"] = {
            "ok": ok,
            "detail": f"Σ items=€{item_sum:.2f} vs subtotal=€{subtotal:.2f} (Δ=€{diff:.2f})"
        }
        if not ok:
            score -= 20
            issues.append(ValidationIssue("items", f"Sum=€{item_sum:.2f}", f"Subtotal=€{subtotal:.2f}",
                          "error" if diff > 0.10 else "warning"))
    elif item_count > 0:
        checks["items_sum"] = {"ok": True, "detail": f"{item_count} items found, no subtotal to compare"}
    else:
        checks["items_sum"] = {"ok": False, "detail": "No items extracted"}
        score -= 30
        issues.append(ValidationIssue("items", "≥1 item", "0 items", "error"))

    # ── CHECK 2: VAT ≈ subtotal × VAT% ─────────────────
    if vat_pct and vat_pct > 0 and subtotal > 0:
        expected_vat = subtotal * vat_pct / 100
        if vat_amount > 0:
            diff = abs(vat_amount - expected_vat)
            ok = diff <= max(EPSILON, subtotal * 0.02)  # 2% tolerance
            checks["vat_calc"] = {
                "ok": ok,
                "detail": f"{vat_pct}% of €{subtotal:.2f} = €{expected_vat:.2f} vs claimed €{vat_amount:.2f} (Δ=€{diff:.2f})"
            }
            if not ok:
                score -= 15
                issues.append(ValidationIssue("vat_amount", f"€{expected_vat:.2f}", f"€{vat_amount:.2f}",
                              "error" if diff > 1.0 else "warning"))
        else:
            checks["vat_calc"] = {"ok": True, "detail": f"{vat_pct}% rate given, no VAT amount to verify"}
    elif vat_amount > 0 and subtotal > 0:
        implied_rate = vat_amount / subtotal * 100
        checks["vat_calc"] = {"ok": True, "detail": f"Implied VAT rate: {implied_rate:.1f}% (€{vat_amount:.2f} / €{subtotal:.2f})"}
    else:
        checks["vat_calc"] = {"ok": True, "detail": "No VAT data to verify"}

    # ── CHECK 3: subtotal + VAT ≈ total ─────────────────
    if subtotal > 0 and total > 0:
        expected_total = subtotal + vat_amount
        diff = abs(total - expected_total)
        ok = diff <= EPSILON
        checks["total_check"] = {
            "ok": ok,
            "detail": f"€{subtotal:.2f} + €{vat_amount:.2f} = €{expected_total:.2f} vs claimed €{total:.2f} (Δ=€{diff:.2f})"
        }
        if not ok:
            score -= 25
            issues.append(ValidationIssue("total", f"€{expected_total:.2f}", f"€{total:.2f}", "error"))
    elif total > 0:
        checks["total_check"] = {"ok": True, "detail": f"Total=€{total:.2f}, no subtotal to verify"}
    else:
        checks["total_check"] = {"ok": False, "detail": "No total extracted"}
        score -= 25
        issues.append(ValidationIssue("total", "present", "missing", "error"))

    # ── CHECK 4: payment - change ≈ total ───────────────
    if payment > 0 and total > 0:
        expected_change = payment - total
        if change > 0 or expected_change >= 0:
            diff = abs(change - max(expected_change, 0))
            ok = diff <= EPSILON
            checks["payment"] = {
                "ok": ok,
                "detail": f"Paid €{payment:.2f}, total €{total:.2f} → change=€{expected_change:.2f} vs claimed €{change:.2f}"
            }
            if not ok:
                score -= 10
                issues.append(ValidationIssue("change", f"€{expected_change:.2f}", f"€{change:.2f}", "warning"))
        else:
            checks["payment"] = {"ok": True, "detail": f"Paid €{payment:.2f}, total €{total:.2f} — short €{-expected_change:.2f}"}
    elif payment > 0:
        checks["payment"] = {"ok": True, "detail": f"Payment €{payment:.2f}, no total to verify"}
    else:
        checks["payment"] = {"ok": True, "detail": "No payment data"}

    # ── CHECK 5: Structural ─────────────────────────────
    if not store or store in ("?", "???", ""):
        score -= 10
        issues.append(ValidationIssue("store_name", "present", "missing", "error"))

    if currency and currency != "EUR":
        checks["currency"] = {"ok": True, "detail": f"Currency: {currency}"}

    # IBAN format (basic): starts with 2 letters, then numbers
    if iban:
        iban_clean = re.sub(r'\s+', '', iban)
        if re.match(r'^[A-Z]{2}\d{2}[A-Z0-9]{11,30}$', iban_clean):
            checks["iban"] = {"ok": True, "detail": f"IBAN {iban_clean[:8]}... valid format"}
        else:
            checks["iban"] = {"ok": False, "detail": f"IBAN '{iban_clean}' invalid format"}
            score -= 5
            issues.append(ValidationIssue("iban", "valid format", iban, "warning"))
    else:
        checks["iban"] = {"ok": True, "detail": "No IBAN"}

    # Reg nr (Latvia): 11 digits starting with 4
    if reg_nr:
        reg_clean = re.sub(r'\D', '', reg_nr)
        if re.match(r'^\d{11}$', reg_clean) and reg_clean.startswith('4'):
            checks["reg_nr"] = {"ok": True, "detail": f"LV reg {reg_clean}"}
        elif re.match(r'^\d{8,11}$', reg_clean):
            checks["reg_nr"] = {"ok": True, "detail": f"reg {reg_clean} (non-standard length)"}
        else:
            checks["reg_nr"] = {"ok": False, "detail": f"'{reg_nr}' doesn't look like LV reg nr"}
            score -= 5
            issues.append(ValidationIssue("reg_nr", "11-digit LV code", reg_nr, "warning"))
    else:
        checks["reg_nr"] = {"ok": True, "detail": "No reg nr"}

    # VAT nr (Latvia): LV + 11 digits
    if vat_nr:
        vat_clean = re.sub(r'[\s-]', '', vat_nr).upper()
        if re.match(r'^LV\d{11}$', vat_clean):
            checks["vat_nr"] = {"ok": True, "detail": f"VAT {vat_clean}"}
        else:
            checks["vat_nr"] = {"ok": False, "detail": f"'{vat_nr}' doesn't look like LV VAT nr"}
            score -= 3
    else:
        checks["vat_nr"] = {"ok": True, "detail": "No VAT nr"}

    # Date presence
    if date:
        checks["date"] = {"ok": True, "detail": date}
    else:
        checks["date"] = {"ok": False, "detail": "No date"}
        score -= 3

    # Raw dump presence (can't verify without it!)
    if raw and len(raw) > 20:
        checks["raw_dump"] = {"ok": True, "detail": f"{len(raw)} chars"}
    else:
        checks["raw_dump"] = {"ok": False, "detail": "Missing or too short — can't verify!"}
        score -= 20
        issues.append(ValidationIssue("raw_text_dump", "≥20 chars", f"{len(raw)} chars", "error"))

    passed = score >= 70 and not any(i.severity == "error" for i in issues)

    return Verdict(
        passed=passed,
        score=max(0, min(100, score)),
        store_name=store,
        total_claimed=total,
        issues=issues,
        checks=checks,
    )


def _to_float(val) -> float:
    """Parse float from JSON value — handles strings, ints, nulls."""
    if val is None:
        return 0.0
    if isinstance(val, (int, float)):
        return float(val)
    if isinstance(val, str):
        # Handle "21%" → 21.0, "€12.50" → 12.50
        cleaned = re.sub(r'[€%\s]', '', val)
        try:
            return float(cleaned)
        except ValueError:
            return 0.0
    return 0.0


# ═══════════════════════════════════════════════════════════════
# Run against our 4 extracted receipts
# ═══════════════════════════════════════════════════════════════
if __name__ == "__main__":
    with open("scripts/ocr_results.json") as f:
        data = json.load(f)

    print("=" * 85)
    print("🧮 RECEIPT VALIDATOR — Mathematical Arbiter")
    print("=" * 85)
    print(f"Checks: Σitems≈subtotal | VAT≈subtotal×rate% | subtotal+VAT≈total")
    print(f"        payment-change≈total | IBAN format | reg_nr format")
    print()

    all_passed = True
    for i, r in enumerate(data["ocr_step"]):
        d = r.get("data") or {}
        verdict = validate_receipt(d)
        print(verdict.summary())
        print()

        if not verdict.passed:
            all_passed = False

        # Show raw dump for failed receipts
        if not verdict.passed and d.get("raw_text_dump"):
            rd = d["raw_text_dump"]
            print(f"   📝 RAW TEXT: {rd[:200]}")
            print()

    print("=" * 85)
    n = len(data["ocr_step"])
    passed_n = sum(1 for r in data["ocr_step"] if validate_receipt(r.get("data") or {}).passed)
    print(f"🏁 RESULT: {passed_n}/{n} receipts passed validation")
    print(f"   {'✅ ALL VALID' if all_passed else '❌ SOME FAILED — needs re-extraction'}")
