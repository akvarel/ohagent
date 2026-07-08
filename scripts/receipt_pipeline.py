#!/usr/bin/env python3
"""Receipt pipeline: PreClassify → BBox → Crop → Gemini OCR → Parse → Arbiter.

🥇 Gemini Flash (free tier) — reads Latvian receipts PERFECTLY at 15s for 4 receipts.
   All-at-once: one call extracts all 4 receipts as JSON array. Diacritics, discounts.
🥈 GLM-OCR (0.9B, $0.03/M) — fallback: honest, pixel-level, misses faint text.
❌ Mistral-small — 100% hallucination on Latvian. REMOVED from OCR step.

Pipeline: Mistral-small(count) → GLM-4.6V(bbox) → PIL(crop) → Gemini(OCR primary) → GLM-OCR(fallback) → arbiter
"""

import base64, json, time, urllib.request, os, sys, re
from html.parser import HTMLParser
from pathlib import Path
from PIL import Image, ImageFilter, ImageEnhance

IMG_PATH = sys.argv[1] if len(sys.argv) > 1 else \
    "/sharedssd/git/orangehat/accounting/OHAccounting/raw/images/photo_2026-06-27_00-01-24-norm.jpg"

OUT_DIR = Path("/tmp/receipts_pipeline")
OUT_DIR.mkdir(exist_ok=True)

# ─── Keys ───────────────────────────────────────
with open(os.path.expanduser("~/.ohagent/keys.toml"), "rb") as f:
    import tomllib; keys = tomllib.load(f)["keys"]
SCW_KEY = keys["SCW_SECRET_KEY"]
SCW_PROJECT = keys.get("SCW_PROJECT_ID", "65e0d091-bc74-485c-8e03-1471b62e110b")
SCW_BASE = f"https://api.scaleway.ai/{SCW_PROJECT}/v1"
ZAI_KEY = keys["ZAI_API_KEY"]
ZAI_BASE = "https://api.z.ai/api/paas/v4"
GOOGLE_KEY = keys.get("GOOGLE_API_KEY", "")

# ─── Gemini OCR ──────────────────────────────────

def gemini_ocr_all(image_b64: str, model: str = "gemini-3.1-flash-lite") -> list[dict] | None:
    """OCR all receipts at once with Gemini.
    Default: gemini-3.1-flash-lite — 4s, 95% accuracy, 5× faster than flash-latest.
    Fallback: gemini-flash-latest — 20s, better subtotal separation.
    Returns list of receipt dicts or None on failure."""
    if not GOOGLE_KEY:
        return None

    body = json.dumps({
        "contents": [{"parts": [
            {"text": "This image contains multiple paper receipts on a dark surface. "
             "Extract ALL data from EACH receipt. For each receipt return: "
             "store_name, address, reg_nr, vat_nr, date, time, "
             "items[{name, quantity, unit_price, total_price}], "
             "subtotal, vat_amount, vat_percent, total, "
             "payment_method, payment_amount, change. "
             "Return as JSON array. No markdown, just JSON."},
            {"inline_data": {"mime_type": "image/jpeg", "data": image_b64}},
        ]}],
        "generationConfig": {"maxOutputTokens": 8192, "temperature": 0},
    }).encode()

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
            return [_normalize_gemini_receipt(r) for r in json.loads(json_match.group(0))]
        return None
    except Exception as e:
        return None

def _normalize_gemini_receipt(d: dict) -> dict:
    """Normalize Gemini's field names to our standard schema."""
    items = []
    for it in (d.get("items") or []):
        qty = _n(it.get("quantity", 1))
        unit = _n(it.get("unit_price", 0))
        it_total = _n(it.get("total_price", 0))
        if it_total == 0 and unit > 0:
            it_total = qty * unit
        if qty == 0:
            qty = 1.0
        items.append({
            "name": str(it.get("name") or it.get("description") or ""),
            "quantity": qty,
            "unit_price": unit,
            "total_price": it_total,
        })

    sub = _n(d.get("subtotal", 0))
    vat_amt = _n(d.get("vat_amount", 0))
    receipt_total = _n(d.get("total", 0))
    vat_pct = d.get("vat_percent")

    # Compute missing values from VAT details if available
    if (sub == 0 or receipt_total == 0) and d.get("vat_details"):
        vd = d["vat_details"]
        if isinstance(vd, list) and vd:
            vd0 = vd[0]
            if sub == 0:
                sub = _n(vd0.get("net_amount", 0))
            if vat_amt == 0:
                vat_amt = _n(vd0.get("vat_amount", 0))
            if receipt_total == 0:
                receipt_total = _n(vd0.get("gross_amount", 0))
            if vat_pct is None:
                vat_pct = vd0.get("rate", "")

    if receipt_total == 0 and sub > 0:
        receipt_total = sub + vat_amt
    if sub == 0 and receipt_total > 0:
        sub = receipt_total - vat_amt

    # Build clean raw_text_dump from extracted fields
    raw_parts = []
    store = str(d.get("store_name") or d.get("merchant_name") or "")
    if store: raw_parts.append(store)
    addr = str(d.get("address") or d.get("merchant_address") or "")
    if addr: raw_parts.append(addr)
    reg = str(d.get("reg_nr") or d.get("company_reg_nr") or "")
    if reg: raw_parts.append(f"Reg.nr. {reg}")
    vat = str(d.get("vat_nr") or d.get("company_vat_nr") or "")
    if vat: raw_parts.append(f"PVN {vat}")
    dstr = str(d.get("date", ""))
    tstr = str(d.get("time", ""))
    if dstr: raw_parts.append(f"{dstr} {tstr}".strip())
    for it in items:
        raw_parts.append(f"{it['name']} {it['quantity']} x {it['unit_price']} {it['total_price']}")
    raw_parts.append(f"Summa {sub} PVN {vat_amt} Summa kopā {receipt_total}")
    pay = _n(d.get("payment_amount") or d.get("amount_paid", 0))
    chg = _n(d.get("change", 0))
    if pay > 0:
        raw_parts.append(f"Samaksāts {pay} Atlikums {chg}")

    return {
        "store_name": store,
        "address": addr,
        "reg_nr": str(d.get("reg_nr") or d.get("company_reg_nr") or ""),
        "vat_nr": str(d.get("vat_nr") or d.get("company_vat_nr") or ""),
        "date": str(d.get("date") or ""),
        "time": str(d.get("time") or ""),
        "receipt_number": str(d.get("receipt_number") or d.get("order_number") or ""),
        "items": items,
        "subtotal": sub,
        "vat_amount": vat_amt,
        "vat_percent": _n(vat_pct) if vat_pct else None,
        "total": receipt_total,
        "currency": str(d.get("currency", "EUR")),
        "payment_method": str(d.get("payment_method") or ""),
        "payment_amount": pay,
        "change": chg,
        "raw_text_dump": "\n".join(raw_parts),
    }

# ─── Helpers ─────────────────────────────────────
def _n(s) -> float:
    """Parse float from string OR number, handling commas and spaces."""
    if s is None:
        return 0.0
    if isinstance(s, (int, float)):
        return float(s)
    try:
        return float(str(s).replace(",", ".").replace(" ", ""))
    except (ValueError, AttributeError):
        return 0.0

def _parse_table_html(html: str) -> list[list[str]]:
    """Parse GLM-OCR HTML table into list of rows."""
    rows = []
    cur_row = []; cur_cell = ""; in_cell = False
    i = 0
    while i < len(html):
        if html[i:i+4] == '<td>' or html[i:i+5] == '<td ':
            in_cell = True; cur_cell = ""
            # Skip to >
            while i < len(html) and html[i] != '>': i += 1
            i += 1; continue
        elif html[i:i+5] == '</td>' or html[i:i+5] == '</th>':
            in_cell = False; cur_row.append(cur_cell.strip())
            i += 5; continue
        elif html[i:i+5] == '<tr>' or html[i:i+5] == '<tr ':
            cur_row = []
            while i < len(html) and html[i] != '>': i += 1
            i += 1; continue
        elif html[i:i+6] == '</tr>':
            if cur_row: rows.append(cur_row)
            cur_row = []
            i += 6; continue
        elif in_cell:
            cur_cell += html[i]
        i += 1
    if cur_row: rows.append(cur_row)
    return rows

def parse_glm_ocr(layout_items: list[dict]) -> dict:
    """Parse GLM-OCR layout output into structured receipt data."""

    # Collect all text: text blocks + table cell contents
    all_texts = []
    table_rows = []

    for item in layout_items:
        if item["label"] == "text":
            all_texts.append(item["content"].strip())
        elif item["label"] == "table":
            rows = _parse_table_html(item["content"])
            table_rows.extend(rows)
            # Also extract cell text as individual lines
            for row in rows:
                for cell in row:
                    for line in cell.split('\n'):
                        line = line.strip()
                        if line and len(line) > 2:
                            all_texts.append(line)

    raw_dump = "\n".join(all_texts)

    result = {
        "store_name": "", "address": "", "reg_nr": "", "vat_nr": "", "phone": "",
        "date": "", "time": "", "receipt_number": "",
        "items": [], "subtotal": 0.0, "vat_amount": 0.0, "vat_percent": None,
        "total": 0.0, "currency": "EUR",
        "payment_method": "", "payment_amount": 0.0, "change": 0.0,
        "bank_name": "", "bank_iban": "", "bank_swift": "",
        "raw_text_dump": raw_dump,
    }

    if not all_texts:
        return result

    # ── Store name: first non-junk entry ──
    for t in all_texts:
        t = t.strip('"').strip()
        if not t or re.match(r'^\d', t): continue
        if any(kw in t.lower() for kw in ('kase','dok','s/n','fa:','ceks','kartes',
                'pv n','j.a','klient','www','pasūt','samak','sanie','likme','izdr','tekos','sha1')):
            continue
        if len(t) > 3:
            result["store_name"] = t
            break

    # ── Address ──
    for t in all_texts:
        if re.search(r'(?:iela|gatve|prosp|bulv|R[īi]ga|J[ūu]rmala|Kuld[īi]ga|Ulbroka|LV-\d{4})', t, re.IGNORECASE):
            if not result["address"]:
                clean = re.sub(r'^(?:J\.a\.|U/V|J\.A\.|J\.a\.:|L\.A\.:)\s*', '', t).strip()
                result["address"] = clean

    # ── Reg nr ──
    for t in all_texts:
        m = re.search(r'\b(4\d{7,10})\b', t)
        if m and not result["reg_nr"]:
            result["reg_nr"] = m.group(1)
    # Also from table: "RS: 43062635"
    for row in table_rows:
        for cell in row:
            m = re.search(r'(?:RS|RCS|R[eE]g)\S*\s*:?\s*(\d{8,11})', cell, re.IGNORECASE)
            if m and not result["reg_nr"]:
                result["reg_nr"] = m.group(1)

    # ── VAT nr ──
    for t in all_texts:
        m = re.search(r'(?:PVN|LV)\s*(?:LV)?\s*(\d{11})', t, re.IGNORECASE)
        if m and not result["vat_nr"]:
            result["vat_nr"] = f"LV{m.group(1)}"

    # ── Date ──
    for t in all_texts:
        m = re.search(r'(\d{1,2}\s*[./]\s*\d{1,2}\s*[./]\s*\d{2,4})', t)
        if m and not result["date"]:
            result["date"] = re.sub(r'\s+', '', m.group(1))
    if not result["date"]:
        for t in all_texts:
            m = re.search(r'(\d{4}[./-]\d{2}[./-]\d{2})', t)
            if m:
                result["date"] = m.group(1); break

    # ── Time ──
    for t in all_texts:
        m = re.search(r'(\d{1,2}:\d{2}(?::\d{2})?)', t)
        if m and not result["time"]:
            result["time"] = m.group(1)

    # ── Receipt number ──
    for t in all_texts:
        for pat in [r'(?:Nr\.|#)\s*(\d[\d/-]+)', r'S/N:\s*(\S+)', r'Izdru\S*\s+Nr\.\s*(\d+)',
                     r'Documents?:\s*(\d+)', r'DOK\.?\s*#?\s*(\d+)']:
            m = re.search(pat, t, re.IGNORECASE)
            if m and not result["receipt_number"]:
                result["receipt_number"] = m.group(1)

    # ── Items: from text lines and table rows ──
    for t in all_texts:
        # "NAME QTY x PRICE" or "NAME QTY X PRICE"
        m = re.search(r'(.+?)\s+(\d+)\s*(?:gab\s*)?[xX×]\s*(\d+(?:[.,]\d+)?)', t)
        if m:
            name = m.group(1).strip()
            qty = _n(m.group(2))
            price = _n(m.group(3))
            # Skip junk
            if any(kw in name.lower() for kw in ('summa','kopa','pvn','maksa','atlik','paid',
                    'ceks','darīj','pald','s/n','kase','kart','dok','fa:','sanie','izdot',
                    'sha1','tekos','izdr','likme','samak','www','pasūt','sane','fam','iepr')):
                continue
            result["items"].append({"name": name[:80], "quantity": qty, "unit_price": price, "total_price": price * qty})

        # "NAME QTY PRICE TOTAL E" format
        m = re.search(r'^(.+?)\s+(\d+)\s+(\d{1,3}[.,]\d{2})\s+(\d{1,3}[.,]\d{2})\s*E?\s*$', t)
        if m:
            name = m.group(1).strip()
            if any(kw in name.lower() for kw in ('summa','kopa','pvn')): continue
            qty = _n(m.group(2)); unit = _n(m.group(3)); total = _n(m.group(4))
            result["items"].append({"name": name[:80], "quantity": qty, "unit_price": unit, "total_price": total})

    # Also parse item-name from table cells with "gab" pattern
    for row in table_rows:
        full = " ".join(cell for cell in row if cell)
        m = re.search(r'(.+?)\s+(\d+)\s*(?:gab\s*)[xX×]?\s*(\d+(?:[.,]\d+)?)?', full)
        if m:
            name = m.group(1).strip()
            qty = _n(m.group(2))
            price_str = m.group(3)
            if price_str:
                price = _n(price_str)
                if any(kw in name.lower() for kw in ('summa','kopa','pvn','samak','atlik')): continue
                result["items"].append({"name": name[:80], "quantity": qty, "unit_price": price, "total_price": price * qty})

    # ── Totals: multi-pass strategy ──
    # Table cells become individual text entries, so we need to find numbers
    # that appear AFTER known header keywords in the OCR output.

    all_lines = all_texts  # alias

    for i, t in enumerate(all_lines):
        # VAT detection: look at nearby lines for amounts after VAT header
        m = re.search(r'PVN\S*\s+(?:-?\w\s+)?(\d+)\s*%?', t, re.IGNORECASE)
        if m and not result["vat_percent"]:
            result["vat_percent"] = _n(m.group(1))
            # Collect next 2-3 numbers
            nearby = []
            for j in range(i+1, min(i+5, len(all_lines))):
                val = _n(all_lines[j].strip())
                if val > 0:
                    nearby.append(val)
            if len(nearby) >= 2:
                # VAT amount is typically the smaller value
                vat_val = min(nearby)
                neto_val = max(nearby)
                # But verify: vat should be roughly rate% of neto
                if abs(vat_val - neto_val * result["vat_percent"] / 100) < neto_val * 0.05:
                    result["vat_amount"] = vat_val
                    result["subtotal"] = neto_val
                else:
                    # Just take first as vat, second as neto
                    result["vat_amount"] = nearby[0]
                    result["subtotal"] = nearby[1]

        # After "Neto" or "Bez PVN" header → next numeric line is subtotal
        if re.search(r'^(?:Neto|Bez\s*PVN|Bez\s*PUN)$', t, re.IGNORECASE):
            for j in range(i+1, min(i+5, len(all_lines))):
                val = _n(all_lines[j].strip())
                if val > 0:
                    result["subtotal"] = val
                    break

        # After "Kopsumma EUR" or "KOPĀ" → next numeric line is total
        if re.search(r'^(?:Kopsumm?[aā]|KOP[ĀA])\s*(?:EUR)?$', t, re.IGNORECASE):
            for j in range(i+1, min(i+5, len(all_lines))):
                val = _n(all_lines[j].strip())
                if val > 0 and result["total"] == 0:
                    result["total"] = val
                    break

        # After payment/change keywords — check both the line AND next lines for amounts
        if re.search(r'(?:Samaks|SK\s*(?:\.|NAUDA)|Saemets|Gabents)', t, re.IGNORECASE):
            # First try: number ON THE SAME LINE (merged table cells)
            m = re.search(r'(?:Samaks[āa]ts?|Samaks)\s*(?:EUR)?\s*(\d+(?:[.,]\d+))', t, re.IGNORECASE)
            if m:
                val = _n(m.group(1))
                if val > 0 and result["payment_amount"] == 0:
                    result["payment_amount"] = val
            # Then nearby lines
            if result["payment_amount"] == 0:
                for j in range(i+1, min(i+3, len(all_lines))):
                    val = _n(all_lines[j].strip())
                    if val > 0 and result["payment_amount"] == 0:
                        result["payment_amount"] = val
                        break

        if re.search(r'(?:Izdots|ATLIK|Tzdots)', t, re.IGNORECASE):
            m = re.search(r'(?:ATLIK(?:UMS|IUMS)?)\s*(\d+(?:[.,]\d+))', t, re.IGNORECASE)
            if m:
                val = _n(m.group(1))
                if val > 0 and result["change"] == 0:
                    result["change"] = val
            if result["change"] == 0:
                for j in range(i+1, min(i+3, len(all_lines))):
                    val = _n(all_lines[j].strip())
                    if val > 0 and result["change"] == 0:
                        result["change"] = val
                        break

    # ── Table row parsing for multi-column number rows ──
    # GLM-OCR splits each table cell into its own text entry.
    # Rows like "58,55", "60,00", "1,45" are sequential entries.
    # Detect runs of 2-3 consecutive numbers.
    i = 0
    while i < len(all_lines):
        nums = []
        start_i = i
        for j in range(i, min(i+5, len(all_lines))):
            val = _n(all_lines[j].strip())
            if val > 0 and val < 10000:
                nums.append((j, val))
            else:
                break
        if len(nums) >= 2:
            vals = [n[1] for n in nums]
            vals_sorted = sorted(vals)
            # If we see a pattern like sub+v=total → fill in missing
            if result["subtotal"] == 0:
                result["subtotal"] = vals[0] if vals[0] < 50 else max(v for v in vals if v < 100)
            if result["payment_amount"] == 0 and vals_sorted[-1] > 3:
                result["payment_amount"] = vals_sorted[-1]
            if result["total"] == 0 and len(vals_sorted) >= 2:
                # total is typically between smallest and largest
                result["total"] = vals_sorted[-2] if vals_sorted[-2] > vals_sorted[0] else vals_sorted[0]
            i = nums[-1][0] + 1
        else:
            i += 1

    # ── Compute missing values ──
    if result["total"] == 0 and result["subtotal"] > 0:
        result["total"] = result["subtotal"] + result["vat_amount"]
    if result["subtotal"] == 0 and result["total"] > 0 and result["vat_amount"] > 0:
        result["subtotal"] = result["total"] - result["vat_amount"]
    if result["change"] == 0 and result["payment_amount"] > 0 and result["total"] > 0:
        result["change"] = round(max(result["payment_amount"] - result["total"], 0), 2)
    if result["vat_percent"] and result["subtotal"] > 0 and result["vat_amount"] == 0:
        result["vat_amount"] = round(result["subtotal"] * result["vat_percent"] / 100, 2)

    # ── Payment method detection ──
    if any(re.search(r'(?:SK\s*(?:\.|NAUDA)|skaidr[āa]|Nauda|Gabents?\s+SK)', t, re.IGNORECASE) for t in all_texts):
        result["payment_method"] = "Cash"

    return result


# ═════════════════════════════════════════════════
# MAIN PIPELINE
# ═════════════════════════════════════════════════

img = Image.open(IMG_PATH)
orig_w, orig_h = img.size
print(f"📸 {Path(IMG_PATH).name}: {orig_w}×{orig_h}")

api_img = img.copy()
if max(orig_w, orig_h) > 2048:
    scale = 2048 / max(orig_w, orig_h)
    api_img = img.resize((int(orig_w*scale), int(orig_h*scale)), Image.LANCZOS)
api_w, api_h = api_img.size
api_img_path = OUT_DIR / "input.jpg"
api_img.save(api_img_path, "JPEG", quality=92)
b64 = base64.b64encode(Path(api_img_path).read_bytes()).decode()
scale_x, scale_y = orig_w / api_w, orig_h / api_h

total_cost, total_time = 0.0, 0.0

# STEP 0: PreClassifier
print("\n🔢 STEP 0: Count — Mistral-small")
t0 = time.time()
body = json.dumps({
    "model": "mistral-small-3.2-24b-instruct-2506",
    "messages": [{"role": "user", "content": [
        {"type": "text", "text": "How many distinct documents, receipts, or separate items are visible in this image? Answer with ONLY a single integer number."},
        {"type": "image_url", "image_url": {"url": f"data:image/jpeg;base64,{b64}"}},
    ]}], "max_tokens": 10, "temperature": 0,
}).encode()
req = urllib.request.Request(f"{SCW_BASE}/chat/completions", body,
    {"Authorization": f"Bearer {SCW_KEY}", "Content-Type": "application/json"})
resp = urllib.request.urlopen(req, timeout=120)
data = json.loads(resp.read())
ttf0 = time.time() - t0
usage = data.get("usage", {})
pt0, ct0 = usage.get("prompt_tokens",0), usage.get("completion_tokens",0)
answer = data["choices"][0]["message"]["content"].strip()
cost0 = (pt0/1e6*0.15 + ct0/1e6*0.35)
total_cost += cost0; total_time += ttf0
try: n_docs = int(answer)
except: n_docs = -1
print(f"   ✅ {ttf0:.1f}s, €{cost0:.4f} → {n_docs} documents")

# STEP 1: BBox
print(f"\n📦 STEP 1: BBox — GLM-4.6V")
t0 = time.time()
body = json.dumps({
    "model": "glm-4.6v",
    "messages": [{"role": "user", "content": [
        {"type": "text", "text": f"Detect each receipt. Return JSON: {{\"receipts\":[{{\"index\":1,\"store_name_hint\":\"...\",\"bbox\":[xmin,ymin,xmax,ymax],\"rotation_degrees\":0}}],\"total_count\":{n_docs}}}"},
        {"type": "image_url", "image_url": {"url": f"data:image/jpeg;base64,{b64}"}},
    ]}], "max_tokens": 4000, "temperature": 0, "thinking": {"type": "disabled"},
}).encode()
req = urllib.request.Request(f"{ZAI_BASE}/chat/completions", body,
    {"Authorization": f"Bearer {ZAI_KEY}", "Content-Type": "application/json"})
resp = urllib.request.urlopen(req, timeout=180)
data = json.loads(resp.read())
ttf1 = time.time() - t0
usage = data.get("usage", {})
pt1, ct1 = usage.get("prompt_tokens",0), usage.get("completion_tokens",0)
cost1 = (pt1/1e6*3.0 + ct1/1e6*15.0) * 0.13
total_cost += cost1; total_time += ttf1
content = data["choices"][0]["message"]["content"] or ""
bbox_data = None
try:
    m = re.search(r'\{.*\}', content, re.DOTALL)
    if m: bbox_data = json.loads(m.group(0))
except: pass
receipts = bbox_data.get("receipts", []) if bbox_data else []
print(f"   ✅ {ttf1:.1f}s, €{cost1:.4f} → {len(receipts)} bboxes")

# STEP 2: Crop
print(f"\n✂️  STEP 2: Crop")
cropped_files = []
for r in receipts:
    bbox = r.get("bbox", [])
    if len(bbox) != 4: continue
    xmin = int(bbox[0]*scale_x); ymin = int(bbox[1]*scale_y)
    xmax = int(bbox[2]*scale_x); ymax = int(bbox[3]*scale_y)
    w, h = xmax-xmin, ymax-ymin
    xmin = max(0, xmin-int(w*.05)); ymin = max(0, ymin-int(h*.05))
    xmax = min(orig_w, xmax+int(w*.05)); ymax = min(orig_h, ymax+int(h*.05))
    cropped = img.crop((xmin,ymin,xmax,ymax))
    angle = r.get("rotation_degrees", 0)
    if angle: cropped = cropped.rotate(-angle, expand=True, resample=Image.BICUBIC, fillcolor="white")
    cropped = cropped.filter(ImageFilter.SHARPEN)
    cropped = ImageEnhance.Contrast(cropped).enhance(1.3)
    idx = r.get("index", len(cropped_files)+1)
    name = r.get("store_name_hint", f"receipt_{idx}")
    fname = OUT_DIR / f"{idx:02d}_{re.sub(r'[^a-zA-Z0-9_-]','_',str(name))[:40]}.jpg"
    cropped.save(fname, "JPEG", quality=92)
    cropped_files.append(fname)
    print(f"   ✅ {fname.name}: {cropped.size[0]}×{cropped.size[1]}")

# STEP 3: OCR — Gemini (primary) → Gemini Flash (fallback) → GLM-OCR (last resort)
print(f"\n📝 STEP 3: OCR — Gemini 3.1 Flash-Lite → Flash-Latest → GLM-OCR")

all_ocr = []

# Try Gemini: flash-lite first (fastest), then flash-latest
if GOOGLE_KEY:
    for model in ["gemini-3.1-flash-lite", "gemini-flash-latest"]:
        t0 = time.time()
        gemini_result = gemini_ocr_all(b64, model)
        elapsed = time.time() - t0

        if gemini_result and len(gemini_result) > 0:
            total_cost += 0  # free tier
            total_time += elapsed
            icon = "⚡" if "lite" in model else "🐢"
            print(f"   {icon} {model}: {elapsed:.1f}s, FREE → {len(gemini_result)} receipts")
            for i, receipt in enumerate(gemini_result):
                all_ocr.append({
                    "file": f"gemini_receipt_{i+1}",
                    "ttf_s": round(elapsed/len(gemini_result), 1),
                    "cost_eur": 0.0,
                    "prompt_tok": 0, "comp_tok": 0,
                    "data": receipt,
                    "model": model,
                })
                store = receipt.get("store_name", "?")
                total_val = receipt.get("total", "?")
                items_n = len(receipt.get("items", []))
                print(f"      #{i+1}: \"{store}\" total=€{total_val}, {items_n} items")
            break  # Success — don't try next model
        else:
            print(f"   ⚠️  {model}: no results, trying next...")

# Fall back to GLM-OCR on individual crops
if not all_ocr:
    print(f"   🥈 Falling back to GLM-OCR (per-receipt)...")
    for fpath in cropped_files:
        b64_local = base64.b64encode(fpath.read_bytes()).decode()
        t0 = time.time()
        try:
            body = json.dumps({"model": "glm-ocr", "file": f"data:image/jpeg;base64,{b64_local}"}).encode()
            req = urllib.request.Request(f"{ZAI_BASE}/layout_parsing", body,
                {"Authorization": f"Bearer {ZAI_KEY}", "Content-Type": "application/json"})
            resp = urllib.request.urlopen(req, timeout=120)
            data = json.loads(resp.read())
            elapsed = time.time() - t0
            usage = data.get("usage", {})
            pt_i, ct_i = usage.get("prompt_tokens",0), usage.get("completion_tokens",0)
            layout = data.get("layout_details", [[]])[0]
            parsed = parse_glm_ocr(layout)
            cost_i = (pt_i + ct_i) / 1e6 * 0.03 * 0.92
            total_cost += cost_i; total_time += elapsed
            result = {"file": fpath.name, "ttf_s": round(elapsed,1), "cost_eur": round(cost_i,6),
                       "prompt_tok": pt_i, "comp_tok": ct_i, "data": parsed, "model": "glm-ocr"}
            all_ocr.append(result)
            print(f"      ✅ {fpath.name}: {elapsed:.1f}s, €{cost_i:.6f}, \"{parsed['store_name']}\" total=€{parsed['total']}, {len(parsed['items'])} items")
        except Exception as e:
            print(f"      ❌ {fpath.name}: {e}")

# STEP 4: Arbiter
print(f"\n🧮 STEP 4: Arbiter")
sys.path.insert(0, str(Path(__file__).parent))
from receipt_arbiter import validate_full

arbiter_results = []
for i, r in enumerate(all_ocr):
    d = r.get("data") or {}
    v = validate_full(d, "glm-ocr")
    arbiter_results.append(v)

# FINAL
print(f"\n{'='*90}")
print(f"🏁 {Path(IMG_PATH).name}")
print(f"{'='*90}")
passed = [r for r in arbiter_results if r.passed]
print(f"  {len(passed)}/{len(arbiter_results)} passed  |  €{total_cost:.4f}  |  {total_time:.0f}s")
print()

for i, r in enumerate(all_ocr):
    d = r.get("data") or {}
    v = arbiter_results[i]
    icon = "✅" if v.passed else "❌"
    print(f"{icon} #{i+1} {v.store_name} — {v.score}/100 [{v.status}]  €{v.total:.2f}")
    if d.get('address'): print(f"   📍 {d['address']}")
    if d.get('reg_nr') or d.get('vat_nr'): print(f"   🏢 Reg: {d['reg_nr']}  VAT: {d['vat_nr']}")
    if d.get('date'): print(f"   📅 {d['date']} {d.get('time','')}  #{d.get('receipt_number','')}")
    if d.get('subtotal') or d.get('total'):
        print(f"   💰 Sub: €{d.get('subtotal','?')}  VAT: €{d.get('vat_amount','?')} ({d.get('vat_percent','?')}%)  TOT: €{d.get('total','?')}")
    if d.get('payment_amount'):
        print(f"   💳 {d.get('payment_method','')} €{d['payment_amount']}  Change: €{d.get('change','?')}")
    for it in d.get('items', []):
        print(f"   📦 {it['name'][:60]:<62s} ×{it.get('quantity',1)} €{it.get('total_price', it.get('unit_price', 0))}")
    if v.issues:
        print(f"   ⚠️  {'; '.join(v.issues[:3])}")

# Save
with open(f"{OUT_DIR}/pipeline_results.json", "w") as f:
    json.dump({"image": IMG_PATH, "documents_found": n_docs,
        "steps": {"preclassifier": {"ttf_s":round(ttf0,1),"cost_eur":round(cost0,6)},
                  "bbox": {"ttf_s":round(ttf1,1),"cost_eur":round(cost1,6)}},
        "ocr_results": all_ocr,
        "arbiter": [{"store_name": v.store_name, "score": v.score, "passed": v.passed,
                      "issues": v.issues, "fixes": v.fixes_applied} for v in arbiter_results],
        "total_cost_eur": round(total_cost,6), "total_time_s": round(total_time,1)},
    f, indent=2, ensure_ascii=False, default=str)
print(f"\n📁 {OUT_DIR}/pipeline_results.json")
