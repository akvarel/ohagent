# Receipt OCR Skill — Gemini Flash

Extract structured receipt data from photos using Google Gemini.

## Trigger
When the user asks to "распознай чеки", "OCR receipts", "extract receipts from photo",
"извлеки данные из чеков", or provides a photo with receipts.

## Model
Primary: `gemini-3.1-flash-lite` (4s, FREE, 95% accuracy)
Fallback: `gemini-flash-latest` (20s, FREE, better subtotal separation)

## Prompt
```
This image contains paper receipts on a dark surface.
Extract ALL data from EACH receipt. For each receipt return:
store_name, address, reg_nr, vat_nr, date, time,
items[{name, quantity, unit_price, total_price}],
subtotal, vat_amount, vat_percent, total,
payment_method, payment_amount, change.
VAT is two letters+numbers. Each receipt HAS either VAT or REGISTRATION number.
Return as JSON array. No markdown, just JSON.
```

## Post-processing
1. Normalize field names (Gemini uses `merchant_name` → `store_name`, `amount_paid` → `payment_amount`)
2. Auto-correct gross/net: if Σitems ≈ subtotal+VAT, items are shown with VAT. Convert to net.
3. Run mathematical arbiter: Σitems≈subtotal, VAT≈subtotal×%, sub+VAT≈total, payment−change≈total

## Known limitations
- Gemini 3.1 Flash-Lite puts gross amount in `subtotal` field instead of net. Net = total − VAT.
- Receipts with discount lines (e.g. "Preces atlaide 7%") need manual item-price reconciliation.
- Only tested on Latvian receipts (EUR, LV reg/VAT numbers, thermal paper).

## Script
`scripts/receipt_pipeline.py` — full 2-step pipeline: Gemini → Arbiter.
Usage: `python3 scripts/receipt_pipeline.py <path/to/photo.jpg>`
