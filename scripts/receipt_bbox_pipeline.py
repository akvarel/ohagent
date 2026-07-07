"""Receipt BBox Pipeline: Detect → Crop → Rotate → Individual OCR.

Step 1: GLM-4.6V detects bounding boxes for each receipt + rotation angle
Step 2: PIL crops and rotates each receipt
Step 3: Each cropped receipt → Mistral-small OCR (cheap, fast)
"""

import base64, json, time, urllib.request, os, sys, re
from pathlib import Path
from PIL import Image, ImageFilter, ImageEnhance

# ─── Config ─────────────────────────────────────
with open(os.path.expanduser("~/.ohagent/keys.toml"), "rb") as f:
    import tomllib
    keys = tomllib.load(f)["keys"]

ZAI_KEY = keys["ZAI_API_KEY"]
ZAI_BASE = "https://api.z.ai/api/paas/v4"
SCW_KEY = keys["SCW_SECRET_KEY"]
SCW_PROJECT = keys.get("SCW_PROJECT_ID", "65e0d091-bc74-485c-8e03-1471b62e110b")
SCW_BASE = f"https://api.scaleway.ai/{SCW_PROJECT}/v1"

SRC_IMG = "/sharedssd/git/orangehat/accounting/OHAccounting/raw/images/photo_2026-06-27_00-01-24.jpg"
OUT_DIR = Path("/tmp/receipts_cropped")
OUT_DIR.mkdir(exist_ok=True)

# Use the ORIGINAL full-resolution image for bbox detection
img = Image.open(SRC_IMG)
orig_w, orig_h = img.size
print(f"📸 Original image: {orig_w}×{orig_h} = {orig_w*orig_h/1e6:.1f}MP")

# Save a max-2048 version for API call (GLM-4.6V handles up to ~50MP but keep reasonable)
if max(orig_w, orig_h) > 2048:
    scale = 2048 / max(orig_w, orig_h)
    api_img = img.resize((int(orig_w*scale), int(orig_h*scale)), Image.LANCZOS)
else:
    api_img = img.copy()

# Save as high-quality JPEG
api_img_path = OUT_DIR / "input.jpg"
api_img.save(api_img_path, "JPEG", quality=92)
api_w, api_h = api_img.size
print(f"📸 API image: {api_w}×{api_h}")
b64 = base64.b64encode(Path(api_img_path).read_bytes()).decode()

# Scale factor: API image coords → original image coords
scale_x = orig_w / api_w
scale_y = orig_h / api_h

# ═══════════════════════════════════════════════════════════════
# STEP 1: Bounding box detection via GLM-4.6V
# ═══════════════════════════════════════════════════════════════
bbox_prompt = """This image contains multiple paper receipts/documents placed on a dark surface.
Detect EACH distinct receipt and return its bounding box AND the rotation angle needed to make the text upright.

Coordinates must be in [[xmin, ymin, xmax, ymax]] format relative to the full image.
Angles in degrees (positive = clockwise rotation needed).

Return valid JSON only:
{
  "receipts": [
    {
      "index": 1,
      "store_name_hint": "store name if visible",
      "bbox": [xmin, ymin, xmax, ymax],
      "rotation_degrees": 0
    }
  ],
  "total_count": 4
}"""

print("\n🔍 STEP 1: Detecting bounding boxes with GLM-4.6V...")
t0 = time.time()

body = json.dumps({
    "model": "glm-4.6v",
    "messages": [{"role": "user", "content": [
        {"type": "text", "text": bbox_prompt},
        {"type": "image_url", "image_url": {"url": f"data:image/jpeg;base64,{b64}"}}
    ]}],
    "max_tokens": 4000,
    "temperature": 0,
    "thinking": {"type": "disabled"},
}).encode()

req = urllib.request.Request(f"{ZAI_BASE}/chat/completions", body,
    {"Authorization": f"Bearer {ZAI_KEY}", "Content-Type": "application/json"})

resp = urllib.request.urlopen(req, timeout=180)
data = json.loads(resp.read())
ttf1 = time.time() - t0
content = data["choices"][0]["message"]["content"] or ""
usage = data.get("usage", {})
pt, ct = usage.get("prompt_tokens", 0), usage.get("completion_tokens", 0)

# Parse bbox JSON
bbox_data = None
try:
    m = re.search(r'\{.*\}', content, re.DOTALL)
    if m:
        bbox_data = json.loads(m.group(0))
except: pass

print(f"   ✅ {ttf1:.1f}s, {pt}+{ct} tok, finish={data['choices'][0].get('finish_reason','?')}")
print(f"   Raw response: {content[:300]}...")

if not bbox_data or "receipts" not in bbox_data:
    print("❌ Failed to parse bbox data from GLM-4.6V")
    print(f"   Full response: {content}")
    sys.exit(1)

receipts = bbox_data.get("receipts", [])
print(f"   Found {len(receipts)} receipts: {[(r.get('store_name_hint','?'), r.get('bbox')) for r in receipts]}")

# ═══════════════════════════════════════════════════════════════
# STEP 2: Crop + rotate each receipt
# ═══════════════════════════════════════════════════════════════
print(f"\n✂️  STEP 2: Cropping + rotating receipts...")

cropped_files = []

for r in receipts:
    bbox = r.get("bbox", [])
    if len(bbox) != 4:
        print(f"   ⚠️  Receipt #{r.get('index','?')}: invalid bbox {bbox}")
        continue

    # API coords → original image coords
    xmin = int(bbox[0] * scale_x)
    ymin = int(bbox[1] * scale_y)
    xmax = int(bbox[2] * scale_x)
    ymax = int(bbox[3] * scale_y)

    # Add 5% padding
    w = xmax - xmin
    h = ymax - ymin
    pad_x = int(w * 0.05)
    pad_y = int(h * 0.05)
    xmin = max(0, xmin - pad_x)
    ymin = max(0, ymin - pad_y)
    xmax = min(orig_w, xmax + pad_x)
    ymax = min(orig_h, ymax + pad_y)

    # Crop from original
    cropped = img.crop((xmin, ymin, xmax, ymax))

    # Rotate to make text upright
    angle = r.get("rotation_degrees", 0)
    if angle != 0:
        cropped = cropped.rotate(-angle, expand=True, resample=Image.BICUBIC,
                                  fillcolor="white")

    # Enhance for better OCR: sharpen + contrast
    cropped = cropped.filter(ImageFilter.SHARPEN)
    enhancer = ImageEnhance.Contrast(cropped)
    cropped = enhancer.enhance(1.3)

    idx = r.get("index", len(cropped_files) + 1)
    name = r.get("store_name_hint", f"receipt_{idx}")
    safe_name = re.sub(r'[^a-zA-Z0-9_-]', '_', str(name))[:50]

    fname = OUT_DIR / f"{idx:02d}_{safe_name}.jpg"
    cropped.save(fname, "JPEG", quality=92)
    cropped_files.append(fname)
    print(f"   ✅ {fname.name}: {cropped.size[0]}×{cropped.size[1]}, bbox=[{xmin},{ymin},{xmax},{ymax}], rotate={angle}°")

print(f"\n📦 {len(cropped_files)} receipts cropped to {OUT_DIR}/")

# ═══════════════════════════════════════════════════════════════
# STEP 3: OCR each cropped receipt with Mistral-small
# ═══════════════════════════════════════════════════════════════
print(f"\n🔍 STEP 3: OCR each receipt with Scaleway Mistral-small...")

ocr_prompt = """You are a precise OCR system for Latvian/EU receipts.
Extract ALL visible text and numbers from this receipt.
Be exhaustive — every line, every number.
If text is unreadable, leave that field empty or mark with "?".
NEVER invent data — only what is visibly printed.

Return valid JSON:
{
  "store_name": "",
  "address": "",
  "reg_nr": "",
  "vat_nr": "",
  "date": "", "time": "", "receipt_number": "",
  "items": [{"name": "", "quantity": 0.0, "unit_price": 0.0, "total_price": 0.0}],
  "subtotal": 0.0, "vat_amount": 0.0, "vat_percent": null,
  "total": 0.0, "currency": "EUR",
  "payment_method": "", "payment_amount": 0.0, "change": 0.0,
  "bank_name": "", "bank_iban": "", "bank_swift": "",
  "raw_text_dump": ""
}"""

all_ocr = []
total_cost = 0

for i, fpath in enumerate(cropped_files):
    b64_local = base64.b64encode(Path(fpath).read_bytes()).decode()

    body = json.dumps({
        "model": "mistral-small-3.2-24b-instruct-2506",
        "messages": [
            {"role": "system", "content": ocr_prompt},
            {"role": "user", "content": [
                {"type": "text", "text": "Extract all data from this receipt image."},
                {"type": "image_url", "image_url": {"url": f"data:image/jpeg;base64,{b64_local}"}},
            ]},
        ],
        "max_tokens": 4096,
        "temperature": 0,
        "response_format": {"type": "json_object"},
    }).encode()

    t0 = time.time()
    req = urllib.request.Request(f"{SCW_BASE}/chat/completions", body,
        {"Authorization": f"Bearer {SCW_KEY}", "Content-Type": "application/json"})

    try:
        resp = urllib.request.urlopen(req, timeout=180)
        data = json.loads(resp.read())
        elapsed = time.time() - t0
        c = data["choices"][0]["message"]["content"] or ""
        u = data.get("usage", {})
        pt_i, ct_i = u.get("prompt_tokens", 0), u.get("completion_tokens", 0)

        # Parse JSON
        parsed = None
        try:
            parsed = json.loads(c)
        except:
            m = re.search(r'\{.*\}', c, re.DOTALL)
            if m:
                try: parsed = json.loads(m.group(0))
                except: pass

        cost_i = (pt_i / 1e6 * 0.15) + (ct_i / 1e6 * 0.35)
        total_cost += cost_i

        result = {
            "file": str(fpath.name),
            "bbox": receipts[i].get("bbox") if i < len(receipts) else None,
            "ttf_s": round(elapsed, 1),
            "prompt_tok": pt_i,
            "comp_tok": ct_i,
            "cost_eur": round(cost_i, 6),
            "data": parsed,
            "raw_text": c[:500],
        }
        all_ocr.append(result)

        store = parsed.get("store_name", "?") if parsed else "?"
        total = parsed.get("total", "?") if parsed else "?"
        items = len(parsed.get("items", [])) if parsed else 0
        print(f"   ✅ {fpath.name}: {elapsed:.1f}s, {pt_i}+{ct_i} tok, €{cost_i:.4f}, "
              f"\"{store}\" total=€{total}, {items} items")

    except Exception as e:
        print(f"   ❌ {fpath.name}: {e}")

# ═══════════════════════════════════════════════════════════════
# FINAL SUMMARY
# ═══════════════════════════════════════════════════════════════
print(f"\n{'='*80}")
print(f"🏁 PIPELINE COMPLETE")
print(f"{'='*80}")
print(f"  Step 1 (GLM-4.6V bbox):    {ttf1:.1f}s, ¥{(3*pt/1e6 + 15*ct/1e6)*0.13:.4f}")
print(f"  Step 2 (Crop):              {len(cropped_files)} receipts")
print(f"  Step 3 (OCR ×{len(all_ocr)}):    €{total_cost:.4f}")
print(f"  Total pipeline cost:        €{total_cost + (3*pt/1e6 + 15*ct/1e6)*0.13:.4f}")
print(f"  Total time:                 ~{ttf1 + sum(r['ttf_s'] for r in all_ocr):.0f}s")

# Save full results
with open(f"{OUT_DIR}/ocr_results.json", "w") as f:
    json.dump({
        "bbox_step": {"ttf_s": ttf1, "prompt_tok": pt, "comp_tok": ct, "data": bbox_data},
        "crop_step": [str(f.name) for f in cropped_files],
        "ocr_step": all_ocr,
        "total_cost_eur": total_cost,
    }, f, indent=2, ensure_ascii=False, default=str)

print(f"\n📄 Full results: {OUT_DIR}/ocr_results.json")
print(f"📁 Cropped receipts: {OUT_DIR}/")

# Print clean summary for each receipt
print(f"\n{'='*80}")
print("📋 EXTRACTED RECEIPTS")
print(f"{'='*80}")
for r in all_ocr:
    d = r.get("data") or {}
    print(f"\n{'─'*60}")
    print(f"📄 {r['file']}")
    print(f"   Store:    {d.get('store_name', '???')}")
    print(f"   Reg nr:   {d.get('reg_nr', '')}")
    print(f"   VAT nr:   {d.get('vat_nr', '')}")
    print(f"   Address:  {d.get('address', '')}")
    print(f"   Date:     {d.get('date', '')} {d.get('time', '')}")
    print(f"   Bank:     {d.get('bank_name', '')} | IBAN: {d.get('bank_iban', '')}")
    print(f"   Total:    €{d.get('total', '?')} | VAT: €{d.get('vat_amount', '?')} | Subtotal: €{d.get('subtotal', '?')}")
    print(f"   Payment:  {d.get('payment_method', '')} €{d.get('payment_amount', '')} | Change: €{d.get('change', '')}")
    items = d.get('items', [])
    print(f"   Items ({len(items)}):")
    for item in items[:15]:
        print(f"     - {item.get('name','?')[:50]} ×{item.get('quantity',1)} €{item.get('total_price', item.get('unit_price', 0))}")
    if len(items) > 15:
        print(f"     ... and {len(items)-15} more items")
