"""Full end-to-end receipt pipeline: PreClassify → BBox → Crop → OCR → Arbiter.

Single script. One photo in, validated JSONs out.
Uses: Scaleway Mistral-small (preclassify), GLM-4.6V (bbox), Mistral-small (OCR),
      ReceiptArbiter (validation).
"""

import base64, json, time, urllib.request, os, sys, re
from pathlib import Path
from PIL import Image, ImageFilter, ImageEnhance

IMG_PATH = sys.argv[1] if len(sys.argv) > 1 else \
    "/sharedssd/git/orangehat/accounting/OHAccounting/raw/images/photo_2026-06-27_00-01-24-norm.jpg"

OUT_DIR = Path("/tmp/receipts_pipeline")
OUT_DIR.mkdir(exist_ok=True)

# ─── Keys ───────────────────────────────────────
with open(os.path.expanduser("~/.ohagent/keys.toml"), "rb") as f:
    import tomllib
    keys = tomllib.load(f)["keys"]

SCW_KEY = keys["SCW_SECRET_KEY"]
SCW_PROJECT = keys.get("SCW_PROJECT_ID", "65e0d091-bc74-485c-8e03-1471b62e110b")
SCW_BASE = f"https://api.scaleway.ai/{SCW_PROJECT}/v1"
ZAI_KEY = keys["ZAI_API_KEY"]
ZAI_BASE = "https://api.z.ai/api/paas/v4"

# ─── Image ──────────────────────────────────────
img = Image.open(IMG_PATH)
orig_w, orig_h = img.size
print(f"📸 {Path(IMG_PATH).name}: {orig_w}×{orig_h}, {img.mode}")

# Resize for API if needed — keep ≤2048 max dim
api_img = img.copy()
if max(orig_w, orig_h) > 2048:
    scale = 2048 / max(orig_w, orig_h)
    api_img = img.resize((int(orig_w*scale), int(orig_h*scale)), Image.LANCZOS)
api_w, api_h = api_img.size
api_img_path = OUT_DIR / "input.jpg"
api_img.save(api_img_path, "JPEG", quality=92)
b64 = base64.b64encode(Path(api_img_path).read_bytes()).decode()
scale_x = orig_w / api_w
scale_y = orig_h / api_h

total_cost = 0.0
total_time = 0.0

# ═════════════════════════════════════════════════
# STEP 0: PreClassifier — count documents
# ═════════════════════════════════════════════════
print("\n🔢 STEP 0: PreClassifier — How many documents?")

t0 = time.time()
body = json.dumps({
    "model": "mistral-small-3.2-24b-instruct-2506",
    "messages": [{"role": "user", "content": [
        {"type": "text", "text": "How many distinct documents, receipts, or separate items are visible in this image? Answer with ONLY a single integer number, nothing else."},
        {"type": "image_url", "image_url": {"url": f"data:image/jpeg;base64,{b64}"}},
    ]}],
    "max_tokens": 10, "temperature": 0,
}).encode()

req = urllib.request.Request(f"{SCW_BASE}/chat/completions", body,
    {"Authorization": f"Bearer {SCW_KEY}", "Content-Type": "application/json"})
resp = urllib.request.urlopen(req, timeout=120)
data = json.loads(resp.read())
ttf0 = time.time() - t0
usage = data.get("usage", {})
pt0, ct0 = usage.get("prompt_tokens", 0), usage.get("completion_tokens", 0)
answer = data["choices"][0]["message"]["content"].strip()
cost0 = (pt0/1e6*0.15 + ct0/1e6*0.35)
total_cost += cost0
total_time += ttf0

try: n_docs = int(answer)
except: n_docs = -1
print(f"   ✅ {ttf0:.1f}s, {pt0}+{ct0} tok, €{cost0:.4f} → {n_docs} documents")

if n_docs <= 1:
    print("   Single document — skipping bbox, OCR directly...")
    # ... (simplified single-doc path — skip for now)
    sys.exit(0)

# ═════════════════════════════════════════════════
# STEP 1: BBox Detection — GLM-4.6V
# ═════════════════════════════════════════════════
print(f"\n📦 STEP 1: BBox Detection — GLM-4.6V")

t0 = time.time()
body = json.dumps({
    "model": "glm-4.6v",
    "messages": [{"role": "user", "content": [
        {"type": "text", "text": "This image contains multiple paper receipts/documents. Detect EACH distinct receipt. Return bounding boxes [[xmin,ymin,xmax,ymax]] and rotation_degrees (0 if text is already upright). Return valid JSON: {\"receipts\":[{\"index\":1,\"store_name_hint\":\"...\",\"bbox\":[xmin,ymin,xmax,ymax],\"rotation_degrees\":0}],\"total_count\":" + str(n_docs) + "}"},
        {"type": "image_url", "image_url": {"url": f"data:image/jpeg;base64,{b64}"}},
    ]}],
    "max_tokens": 4000, "temperature": 0,
    "thinking": {"type": "disabled"},
}).encode()

req = urllib.request.Request(f"{ZAI_BASE}/chat/completions", body,
    {"Authorization": f"Bearer {ZAI_KEY}", "Content-Type": "application/json"})
resp = urllib.request.urlopen(req, timeout=180)
data = json.loads(resp.read())
ttf1 = time.time() - t0
usage = data.get("usage", {})
pt1, ct1 = usage.get("prompt_tokens", 0), usage.get("completion_tokens", 0)
content = data["choices"][0]["message"]["content"] or ""
cost1 = (pt1/1e6*3.0 + ct1/1e6*15.0) * 0.13  # CNY→EUR
total_cost += cost1
total_time += ttf1

# Parse bbox
bbox_data = None
try:
    m = re.search(r'\{.*\}', content, re.DOTALL)
    if m: bbox_data = json.loads(m.group(0))
except: pass

if not bbox_data or "receipts" not in bbox_data:
    print(f"❌ Failed to parse bbox. Raw: {content[:500]}")
    sys.exit(1)

receipts = bbox_data.get("receipts", [])
print(f"   ✅ {ttf1:.1f}s, {pt1}+{ct1} tok, €{cost1:.4f} → {len(receipts)} bboxes")

# ═════════════════════════════════════════════════
# STEP 2: Crop + Enhance
# ═════════════════════════════════════════════════
print(f"\n✂️  STEP 2: Crop + Enhance")

cropped_files = []
for r in receipts:
    bbox = r.get("bbox", [])
    if len(bbox) != 4: continue

    xmin = int(bbox[0] * scale_x)
    ymin = int(bbox[1] * scale_y)
    xmax = int(bbox[2] * scale_x)
    ymax = int(bbox[3] * scale_y)

    # 5% padding
    w, h = xmax - xmin, ymax - ymin
    xmin = max(0, xmin - int(w * 0.05))
    ymin = max(0, ymin - int(h * 0.05))
    xmax = min(orig_w, xmax + int(w * 0.05))
    ymax = min(orig_h, ymax + int(h * 0.05))

    cropped = img.crop((xmin, ymin, xmax, ymax))

    angle = r.get("rotation_degrees", 0)
    if angle != 0:
        cropped = cropped.rotate(-angle, expand=True, resample=Image.BICUBIC, fillcolor="white")

    cropped = cropped.filter(ImageFilter.SHARPEN)
    enhancer = ImageEnhance.Contrast(cropped)
    cropped = enhancer.enhance(1.3)

    idx = r.get("index", len(cropped_files) + 1)
    name = r.get("store_name_hint", f"receipt_{idx}")
    fname = OUT_DIR / f"{idx:02d}_{re.sub(r'[^a-zA-Z0-9_-]','_',str(name))[:40]}.jpg"
    cropped.save(fname, "JPEG", quality=92)
    cropped_files.append(fname)
    print(f"   ✅ {fname.name}: {cropped.size[0]}×{cropped.size[1]}, rotate={angle}°")

# ═════════════════════════════════════════════════
# STEP 3: OCR each receipt — Mistral-small
# ═════════════════════════════════════════════════
print(f"\n🔍 STEP 3: OCR ({len(cropped_files)} receipts)")

OCR_PROMPT = "Extract ALL visible text from this receipt. Return JSON: store_name, address, reg_nr, vat_nr, phone, date, time, receipt_number, items[{name, quantity, unit_price, total_price}], subtotal, vat_amount, vat_percent, total, currency, payment_method, payment_amount, change, bank_name, bank_iban, bank_swift, raw_text_dump. Keep raw_text_dump SHORT — only the printed text, one line per field."

all_ocr = []
for fpath in cropped_files:
    b64_local = base64.b64encode(Path(fpath).read_bytes()).decode()

    body = json.dumps({
        "model": "mistral-small-3.2-24b-instruct-2506",
        "messages": [
            {"role": "system", "content": OCR_PROMPT},
            {"role": "user", "content": [
                {"type": "text", "text": "Extract all data from this receipt."},
                {"type": "image_url", "image_url": {"url": f"data:image/jpeg;base64,{b64_local}"}},
            ]},
        ],
        "max_tokens": 8192, "temperature": 0,
        "response_format": {"type": "json_object"},
    }).encode()

    t0 = time.time()
    try:
        req = urllib.request.Request(f"{SCW_BASE}/chat/completions", body,
            {"Authorization": f"Bearer {SCW_KEY}", "Content-Type": "application/json"})
        resp = urllib.request.urlopen(req, timeout=180)
        data = json.loads(resp.read())
        elapsed = time.time() - t0
        usage = data.get("usage", {})
        pt_i, ct_i = usage.get("prompt_tokens", 0), usage.get("completion_tokens", 0)
        content_i = data["choices"][0]["message"]["content"] or ""

        parsed = None
        try: parsed = json.loads(content_i)
        except:
            m = re.search(r'\{.*\}', content_i, re.DOTALL)
            if m:
                try: parsed = json.loads(m.group(0))
                except: pass

        cost_i = (pt_i/1e6*0.15 + ct_i/1e6*0.35)
        total_cost += cost_i
        total_time += elapsed

        result = {"file": fpath.name, "ttf_s": round(elapsed,1), "cost_eur": round(cost_i,6),
                   "prompt_tok": pt_i, "comp_tok": ct_i, "data": parsed}
        all_ocr.append(result)

        store = parsed.get("store_name","?") if parsed else "?"
        total_val = parsed.get("total","?") if parsed else "?"
        items_n = len(parsed.get("items",[])) if parsed else 0
        print(f"   ✅ {fpath.name}: {elapsed:.1f}s, {pt_i}+{ct_i} tok, €{cost_i:.4f}, \"{store}\" total=€{total_val}, {items_n} items")

    except Exception as e:
        print(f"   ❌ {fpath.name}: {e}")

# ═════════════════════════════════════════════════
# STEP 4: Arbiter validation
# ═════════════════════════════════════════════════
print(f"\n🧮 STEP 4: Arbiter Validation")

# Import arbiter functions from our script
sys.path.insert(0, str(Path(__file__).parent))
from receipt_arbiter import validate_full, re_extract

arbiter_results = []
for i, r in enumerate(all_ocr):
    d = r.get("data") or {}
    verdict = validate_full(d, f"mistral-small")
    print(f"\n{'─'*70}")
    print(f"📄 {r['file']}")
    print(verdict.summary())

    if not verdict.passed:
        print(f"   🔄 Re-extracting...")
        cropped_path = OUT_DIR / r["file"]
        new_data = re_extract(str(cropped_path))
        if new_data:
            v2 = validate_full(new_data, "mistral-small→fallback")
            v2.status = "fallback" if v2.passed else "failed"
            print(f"   After fallback:")
            print(v2.summary())
            verdict = v2
            if v2.passed:
                r["data"] = new_data

    arbiter_results.append(verdict)

# ═════════════════════════════════════════════════
# FINAL SUMMARY
# ═════════════════════════════════════════════════
print(f"\n{'='*80}")
print(f"🏁 PIPELINE COMPLETE — {Path(IMG_PATH).name}")
print(f"{'='*80}")

passed = [r for r in arbiter_results if r.passed]
failed = [r for r in arbiter_results if not r.passed]

print(f"  Documents found: {n_docs}")
print(f"  Extracted:       {len(all_ocr)}")
print(f"  Passed:          {len(passed)}  ({', '.join(r.store_name for r in passed) or 'none'})")
print(f"  Failed:          {len(failed)}  ({', '.join(r.store_name for r in failed) or 'none'})")
print(f"  Total cost:      €{total_cost:.4f}")
print(f"  Total time:      {total_time:.0f}s")
print(f"  Per-receipt:     €{total_cost/max(len(all_ocr),1):.4f}")

# Print full data for each receipt
print(f"\n{'='*80}")
print("📋 EXTRACTED DATA")
print(f"{'='*80}")

for i, r in enumerate(all_ocr):
    d = r.get("data") or {}
    verdict = arbiter_results[i] if i < len(arbiter_results) else None
    icon = "✅" if (verdict and verdict.passed) else "❌"
    score = verdict.score if verdict else 0

    print(f"\n{'─'*80}")
    print(f"{icon} RECEIPT #{i+1}: {r['file']} — {score}/100")
    print(f"{'─'*80}")
    print(f"  Store:    {d.get('store_name', '???')}")
    print(f"  Address:  {d.get('address', '')}")
    print(f"  Reg nr:   {d.get('reg_nr', '')}")
    print(f"  VAT nr:   {d.get('vat_nr', '')}")
    print(f"  Phone:    {d.get('phone', '')}")
    print(f"  Date:     {d.get('date', '')} {d.get('time', '')}")
    print(f"  Receipt #:{d.get('receipt_number', '')}")
    print(f"  Bank:     {d.get('bank_name', '')} | IBAN: {d.get('bank_iban', '')} | SWIFT: {d.get('bank_swift', '')}")
    print(f"  Subtotal: €{d.get('subtotal', '?')} | VAT: €{d.get('vat_amount', '?')} | TOTAL: €{d.get('total', '?')}")
    print(f"  Payment:  {d.get('payment_method', '')} €{d.get('payment_amount', '')} | Change: €{d.get('change', '')}")
    items = d.get('items', [])
    print(f"  Items ({len(items)}):")
    for item in items:
        print(f"    - {item.get('name','?')[:60]:<62s} ×{item.get('quantity',1)}  €{item.get('total_price', item.get('unit_price', 0))}")
    rd = d.get('raw_text_dump', '')
    if rd:
        print(f"  ── Raw text ──")
        for line in rd.split('\n')[:10]:
            print(f"    {line[:90]}")

# Save
with open(f"{OUT_DIR}/pipeline_results.json", "w") as f:
    json.dump({
        "image": IMG_PATH,
        "documents_found": n_docs,
        "preclassifier": {"ttf_s": round(ttf0,1), "cost_eur": round(cost0,6), "answer": n_docs},
        "bbox": {"ttf_s": round(ttf1,1), "cost_eur": round(cost1,6), "receipts": len(receipts)},
        "ocr_results": all_ocr,
        "arbiter": [{"store_name": v.store_name, "score": v.score, "passed": v.passed,
                      "issues": v.issues, "fixes": v.fixes_applied} for v in arbiter_results],
        "total_cost_eur": round(total_cost, 6),
        "total_time_s": round(total_time, 1),
    }, f, indent=2, ensure_ascii=False, default=str)

print(f"\n📁 Results: {OUT_DIR}/pipeline_results.json")
print(f"📁 Cropped: {OUT_DIR}/")
