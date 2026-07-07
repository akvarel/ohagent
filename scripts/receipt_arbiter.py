"""Receipt Arbiter v2 — deterministic regex extraction + cross-field validation.

Philosophy: regex > model. The raw_text_dump is ground truth.
Models hallucinate; regex doesn't.

Flow:
  1. Parse raw_text_dump with regex → extract all fields
  2. Cross-validate extracted fields against model's JSON
  3. If regex data differs from model → flag discrepancy
  4. If regex data is internally consistent → use regex (preferred)
  5. Only if BOTH fail → re-extract with fallback model
  6. Cross-validate fallback against raw_text_dump too

Validates ALL fields:
  - Mathematical: Σitems≈subtotal, VAT≈rate%, subtotal+VAT≈total, payment-change≈total
  - Structural: company name, address, reg_nr, vat_nr, IBAN, SWIFT, phone, receipt code
  - Items: names present, prices > 0, quantities > 0
  - Consistency: do extracted fields appear in raw_text_dump?
"""

import json, re, os, base64, time, urllib.request
from pathlib import Path
from dataclasses import dataclass, field
from typing import Optional

EPSILON = 0.03

# ─── Keys ────────────────────────────────────────
with open(os.path.expanduser("~/.ohagent/keys.toml"), "rb") as f:
    import tomllib
    _keys = tomllib.load(f)["keys"]
SCW_KEY = _keys["SCW_SECRET_KEY"]
SCW_PROJECT = _keys.get("SCW_PROJECT_ID", "65e0d091-bc74-485c-8e03-1471b62e110b")
SCW_BASE = f"https://api.scaleway.ai/{SCW_PROJECT}/v1"

# ═══════════════════════════════════════════════════
# REGEX PARSER — deterministic, no LLM
# ═══════════════════════════════════════════════════

def parse_raw_dump_full(raw: str) -> dict:
    """Parse Latvian receipt raw text into structured fields."""
    result = {
        "store_name": "", "address": "", "reg_nr": "", "vat_nr": "",
        "phone": "", "date": "", "time": "", "receipt_number": "",
        "items": [],
        "subtotal": 0.0, "vat_amount": 0.0, "total": 0.0,
        "payment_method": "", "payment_amount": 0.0, "change": 0.0,
        "bank_name": "", "bank_iban": "", "bank_swift": "",
    }
    if not raw or len(raw) < 20:
        return result

    # ── Split into logical lines ──
    # Latvian receipt fields are separated by delimiters like:
    # "Reg.nr.", "PVN", "Tel.", "Summa", "PVN X", "Summa kopā"
    # Also: ". 20.06.2026" (period+space+date starts a new section)
    # First, split on known field boundaries into segments, then split segments on n
    raw = re.sub(r'(?<=[a-zā-ū])\.\s+(?=[A-ZĀ-Ū\d])', '.\n', raw)  # sentence breaks
    raw = re.sub(r'\b(Reg\.\s*[Nn]r\.)', r'\n\1', raw)
    raw = re.sub(r'\b(PV[Nn]\s+(?:[Nn]r\.?\s*)?)', r'\n\1', raw)
    raw = re.sub(r'\b(Tel\.\s*)', r'\n\1', raw)
    raw = re.sub(r'\b(S\/N:)', r'\n\1', raw)
    raw = re.sub(r'\b(Summa\s+(?:kop[āa])?)', r'\n\1', raw)
    raw = re.sub(r'\b(Subtotal:)', r'\n\1', raw)
    raw = re.sub(r'\b(VAT\s+\d+%?:)', r'\n\1', raw)
    raw = re.sub(r'\b(Total:)', r'\n\1', raw)
    raw = re.sub(r'\b(Maksa\s+par\s+pirkumu)', r'\n\1', raw)
    raw = re.sub(r'\b(Paid[īi]ts\s+par\s+pirkumu)', r'\n\1', raw)
    raw = re.sub(r'\b(Atlikums)', r'\n\1', raw)
    raw = re.sub(r'\b(Cash\b|CARD\b|PIV\b)', r'\n\1', raw)
    raw = re.sub(r'\b(Swedbank|SEB\s|Citadele|Luminor|LUNA)\b', r'\n\1', raw)
    raw = re.sub(r'\b(Nor[ēe]ķinu\s+karte)', r'\n\1', raw)
    lines = [l.strip() for l in raw.split("\n") if l.strip()]
    # ── Company name: first non-date, non-number line ──
    for line in lines[:3]:
        line = line.strip()
        if re.match(r'^\d{1,2}[./]\d{1,2}[./]\d{2,4}', line):
            continue
        if re.match(r'^[-\d\s.:]+$', line):
            continue
        if len(line) > 5:
            # Strip trailing address/reg nr-like suffixes
            name = line.strip('"').strip()
            # Remove address suffix and everything after it
            name = re.sub(r'\s+(Lubanas|Jūrmalas|Liel[āa]|Dartijas|Gunara|Br[īi]vadzes|Krasta|Krassta)\s+.*$', '', name)
            # Remove reg nr suffix: "SIA BARIJA Jurmalas..." → stop at first number after store name
            name = re.sub(r'\s+(?:Reg|PVN|LV|Tel|EKA|DOK|S/N|PwV)\b.*$', '', name, flags=re.IGNORECASE)
            # Cap at 60 chars — store names don't exceed this
            if len(name) > 60:
                # Take just the first ~4 words
                words = name.split()
                name = ' '.join(words[:4])
            result["store_name"] = name
            break

    # ── Address: contains "iela", "gatve", etc ──
    for line in lines:
        if re.search(r'(iela|gatve|prospekts|bulv[āa]ris|R[īi]ga|J[ūu]rmala|Kuld[īi]ga|Liep[āa]ja|Daugavpils|Ventspils|R[ēe]zekne)', line, re.IGNORECASE):
            addr = re.sub(r'^.*?(Lubanas|Jūrmalas|Liel[āa]|Dartijas)\s+', r'\1 ', line)
            addr = re.sub(r'\bReg\.?\s*([Nn]r\.?)?\s*\d+.*$', '', addr).strip()
            addr = re.sub(r'\bPV[Nn]\s*([Nn]r\.?)?\s*LV\d+.*$', '', addr).strip()
            addr = re.sub(r'\bTel\.?\s*\d+.*$', '', addr).strip()
            result["address"] = addr.strip(", ")
            break

    # ── Reg nr: "Reg.nr. 4000399995" or "Reg.Nr.40002635" ──
    m = re.search(r'[Rr]eg\.?\s*[Nn]r\.?\s*(\d{8,11})', raw)
    if m: result["reg_nr"] = m.group(1)

    # ── VAT nr: "PVN LV40003999995" or "PVN Nr. LV42503151373" ──
    m = re.search(r'PV[Nn]\s*(?:[Nn]r\.?)?\s*(LV\d{11})', raw)
    if m: result["vat_nr"] = m.group(1)

    # ── Phone: "Tel. 67447747" or "Tālrunis: 20203974" ──
    m = re.search(r'(?:Tel\.?|T[āa]lrunis)[\s:]*(\d[\d\s]{5,15})', raw)
    if m: result["phone"] = re.sub(r'\s+', '', m.group(1))

    # ── Date: "20.06.2026" or "2024-05-12" or "27.07" ──
    for pat in [r'(\d{1,2}[./]\d{1,2}[./]\d{2,4})', r'(\d{4}[./-]\d{2}[./-]\d{2})']:
        m = re.search(pat, raw)
        if m:
            result["date"] = m.group(1)
            break

    # ── Time: "20:11" or "17:17" ──
    m = re.search(r'(\d{1,2}:\d{2}(?::\d{2})?)', raw)
    if m: result["time"] = m.group(1)

    # ── Receipt number: "Nr.201" or "16-02" or "SN:NR0201001010" ──
    for pat in [r'[Nn]r\.?\s*(\d[\d/-]+)', r'S/N:\s*(\w+)']:
        m = re.search(pat, raw)
        if m:
            result["receipt_number"] = m.group(1)
            break

    # ── Items: "Name 1 x 6.90 6.90" ──
    for line in lines:
        line = line.strip()
        if not line or len(line) < 8:
            continue
        # Skip known header/footer lines
        if any(kw in line.lower() for kw in ("summa", "kopa", "pvn", "maksa", "atlikums",
                "paid", "samaks", "censi", "bank", "swift", "reg.", "pvn nr",
                "tel.", "s/n:", "pvn-maksātāja", "kase", "čeks", "pirkums",
                "darījums", "kopsumma", "starpsumma")):
            continue

        # Try "N x P T" or "N X P T" or "N  P  T"
        m = re.match(r'^(.+?)\s+(\d+(?:[.,]\d+)?)\s*[xX×]\s*(\d+(?:[.,]\d+)?)\s+(\d+(?:[.,]\d+)?)\s*$', line)
        if not m:
            m = re.match(r'^(.+?)\s+(\d+(?:[.,]\d+)?)\s+(\d+(?:[.,]\d+)?)\s+(\d+(?:[.,]\d+)?)\s*$', line)

        if m:
            name = m.group(1).strip()
            qty = _num(m.group(2))
            unit = _num(m.group(3))
            total = _num(m.group(4))
            if total > 0 or unit > 0:
                result["items"].append({
                    "name": name,
                    "quantity": qty if qty > 0 else 1.0,
                    "unit_price": unit if unit > 0 else total,
                    "total_price": total if total > 0 else unit * qty,
                })

    # ── Subtotal: "Summa X" ──
    m = re.search(r'(?i)(?:^|\n)\s*summa\s+(\d+(?:[.,]\d+))\s*$', raw, re.MULTILINE)
    if m: result["subtotal"] = _num(m.group(1))

    # ── VAT: "PVN X" or "PVN 21%: X" ──
    for pat in [r'PV[Nn]\s+\d+%?:?\s*(\d+(?:[.,]\d+))', r'PV[Nn]\s+(\d+(?:[.,]\d+))$']:
        m = re.search(pat, raw, re.MULTILINE)
        if m and result["vat_amount"] == 0:
            result["vat_amount"] = _num(m.group(1))
            break

    # ── Total: "Summa kopā Z" ──
    for pat in [r'(?i)summa\s+kop[āa]\s+(\d+(?:[.,]\d+))', r'(?i)kop[āa]\s+(\d+(?:[.,]\d+))$',
                 r'(?i)kopsumma\s+(\d+(?:[.,]\d+))']:
        m = re.search(pat, raw)
        if m and result["total"] == 0:
            result["total"] = _num(m.group(1))
            break

    # If total not found, try last EUR amount
    if result["total"] == 0:
        amounts = re.findall(r'(\d{1,3}(?:[.,]\d{2}))', raw)
        if amounts:
            result["total"] = max(_num(a) for a in amounts)

    # ── Payment: "Maksa par pirkumu: CASH 2.00" or similar ──
    for pat in [r'(?:maksa|samaks[āa]ts?|paid[īi]ts?).*?(\d+(?:[.,]\d+))\s*$',
                 r'(?:maksa|samaks[āa]ts?|paid[īi]ts?).*?(\d+(?:[.,]\d+))']:
        m = re.search(pat, raw, re.IGNORECASE)
        if m and result["payment_amount"] == 0:
            result["payment_amount"] = _num(m.group(1))
            break

    # ── Payment method: CASH / CARD / PIV ──
    m = re.search(r'(?i)\b(CASH|CARD|PIV|SKAI[DT]RA|BANKAS\s*KARTE|NOR[ĒE]ĶINU)\b', raw)
    if m: result["payment_method"] = m.group(1).upper()

    # ── Change: "Atlikums: X" ──
    m = re.search(r'(?i)atlikums:?\s*(\d+(?:[.,]\d+))', raw)
    if m: result["change"] = _num(m.group(1))

    # ── IBAN: "LV84UNLA0020300010000" ──
    m = re.search(r'(LV\d{2}[A-Z0-9]{11,30})', raw, re.IGNORECASE)
    if m: result["bank_iban"] = m.group(1).upper()

    # ── SWIFT: "UNLALV22" or "RIKLV22" ──
    m = re.search(r'([A-Z]{6}[A-Z0-9]{2,5})', raw)
    if m:
        swift = m.group(1)
        # Filter out false positives (store names in ALLCAPS, VAT numbers)
        if re.match(r'^[A-Z]{4}[A-Z]{2}', swift) and not swift.startswith("LV") and swift not in ("SUMMA", "KOPĀ", "MAKSA"):
            result["bank_swift"] = swift

    # ── Bank name: "Swedbank", "AS LUNA", "Citadele" ──
    for bank in ["Swedbank", "SEB", "Citadele", "Luminor", "LUNA", "RIKV", "UNLA", "RTSB"]:
        if bank.lower() in raw.lower():
            result["bank_name"] = bank
            break

    return result


# ═══════════════════════════════════════════════════
# CROSS-FIELD VALIDATOR
# ═══════════════════════════════════════════════════

@dataclass
class FieldCheck:
    field: str
    model_value: str
    regex_value: str
    matches: bool
    in_raw: bool  # does model value appear in raw_text_dump?

@dataclass
class ArbiterVerdict:
    passed: bool
    score: int  # 0-100
    store_name: str
    total: float
    status: str  # "regex_preferred" | "model_ok" | "fallback" | "failed"
    math_checks: dict = field(default_factory=dict)
    field_checks: list = field(default_factory=list)
    issues: list = field(default_factory=list)
    fixes_applied: list = field(default_factory=list)

    def summary(self):
        lines = [f"{'✅' if self.passed else '❌'} {self.store_name} — {self.score}/100 [{self.status}]"]
        lines.append(f"   Total: €{self.total}")
        for ck, cv in self.math_checks.items():
            icon = "✅" if cv.get("ok") else "❌"
            lines.append(f"   {icon} {ck}: {cv.get('detail','')}")
        for fc in self.field_checks:
            if fc.matches and fc.in_raw:
                continue  # Skip boring "everything matches"
            icon = "✅" if fc.matches else "⚠️"
            detail = f"model={fc.model_value[:40]}"
            if fc.regex_value:
                detail += f" regex={fc.regex_value[:40]}"
            if not fc.in_raw:
                detail += " [NOT IN RAW!]"
            lines.append(f"   {icon} {fc.field}: {detail}")
        for fix in self.fixes_applied:
            lines.append(f"   🔧 {fix}")
        for issue in self.issues:
            lines.append(f"   ❌ {issue}")
        return "\n".join(lines)


def validate_full(model_data: dict, model_name: str = "") -> ArbiterVerdict:
    """Full validation: regex extraction + cross-field + math checks."""
    raw = str(model_data.get("raw_text_dump", ""))
    regex_data = parse_raw_dump_full(raw) if raw else {}

    issues = []
    math_checks = {}
    field_checks = []
    fixes = []
    score = 100

    # ── MATH CHECKS (using model data) ────────────
    items = model_data.get("items", []) or []
    subtotal = _to_f(model_data.get("subtotal"))
    vat_amount = _to_f(model_data.get("vat_amount"))
    vat_pct = _to_f(model_data.get("vat_percent"))
    total = _to_f(model_data.get("total"))
    payment = _to_f(model_data.get("payment_amount"))
    change = _to_f(model_data.get("change"))

    item_sum = 0.0
    n_items = len(items)
    if n_items > 0:
        for it in items:
            qty = _to_f(it.get("quantity", 1))
            unit = _to_f(it.get("unit_price", 0))
            item_total = _to_f(it.get("total_price", 0))

            if item_total > 0:
                # If unit_price ≈ item_total, item_total is per-unit — use qty × item_total
                # Otherwise item_total is already quantity-inclusive — use as-is
                if unit > 0 and abs(unit - item_total) <= EPSILON:
                    # "unit_price: 6.90, total_price: 6.90" → qty × item_total
                    item_sum += qty * item_total
                elif qty > 1 and abs(item_total / qty - unit) <= EPSILON:
                    # "unit_price: 6.90, total_price: 13.80, qty: 2" → use item_total as-is
                    item_sum += item_total
                else:
                    # Ambiguous — prefer item_total
                    item_sum += item_total
            elif unit > 0:
                item_sum += qty * unit

    # 1. Items sum — with auto-correction for VAT allocation lines
    if n_items > 0 and subtotal > 0:
        # Detect "Preces attiecinata" pattern — these are VAT allocation lines, not real items
        allocation_items = [it for it in items if "attiecin" in str(it.get("name", "")).lower()]

        if allocation_items and len(allocation_items) == n_items:
            # All items are VAT allocation — sum of their prices != subtotal by design
            # The real items are not on this receipt; just accept the totals as-is
            math_checks["items_sum"] = {
                "ok": True,
                "detail": f"VAT allocation lines (not real items): Σ=€{item_sum:.2f}, subtotal=€{subtotal:.2f}"
            }
        else:
            diff = abs(item_sum - subtotal)
            ok = diff <= EPSILON
            math_checks["items_sum"] = {
                "ok": ok,
                "detail": f"Σ items=€{item_sum:.2f} vs subtotal=€{subtotal:.2f} (Δ=€{diff:.2f})"
            }
            if not ok:
                score -= 20
                issues.append(f"items_sum: €{item_sum:.2f} ≠ subtotal €{subtotal:.2f}")
                # Try regex items
                if regex_data.get("items"):
                    rsum = sum(it["total_price"] for it in regex_data["items"])
                    if subtotal > 0 and abs(rsum - subtotal) <= EPSILON:
                        model_data["items"] = regex_data["items"]
                        fixes.append(f"items: replaced model items with regex (Σ=€{rsum:.2f})")
                        score += 15

    # 2. VAT
    if vat_pct and vat_pct > 0 and subtotal > 0 and vat_amount > 0:
        expected = subtotal * vat_pct / 100
        diff = abs(vat_amount - expected)
        ok = diff <= max(EPSILON, subtotal * 0.02)
        math_checks["vat_calc"] = {
            "ok": ok,
            "detail": f"{vat_pct}% × €{subtotal:.2f} = €{expected:.2f} vs €{vat_amount:.2f}"
        }
        if not ok:
            score -= 15
            issues.append(f"vat: {vat_pct}%×€{subtotal:.2f}=€{expected:.2f} ≠ €{vat_amount:.2f}")
    else:
        math_checks["vat_calc"] = {"ok": True, "detail": "insufficient data"}

    # 3. Total
    if subtotal > 0 and total > 0:
        expected = subtotal + vat_amount
        diff = abs(total - expected)
        ok = diff <= EPSILON
        math_checks["total_check"] = {
            "ok": ok,
            "detail": f"€{subtotal:.2f}+€{vat_amount:.2f}=€{expected:.2f} vs €{total:.2f}"
        }
        if not ok:
            # Try regex total
            rsub = regex_data.get("subtotal", 0) if regex_data else 0
            rvat = regex_data.get("vat_amount", 0) if regex_data else 0
            rtot = regex_data.get("total", 0) if regex_data else 0
            if rtot > 0 and abs(rsub + rvat - rtot) <= EPSILON:
                # Regex numbers are internally consistent
                model_data["subtotal"] = rsub if rsub > 0 else subtotal
                model_data["vat_amount"] = rvat
                model_data["total"] = rtot
                fixes.append(f"totals: regex override sub=€{rsub} vat=€{rvat} total=€{rtot}")
                score += 20
            else:
                score -= 25
                issues.append(f"total: €{subtotal:.2f}+€{vat_amount:.2f}=€{expected:.2f} ≠ €{total:.2f}")
    elif total == 0:
        score -= 25
        issues.append("total: missing")

    # 4. Payment
    if payment > 0 and total > 0:
        expected_change = max(payment - total, 0)
        diff = abs(change - expected_change)
        ok = diff <= EPSILON
        math_checks["payment"] = {
            "ok": ok,
            "detail": f"Paid €{payment:.2f}, total €{total:.2f} → change=€{expected_change:.2f} vs €{change:.2f}"
        }
        if not ok:
            score -= 5
            issues.append(f"payment: paid €{payment:.2f} − total €{total:.2f} ≠ change €{change:.2f}")
    else:
        math_checks["payment"] = {"ok": True, "detail": "no payment data"}

    # ── FIELD CROSS-VALIDATION ─────────────────────
    FIELD_MAP = [
        ("store_name", "company", True),
        ("address", "address", True),
        ("reg_nr", "company", True),
        ("vat_nr", "company", True),
        ("date", "receipt", True),
        ("time", "receipt", False),
        ("receipt_number", "receipt", False),
        ("bank_name", "bank", False),
        ("bank_iban", "bank", True),
        ("bank_swift", "bank", False),
        ("phone", "contact", False),
        ("payment_method", "payment", False),
    ]

    for field, category, critical in FIELD_MAP:
        model_val = str(model_data.get(field, "")).strip()
        regex_val = str(regex_data.get(field, "")).strip() if regex_data else ""

        # Check if model value appears in raw dump
        in_raw = False
        model_val_clean = model_val.strip()
        # "Not visible" / "Nav redzams" / "Not available" = legitimate empty
        is_legit_empty = model_val_clean.lower() in ("", "not visible", "nav redzams", "n/a", "not available", "none")
        if is_legit_empty:
            in_raw = True  # Don't flag as hallucination — model correctly reported it can't see this
        elif model_val_clean and raw and len(model_val_clean) > 2:
            model_norm = re.sub(r'[\s\-"\'.,]', '', model_val_clean.lower())
            raw_norm = re.sub(r'[\s\-"\'.,]', '', raw.lower())
            in_raw = model_norm in raw_norm

        # Match: regex value contains model value (or vice versa), or both empty
        model_norm = re.sub(r'[\s\-"\'.,]', '', model_val.lower()) if model_val else ""
        regex_norm = re.sub(r'[\s\-"\'.,]', '', regex_val.lower()) if regex_val else ""
        matches = (not regex_norm) or (not model_norm) or (model_norm in regex_norm) or (regex_norm in model_norm)

        fc = FieldCheck(field, model_val, regex_val, matches, in_raw)
        field_checks.append(fc)

        if critical and not in_raw and model_val and not is_legit_empty:
            score -= 5
            issues.append(f"{field}: '{model_val}' not found in raw_text_dump — possible hallucination")

        if critical and model_val and regex_val and not matches and not is_legit_empty:
            score -= 3
            issues.append(f"{field}: model='{model_val}' ≠ regex='{regex_val}'")

    # ── Item name cross-check ─────────────────────
    if n_items > 0:
        raw_norm = re.sub(r'[\s\-"\'.,]', '', raw.lower()) if raw else ""
        bad_items = 0
        for item in items[:10]:
            name = str(item.get("name", ""))
            if name and len(name) > 3:
                name_norm = re.sub(r'[\s\-"\'.,]', '', name.lower())
                in_raw = name_norm in raw_norm if raw_norm else False
                if not in_raw:
                    bad_items += 1
        if bad_items > 0:
            score -= bad_items * 2
            issues.append(f"{bad_items}/{n_items} item names not found in raw_text_dump")

    # ── Final score ────────────────────────────────
    score = max(0, min(100, score))
    passed = score >= 70 and not any("hallucination" in i.lower() for i in issues)

    # Determine status
    if fixes:
        status = "regex_fixed"
    elif score >= 90:
        status = "perfect"
    elif score >= 70:
        status = "model_ok"
    else:
        status = "failed"

    store = regex_data.get("store_name") or str(model_data.get("store_name", "???"))
    total_val = regex_data.get("total") or _to_f(model_data.get("total"))

    return ArbiterVerdict(
        passed=passed, score=score, store_name=store, total=total_val,
        status=status, math_checks=math_checks, field_checks=field_checks,
        issues=issues, fixes_applied=fixes,
    )


# ═══════════════════════════════════════════════════
# FALLBACK (last resort)
# ═══════════════════════════════════════════════════

def re_extract(cropped_img_path: str) -> Optional[dict]:
    """Re-extract with fallback models. Pixtral first, then GPT-4o-mini."""
    if not cropped_img_path or not Path(cropped_img_path).exists():
        return None
    b64 = base64.b64encode(Path(cropped_img_path).read_bytes()).decode()
    # Fallback: Mistral-small with higher max_tokens.
    # Pixtral returns HTTP 400 on these images.
    # GPT-4o-mini hallucinates (see MODEL-GUIDE.md).
    for model, provider, base, key, max_tok in [
        ("mistral-small-3.2-24b-instruct-2506", "scaleway", SCW_BASE, SCW_KEY, 8192),
    ]:
        if not key: continue
        try:
            body = json.dumps({
                "model": model, "max_tokens": max_tok, "temperature": 0,
                "response_format": {"type": "json_object"},
                "messages": [{"role": "system", "content":
                    "Extract ALL visible text from this receipt. Return JSON: store_name, address, "
                    "reg_nr, vat_nr, date, time, receipt_number, phone, "
                    "items[{name, quantity, unit_price, total_price}], "
                    "subtotal, vat_amount, vat_percent, total, currency, "
                    "payment_method, payment_amount, change, "
                    "bank_name, bank_iban, bank_swift, raw_text_dump. "
                    "Keep raw_text_dump SHORT — only the printed text."},
                 {"role": "user", "content": [
                    {"type": "text", "text": "Extract all data from this receipt."},
                    {"type": "image_url", "image_url": {"url": f"data:image/jpeg;base64,{b64}"}},
                ]}],
            }).encode()
            req = urllib.request.Request(f"{base}/chat/completions", body,
                {"Authorization": f"Bearer {key}", "Content-Type": "application/json"})
            resp = urllib.request.urlopen(req, timeout=120)
            data = json.loads(resp.read())
            content = data["choices"][0]["message"]["content"] or ""
            try: return json.loads(content)
            except:
                m = re.search(r'\{.*\}', content, re.DOTALL)
                if m:
                    try: return json.loads(m.group(0))
                    except: pass
        except Exception as e:
            print(f"   ❌ {provider}/{model}: {str(e)[:100]}")
    return None


# ═══════════════════════════════════════════════════
# HELPERS
# ═══════════════════════════════════════════════════
def _num(s: str) -> float:
    try: return float(s.replace(",", "."))
    except: return 0.0

def _to_f(val) -> float:
    if val is None: return 0.0
    if isinstance(val, (int, float)): return float(val)
    if isinstance(val, str):
        try:
            return float(re.sub(r'[€%\s]', '', val).replace(",", "."))
        except: return 0.0
    return 0.0


# ═══════════════════════════════════════════════════
# MAIN
# ═══════════════════════════════════════════════════
if __name__ == "__main__":
    with open("scripts/ocr_results.json") as f:
        ocr_data = json.load(f)

    cropped_dir = Path("/tmp/receipts_cropped")
    cropped_files = sorted(cropped_dir.glob("*.jpg"))

    print("=" * 90)
    print("🧮 RECEIPT ARBITER v2 — Regex-first + Full-field validation")
    print("=" * 90)

    results = []
    for i, r in enumerate(ocr_data["ocr_step"]):
        d = r.get("data") or {}
        model = f"{r.get('provider','?')}/{r.get('model','?')}"
        fname = r.get("file", f"receipt_{i}")

        print(f"\n{'─'*90}")
        print(f"📄 Receipt #{i+1}: {fname} ({model})")
        print(f"{'─'*90}")

        verdict = validate_full(d, model)
        print(verdict.summary())

        # Fallback only if regex couldn't fix
        if not verdict.passed and i < len(cropped_files):
            print(f"   🔄 Last resort: re-extraction...")
            new_data = re_extract(str(cropped_files[i]))
            if new_data:
                new_verdict = validate_full(new_data, f"{model}→fallback")
                new_verdict.status = "fallback" if new_verdict.passed else "failed"
                print(f"\n   After fallback:")
                print(new_verdict.summary())
                verdict = new_verdict
                if new_verdict.passed:
                    ocr_data["ocr_step"][i]["data"] = new_data

        results.append(verdict)

    # Save
    with open("scripts/ocr_results.json", "w") as f:
        json.dump(ocr_data, f, indent=2, ensure_ascii=False, default=str)

    # Tally
    print(f"\n{'='*90}")
    n = len(results)
    perfect = [r for r in results if r.status == "perfect"]
    regex_fixed = [r for r in results if r.status == "regex_fixed"]
    fallback = [r for r in results if r.status == "fallback"]
    failed = [r for r in results if not r.passed]
    total_issues = sum(len(r.issues) for r in results)

    print(f"🏁 RESULT: {sum(1 for r in results if r.passed)}/{n} passed")
    print(f"   Perfect:       {len(perfect)} — {', '.join(r.store_name for r in perfect) or 'none'}")
    print(f"   Regex-fixed:   {len(regex_fixed)} — {', '.join(r.store_name for r in regex_fixed) or 'none'}")
    print(f"   Fallback:      {len(fallback)} — {', '.join(r.store_name for r in fallback) or 'none'}")
    print(f"   Failed:        {len(failed)} — {', '.join(r.store_name for r in failed) or 'none'}")

    # Field coverage report
    print(f"\n📊 FIELD COVERAGE:")
    for field in ["store_name", "address", "reg_nr", "vat_nr", "phone", "date", "time",
                   "receipt_number", "bank_name", "bank_iban", "bank_swift"]:
        present = sum(1 for r in ocr_data["ocr_step"]
                      if str(r.get("data", {}).get(field, "")).strip() not in ("", "0", "0.0", "?"))
        print(f"   {field:<18s}: {present}/{len(results)} receipts")
