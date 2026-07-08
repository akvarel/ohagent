#!/usr/bin/env python3
"""Receipt pipeline: Gemini → Arbiter. Two steps. 4 seconds. Free.

Gemini 3.1 Flash-Lite reads the photo directly — no bbox, crop, or per-receipt OCR needed.
Mathematical arbiter catches the one known weakness (gross/net confusion in subtotal).
"""

import base64, json, time, urllib.request, os, sys, re
from pathlib import Path

IMG_PATH = sys.argv[1] if len(sys.argv) > 1 else \
    "/sharedssd/git/orangehat/accounting/OHAccounting/raw/images/photo_2026-06-27_00-01-24-norm.jpg"

OUT_DIR = Path("/tmp/receipts_pipeline")
OUT_DIR.mkdir(exist_ok=True)

# ─── Keys ───────────────────────────────────────
with open(os.path.expanduser("~/.ohagent/keys.toml"), "rb") as f:
    import tomllib; keys = tomllib.load(f)["keys"]
GOOGLE_KEY = keys.get("GOOGLE_API_KEY", "")

if not GOOGLE_KEY:
    print("❌ GOOGLE_API_KEY not found in ~/.ohagent/keys.toml")
    print("   Add it: [keys]  GOOGLE_API_KEY = \"...\"")
    sys.exit(1)

# ─── Helpers ─────────────────────────────────────
def _n(s) -> float:
    if s is None: return 0.0
    if isinstance(s, (int, float)): return float(s)
    try: return float(str(s).replace(",", ".").replace(" ", ""))
    except: return 0.0

# ─── STEP 1: Gemini OCR ─────────────────────────

def gemini_ocr(image_b64: str) -> list[dict] | None:
    """One call to Gemini: photo → JSON array of receipts."""
    body = json.dumps({
        "contents": [{"parts": [
            {"text": "This image contains paper receipts on a dark surface. "
             "Extract ALL data from EACH receipt. For each receipt return: "
             "store_name, address, reg_nr, vat_nr, date, time, "
             "items[{name, quantity, unit_price, total_price}], "
             "subtotal, vat_amount, vat_percent, total, "
             "payment_method, payment_amount, change. "
             "VAT is two letters+numbers. Each receipt HAS either VAT or REGISTRATION number. "
             "Return as JSON array. No markdown, just JSON."},
            {"inline_data": {"mime_type": "image/jpeg", "data": image_b64}},
        ]}],
        "generationConfig": {"maxOutputTokens": 8192, "temperature": 0},
    }).encode()

    models = ["gemini-3.1-flash-lite", "gemini-flash-latest"]
    for model in models:
        try:
            url = f"https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent?key={GOOGLE_KEY}"
            req = urllib.request.Request(url, body, {"Content-Type": "application/json"})
            resp = urllib.request.urlopen(req, timeout=60)
            data = json.loads(resp.read())
            text = ""
            for part in data.get("candidates", [{}])[0].get("content", {}).get("parts", []):
                text += part.get("text", "")
            json_match = re.search(r'\[.*\]', text, re.DOTALL)
            if json_match:
                return json.loads(json_match.group(0)), model, data.get("usageMetadata", {})
        except Exception as e:
            print(f"   ⚠️  {model}: {str(e)[:80]}")
            continue
    return None, None, None

def normalize_receipt(d: dict) -> dict:
    """Normalize Gemini output to our standard schema."""
    items = []
    for it in (d.get("items") or []):
        qty = _n(it.get("quantity", 1)) or 1.0
        unit = _n(it.get("unit_price", 0))
        it_total = _n(it.get("total_price", 0))
        if it_total == 0 and unit > 0: it_total = qty * unit
        items.append({"name": str(it.get("name") or it.get("description") or ""),
                       "quantity": qty, "unit_price": unit, "total_price": it_total})

    sub = _n(d.get("subtotal", 0))
    vat_amt = _n(d.get("vat_amount", 0))
    total = _n(d.get("total", 0))
    vat_pct = d.get("vat_percent")

    # Extract from vat_details if available
    if (sub == 0 or total == 0) and d.get("vat_details"):
        vd = d["vat_details"]; vd0 = vd[0] if isinstance(vd, list) and vd else vd
        if isinstance(vd0, dict):
            if sub == 0: sub = _n(vd0.get("net_amount", 0))
            if vat_amt == 0: vat_amt = _n(vd0.get("vat_amount", 0))
            if total == 0: total = _n(vd0.get("gross_amount", 0))
            if not vat_pct: vat_pct = vd0.get("rate", "")

    # Compute missing values
    if total == 0 and sub > 0: total = sub + vat_amt
    if sub == 0 and total > 0 and vat_amt > 0: sub = total - vat_amt

    # Auto-correct: detect gross-priced items vs net subtotal.
    # Latvian receipts print item prices WITH VAT. Subtotal/total are NET.
    # If items_sum ≈ subtotal+VAT (within rounding), items are gross → net.
    pos_items = [it for it in items if it["total_price"] > 0]
    pos_sum = sum(it["total_price"] for it in pos_items) if pos_items else 0
    if pos_sum > 0 and sub > 0 and abs(pos_sum - (sub + vat_amt)) <= max(0.15, (sub+vat_amt)*0.02):
        # Items are gross (with VAT). Convert to net for mathematical consistency.
        ratio = sub / pos_sum if pos_sum > 0 else 1.0
        for it in items:
            it["total_price"] = round(it["total_price"] * ratio, 2)
            it["unit_price"] = round(it["unit_price"] * ratio, 2)

    # Build raw_text_dump
    raw = []
    store = str(d.get("store_name") or d.get("merchant_name") or "")
    if store: raw.append(store)
    addr = str(d.get("address") or d.get("merchant_address") or "")
    if addr: raw.append(addr)
    reg = str(d.get("reg_nr") or d.get("company_reg_nr") or "")
    if reg: raw.append(f"Reg.nr. {reg}")
    vat = str(d.get("vat_nr") or d.get("company_vat_nr") or "")
    if vat: raw.append(f"PVN {vat}")
    ds = str(d.get("date", "")); ts = str(d.get("time", ""))
    if ds: raw.append(f"{ds} {ts}".strip())
    for it in items:
        raw.append(f"{it['name']} {it['quantity']} x {it['unit_price']} {it['total_price']}")
    raw.append(f"Summa {sub} PVN {vat_amt} Summa kopā {total}")
    pay = _n(d.get("payment_amount") or d.get("amount_paid", 0))
    chg = _n(d.get("change", 0))
    if pay > 0: raw.append(f"Samaksāts {pay} Atlikums {chg}")

    return {
        "store_name": store, "address": addr,
        "reg_nr": str(d.get("reg_nr") or d.get("company_reg_nr") or ""),
        "vat_nr": str(d.get("vat_nr") or d.get("company_vat_nr") or ""),
        "date": str(d.get("date", "")), "time": str(d.get("time", "")),
        "receipt_number": str(d.get("receipt_number") or d.get("order_number") or ""),
        "items": items, "subtotal": sub, "vat_amount": vat_amt,
        "vat_percent": _n(vat_pct) if vat_pct else None, "total": total,
        "currency": str(d.get("currency", "EUR")),
        "payment_method": str(d.get("payment_method") or ""),
        "payment_amount": pay, "change": chg,
        "raw_text_dump": "\n".join(raw),
    }

# ═════════════════════════════════════════════════
# MAIN
# ═════════════════════════════════════════════════

img_path = Path(IMG_PATH)
b64 = base64.b64encode(img_path.read_bytes()).decode()
print(f"📸 {img_path.name}")

# ── STEP 1: Gemini ──
print(f"\n🔍 STEP 1: Gemini → receipts")
t0 = time.time()
raw_receipts, model_used, usage = gemini_ocr(b64)
elapsed = time.time() - t0

if not raw_receipts:
    print("❌ Gemini failed. Exiting.")
    sys.exit(1)

receipts = [normalize_receipt(r) for r in raw_receipts]
tok = usage.get("promptTokenCount", 0) + usage.get("candidatesTokenCount", 0)
print(f"   ✅ {model_used}: {elapsed:.1f}s, {tok} tok, FREE → {len(receipts)} receipts")

# ── STEP 2: Arbiter ──
print(f"\n🧮 STEP 2: Arbiter")
sys.path.insert(0, str(Path(__file__).parent))
from receipt_arbiter import validate_full

results = []
for i, receipt in enumerate(receipts):
    v = validate_full(receipt, model_used)
    results.append(v)

# ── OUTPUT ──
print(f"\n{'='*85}")
print(f"🏁 {img_path.name} — {elapsed:.0f}s, FREE")
print(f"{'='*85}")

for i, receipt in enumerate(receipts):
    v = results[i]
    icon = "✅" if v.passed else "❌"
    print(f"\n{icon} RECEIPT #{i+1} — {v.store_name} — {v.score}/100 [{v.status}]")
    print(f"   Total: €{v.total:.2f}  |  {receipt.get('address','')}")
    if receipt.get('reg_nr') or receipt.get('vat_nr'):
        print(f"   Reg: {receipt['reg_nr']}  VAT: {receipt['vat_nr']}")
    dstr = f"{receipt.get('date','')} {receipt.get('time','')}".strip()
    if dstr: print(f"   {dstr}")
    print(f"   Sub: €{receipt['subtotal']:.2f}  VAT: €{receipt['vat_amount']:.2f} ({receipt.get('vat_percent','?')}%)  TOT: €{receipt['total']:.2f}")
    pay = receipt.get('payment_amount', 0)
    if pay:
        print(f"   {receipt.get('payment_method','')} €{pay:.2f}  Change: €{receipt.get('change',0):.2f}")
    for it in receipt.get('items', []):
        print(f"   📦 {it['name'][:65]:<67s} ×{it.get('quantity',1)}  €{it.get('total_price',0):.2f}")
    if v.issues:
        print(f"   ⚠️  {'; '.join(v.issues[:3])}")

# ── Summary ──
print(f"\n{'='*85}")
passed = [r for r in results if r.passed]
print(f"🏁 {len(passed)}/{len(results)} passed  |  {elapsed:.0f}s  |  FREE  |  {model_used}")

# Save
with open(f"{OUT_DIR}/pipeline_results.json", "w") as f:
    json.dump({"image": IMG_PATH, "model": model_used, "ttf_s": round(elapsed,1),
        "receipts": receipts,
        "arbiter": [{"store_name": v.store_name, "score": v.score, "passed": v.passed,
                      "issues": v.issues, "fixes": v.fixes_applied} for v in results]},
    f, indent=2, ensure_ascii=False, default=str)
print(f"📁 {OUT_DIR}/pipeline_results.json")
