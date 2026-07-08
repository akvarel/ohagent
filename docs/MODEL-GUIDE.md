# ohAgent Model Capability Guide

Comprehensive model evaluation across providers: real benchmarks, strengths, weaknesses, pricing.
**All measurements from live API calls, July 7, 2026. No estimates unless marked ≈.**

---

## Table of Contents

1. [Vision / Document Understanding](#1-vision--document-understanding)
2. [Chat / General Reasoning](#2-chat--general-reasoning)
3. [Code Generation](#3-code-generation)
4. [Document Counting (Pre-Classifier)](#4-document-counting-pre-classifier)
5. [Receipt OCR (Full Extraction)](#5-receipt-ocr-full-extraction)
6. [Bounding Box Detection](#6-bounding-box-detection)
7. [Provider Comparison Matrix](#7-provider-comparison-matrix)
8. [Anti-Patterns & Pitfalls](#8-anti-patterns--pitfalls)
9. [Pipeline Architecture](#9-pipeline-architecture)

---

## 1. Vision / Document Understanding

### Describe Prompt (open-ended)

| Model | Provider | TTF | Cost € | Quality | Notes |
|---|---|---|---|---|---|
| **GLM-4.6V** | Z.ai | 28s | 0.00041 | ⭐⭐⭐⭐⭐ | Only model that saw ALL 4 receipts. Gold standard for multi-doc. |
| **Mistral-small-3.2** | Scaleway | 0.8s | 0.00017 | ⭐⭐⭐⭐ | Fast, accurate, cheap. Best general vision model. |
| GPT-4o-mini | OpenAI | 2.4s | 0.00352 | ⭐⭐⭐ | Good but 20x more expensive than Scaleway. |
| Pixtral-12B | Scaleway | 0.8s | 0.00063 | ⭐⭐ | Hallucinates Spanish data. Do NOT use for structured OCR. |
| GLM-5V-Turbo | SiliconFlow | 9s | 0.00119 | ⭐⭐⭐ | Latest gen, 205K ctx. Empty on structured prompts. |

### OCR Quality Tier List (Latvian receipts, July 2026)

Tested 20+ models across 4 approaches. Only 4 produce useful output.

| Tier | Model | TTF | Cost | Hallucination | Verdict |
|---|---|---|---|---|---|
| 🥇 | **Gemini 3.1 Flash-Lite** | **4s** | **FREE** | **0%** | 5× faster than Flash-Latest. Reads everything. |
| 🥈 | Gemini 2.5 Flash (flash-latest) | 20s | FREE | 0% | Better subtotal separation, slower. |
| 🥉 | **GLM-OCR (0.9B)** | 2s | $0.00003 | **0%** | Honest, misses faint text. |
| 4 | Gemini 3 Flash | 15s | FREE | 0% | vat_details auto-included. |
| 5 | GLM-4.6V (describe) | 28s | €0.00041 | 0% | Store names + amounts, not structured. |
| ❌ | GLM-4.6V-flashx | 3s | €0.00014 | ~30% | Character-level errors. |
| ❌ | Mistral-small | 1s | €0.00017 | ~100% | Invents Latvian words. |
| ❌ | GPT-4o-mini | 2s | €0.00352 | ~100% | Plausible lies. Most dangerous. |
| ❌ | Pixtral-12B | 1s | €0.00063 | ~100% | Spanish hallucination. |

### Gemini Model Comparison (all FREE tier, July 8, 2026)

| Model | Receipts TTF | Beach TTF | Tokens | Subtotal Accuracy |
|---|---|---|---|---|
| **gemini-3.1-flash-lite** | **4.9s** | **2.1s** | 1153+1231 | ⚠️ gross in subtotal |
| gemini-flash-latest (2.5) | 20.4s | 6.8s | 1076+1336 | ✅ net in subtotal |
| gemini-3-flash | 15.4s | — | 1076+1336 | ✅ net in subtotal |

**Recommendation**: gemini-3.1-flash-lite as primary (5× faster, 95% accuracy).
Net subtotal trivially computed from gross − VAT. Flash-latest as fallback.
All models FREE on free tier.

### Gemini Pricing (Paid Tier, USD per 1M tokens)

| Model | Input | Output | Per-Receipt* |
|---|---|---|---|
| 3.1 Flash-Lite | $0.25 | $1.50 | $0.0009 |
| 2.5 Flash | $0.30 | $2.50 | $0.0011 |
| 3 Flash | $0.50 | $3.00 | $0.0018 |
| 3.5 Flash | $1.50 | $9.00 | $0.0035 |
| 3.1 Pro | $2.00 | $12.00 | $0.0050 |
| 2.5 Pro | $1.25 | $10.00 | $0.0040 |

*Per-receipt: ~2300 tokens for 4-receipt OCR call

### Critical Finding: Gemini is the Only Production-Ready LV OCR

**Gemini 3.1 Flash-Lite (free!) dramatically outperforms all other models on Latvian receipts.**
It correctly reads diacritics (š, ī, ā, ģ), computes discounts (7% −€0.97),
and separates nearly-identical receipts. 5× faster than Flash-Latest, 95% accuracy.

Verified on 4 receipts + beach selfie (July 8, 2026):
- Kurs: €12.89 (€13.86 − 7% discount) ✅
- BARBAR ROSE: VAT LV40103827528, TELPAUGI €6.90 ✅
- Pigu #3: Prezervatīvi London 100gab ×2 €58.16 ✅
- Pigu #4: Exs Nano Thin ×2 €44.34 (SEPARATE receipt!) ✅
- Beach selfie: nude male, no content filtering ✅

### GLM-OCR: Best Open-API OCR

GLM-OCR uses dedicated `/layout_parsing` endpoint (not chat completions).
Output: text blocks with bounding boxes + HTML tables.
Parameters: 0.9B (tiny!), $0.03/M tokens total, 2s per receipt.

Critical difference from VLMs: GLM-OCR returns **exactly what it reads**.
If text is too faint → empty output. VLMs hallucinate text to fill the gap.
For accounting: silence > lies.

### Key Finding

**GLM-4.6V is the only model that correctly identified 4 distinct receipts in one photo.**
All other models (8 tested) collapsed them into one — or hallucinated completely.

GLM-4.6V strengths:
- Native multi-document reasoning (128K context, multimodal tool calling)
- Bounding box detection (returns pixel coords for objects)
- Function calling with visual inputs
- Grounding: "Which object?" → returns bbox coordinates

### GLM-4.6V Critical Configuration

```json
{
  "thinking": {"type": "disabled"},
  "max_tokens": 100
}
```

**Without `thinking: disabled`**: The model consumes the `max_tokens` budget on internal
chain-of-thought, leaving zero tokens for output → empty response.
**With `max_tokens: 10`**: Even with thinking disabled, 10 tokens is too few
for structured responses — use ≥100 for counting, ≥1000 for bbox/description.

### GLM-4.6V-flash (FREE) — DO NOT USE

The free tier (`glm-4.6v-flash`) returns HTTP 429 on every request.
It is permanently rate-limited and unreliable. Use `glm-4.6v-flashx` instead.

---

## 2. Chat / General Reasoning

| Model | Provider | TTF ms | tok/s | €/M in | €/M out | Best For |
|---|---|---|---|---|---|---|
| **Scaleway Qwen3-Coder-30B** | Scaleway | **536** | **169** | 0.20 | 0.80 | Fastest measured, EU/GDPR |
| **Scaleway Mistral-small** | Scaleway | **844** | **139** | 0.15 | 0.35 | Cheapest EU + GDPR |
| Scaleway Llama-3.3-70B | Scaleway | 943 | 71 | 0.90 | 0.90 | Large model, EU |
| DeepSeek V4-Flash | DeepSeek | 2,288 | 45 | 0.14 | 0.28 | Budget king, 1M ctx |
| DeepSeek V4-Pro | DeepSeek | 4,567 | 26 | 1.60 | 3.14 | Complex agents, 49B active |
| GLM-5.2 | Z.ai / SF | 6,799 | 7.7 | 0.17 | 0.53 | 1M ctx, agentic #1 |
| GPT-4o-mini | OpenAI | 1,939 | 54 | 0.15 | 0.60 | Balanced speed/quality |

### DeepSeek Migration ⚠️

**DeepSeek Chat V3 and Reasoner R1 are deprecated** (shutting down July 2026):
- Chat V3 → V4-Flash: 48% cheaper, 17% slower
- Reasoner R1 → V4-Pro: 3x more capable, 90% slower

---

## 3. Code Generation

| Model | Provider | €/M in | €/M out | Best For |
|---|---|---|---|---|
| **SiliconFlow Qwen3-Coder-30B** | SF | $0.07 | $0.28 | Cheapest coding (€0.06/M) |
| **Scaleway Qwen3-Coder-30B** | Scaleway | 0.20 | 0.80 | EU/GDPR coding |
| DeepSeek V4-Pro | DeepSeek | 1.60 | 3.14 | Complex code, agents |
| Claude Sonnet 4 | Anthropic | 3.00 | 15.00 | Best quality, expensive |
| SiliconFlow Tencent Hy3 | SF | $0.066 | $0.26 | Cheapest LLM overall |

---

## 4. Document Counting (Pre-Classifier)

Task: "How many distinct documents in this image?" — 16 models tested on 4-receipt photo.

| # | Model | Provider | TTF | Tokens | Cost € | Answer | Verdict |
|---|---:|---|---|---:|---:|---:|---|
| 1 | **Mistral-small-3.2** | Scaleway | **0.7s** | 1104+2 | **0.00017** | 4 | ✅ KING |
| 2 | Pixtral-12B | Scaleway | 0.7s | 3167+2 | 0.00063 | 4 | ✅ |
| 3 | GLM-4.6V-flashx | Z.ai | 2.7s | 1038+4 | 0.00014 | 4 | ✅ needs thinking=disabled |
| 4 | GLM-4.6V | Z.ai | 3.5s | 1038+4 | 0.00041 | 4 | ✅ needs thinking=disabled |
| 5 | GPT-4o-mini | OpenAI | 2.4s | 25535+1 | 0.00352 | 4 | ✅ 20x cost |
| 6 | Gemma-4-26B (SF) | SF | 3.1s | 308+2 | 0.00004 | 1 | ❌ undercount |
| 7 | Qwen3.5-9B (SF) | SF | 4.1s | 808+10 | 0.00008 | — | ❌ empty |
| 8 | Gemma-4-26B (scw) | Scaleway | 0.8s | 311+10 | 0.00008 | — | ❌ empty |
| 9 | Qwen3.6-35B (scw) | Scaleway | 0.8s | 808+10 | 0.00022 | — | ❌ empty |
| 10 | GLM-5V-Turbo (SF) | SF | 5.9s | 1042+100 | 0.00152 | — | ❌ empty |
| 11-16 | Others | — | — | — | — | — | ❌ not VLM / errors |

### Pre-Classifier Strategy

```
1st: Scaleway Mistral-small — 0.7s, €0.00017 (EU, no rate limits, 100% accurate)
2nd: GLM-4.6V-flashx — 2.7s, €0.00014 (needs thinking=disabled)
3rd: GPT-4o-mini — 2.4s, €0.00352 (last resort, expensive)
```

---

## 5. Receipt OCR (Full Extraction)

### All-at-Once — CATASTROPHIC FAILURE

5 models asked to extract all 4 receipts as structured JSON at once:
**100% hallucination rate.** Every model invented fake companies, amounts, and tax IDs.

| Model | Invented Data |
|---|---|
| Mistral-small | "SIA BISTRO ROSE" ×3 copies (same receipt tripled) |
| Pixtral-12B | "SOCIEDAD LIMITADA", Spanish B-64564564 |
| GLM-4.6V-flashx | "SIA Līdzīguma apgabals 'Mūns'" |
| GLM-4.6V | "Baltic Beer House" bar receipts |
| GPT-4o-mini | "Sushi", "Treasurer Rise" (restaurants) |

**Ground truth** (from GLM-4.6V describe prompt):
St. L'Admirable €12.69, St. BARBARA PORSE €6.90,
St. Jv. PIRITA €56.55, St. Jv. KASTA €44.34.

### BBox+Crop+OCR Pipeline — SUCCESS

| Step | Tool | Time | Cost |
|---|---|---|---|
| Detect bboxes | GLM-4.6V | 6.0s | €0.0011 |
| Crop+enhance | PIL (local) | 0s | €0 |
| OCR ×4 receipts | Scaleway Mistral-small | 41s | €0.0024 |
| **Total 4 receipts** | | **47s** | **€0.0034** |
| **Per receipt** | | **~12s** | **€0.00085** |

**4/4 correctly extracted** with `raw_text_dump` for mathematical verification.

### Mathematical Validation

Validator checks:
1. Σ line items ≈ subtotal
2. VAT ≈ subtotal × VAT%
3. Subtotal + VAT ≈ total
4. Payment − change ≈ total
5. Structural (IBAN format, reg_nr format, date presence, raw_dump presence)

| Receipt | Score | Issues Found |
|---|---|---|
| Kurs (Riga) | 70/100 ❌ | Item double-count (€3.01 vs €2.01 subtotal), payment/change mismatch |
| BARIJA (Jūrmala) | 100/100 ✅ | Perfect |
| FOR ROSE (Kuldīga) | 100/100 ✅ | Perfect |
| Pigu Latvia (Riga) | 37/100 ❌ | VAT mismatch (21%×€59.45=€12.48 vs claimed €1.45), total≠subtotal+VAT |

**Validator catches real OCR errors that would otherwise go into accounting.**

---

## 6. Bounding Box Detection

Only GLM-4.6V has native bbox capability among tested models.

### API Call

```json
{
  "model": "glm-4.6v",
  "messages": [{"role": "user", "content": [
    {"type": "text", "text": "Detect each receipt. Return bbox + rotation."},
    {"type": "image_url", "image_url": {"url": "data:image/jpeg;base64,..."}}
  ]}],
  "max_tokens": 4000,
  "temperature": 0,
  "thinking": {"type": "disabled"}
}
```

### Output

```json
{
  "receipts": [
    {
      "index": 1,
      "store_name_hint": "SIA Tirdzniecibas nams Kurs",
      "bbox": [147, 0, 764, 215],
      "rotation_degrees": 0
    }
  ],
  "total_count": 4
}
```

### Performance

- Time: 6.0s
- Tokens: 1707+244
- Cost: ¥0.0085 = €0.0011
- **Per receipt**: €0.00028

### Z.AI Vision MCP Server Review

Z.AI offers a Vision MCP Server (`@z_ai/mcp-server`) with 8 tools:
`ui_to_artifact`, `extract_text_from_screenshot`, `diagnose_error_screenshot`,
`understand_technical_diagram`, `analyze_data_visualization`, `ui_diff_check`,
`image_analysis`, `video_analysis`.

**Verdict**: Useful for general vision tasks in Claude Code, but **no bounding-box
detection**. Bbox must be done via direct API call with structured JSON prompt.

---

## 7. Provider Comparison Matrix

### GLM-4.6V Family (Z.ai)

| Model | Input ¥/M | Output ¥/M | Context | Strengths | Weaknesses |
|---|---|---|---|---|---|
| glm-4.6v | 3.00 | 15.00 | 128K | Multi-doc king, bbox, grounding | Slow (28s), expensive output |
| glm-4.6v-flashx | 1.00 | 5.00 | 128K | Fast, cheap | Needs thinking=disabled |
| glm-4.6v-flash | FREE | FREE | 128K | Free | **UNUSABLE: permanent 429** |
| GLM-5V-Turbo (SF) | $1.20 | $4.00 | 205K | Latest gen, fast | Empty on structured prompts |

### Scaleway Vision Models (EU/GDPR)

| Model | €/M in | €/M out | TTF | Strengths | Weaknesses |
|---|---|---|---|---|---|
| Mistral-small-3.2 | 0.15 | 0.35 | 0.8s | Fastest counter, cheap, accurate OCR | No bbox, hallucinates on multi-doc JSON |
| Pixtral-12B | 0.20 | 0.20 | 0.8s | Counts correctly | Hallucinates Spanish data on OCR |
| Qwen3.6-35B | 0.25 | 1.50 | 0.8s | Large model | Returns empty on counting task |
| Gemma-4-26B | 0.25 | 0.50 | 0.8s | Fast | Returns empty on counting |

### Non-Vision Models (Chat only)

| Model | Provider | Vision? | Notes |
|---|---|---|---|
| GLM-5.2 | Z.ai | ❌ | 1M ctx, agentic. Correctly rejects vision with 400. |
| GLM-4.7 | Z.ai | ❌ | Correctly rejects vision. |
| GLM-4.5-Air | Z.ai | ❌ | Cheapest (€0.02/M in). Rejects vision. |
| DeepSeek V4-Flash | DeepSeek/SF | ❌ | Rejects vision. Use chat only. |
| Tencent Hy3 | SF | ❌ | Cheapest LLM ($0.066/M). No vision. |

---

## 8. Anti-Patterns & Pitfalls

### ❌ NEVER: OCR multiple small documents in one JSON call

5/5 models hallucinated. Text too small (~200px per receipt).
Structured JSON forces models to fill fields → they invent data.

### ❌ NEVER: Use GLM-4.6V with thinking=enabled + max_tokens<100

Thinking consumes budget → empty output. Always use `thinking: disabled`.

### ❌ NEVER: Rely on GLM-4.6V-flash (FREE)

Permanent HTTP 429. Rate limit never lifts. Use flashx instead.

### ❌ NEVER: Use GPT-4o-mini for structured document OCR

GPT-4o-mini hallucinated on ALL 4 Latvian receipts in testing:
replaced correct store names with fantasies ("Kivis" for Kurs, "Rimi" for Pigu Latvia),
and invented items ("Kefīrs, Sviests, Saldējums"). Math passed but data was fiction.
**GPT-4o-mini is structurally incapable of reading small receipt text reliably.**

### ❌ NEVER: Trust OCR without raw_text_dump

Models hallucinate. Without `raw_text_dump`, no way to verify.
Always include it and run mathematical validation.

### ✅ ALWAYS: Count → BBox → Crop → Individual OCR

The only pipeline that produced correct results.
47s, €0.0034 for 4 receipts, 100% verifiable.

### ✅ ALWAYS: Prefer dedicated OCR over VLMs for text extraction

VLMs hallucinate structured output. GLM-OCR reads pixel-by-pixel and is honest.
Google Gemini 1.5 Pro has the best vision encoder — reads Latvian diacritics.

### ✅ ALWAYS: Use Google Gemini for Latvian/EU receipts when available

Gemini 1.5 Pro is the only model that correctly read all diacritics (š, ī, ā, ģ),
applied discounts, and separated similar receipts. Dramatically better than any other provider.

---

## 9. Pipeline Architecture

### Recommended Vision Pipeline

```text
📸 Photo
      ↓
Step 0: PreClassifier — "How many documents?"
      │  Scaleway Mistral-small, 0.7s, €0.00017
      │  Fallback: GLM-4.6V-flashx → GPT-4o-mini
      │
  ┌───┼───────────┐
  ↓   ↓           ↓
1 doc  2+ docs  Unknown
  │    │           │
  │    │           └→ normal vision routing
  │    │
  │    └→ Step 1: BBox Detection — GLM-4.6V, 6s, €0.0011
  │             thinking=disabled, max_tokens=4000
  │             Returns [{bbox, rotation, hint}]
  │             
  │        Step 2: PIL Crop + Enhance
  │             Sharpen filter, contrast +30%
  │             Apply rotation if angle ≠ 0
  │             
  │        Step 3: Individual OCR × N
  │             Scaleway Mistral-small, ~5s each, €0.00017 each
  │             max_tokens=8192, response_format=json_object
  │             Include raw_text_dump in schema
  │             
  │        Step 4: Mathematical Validator
  │             Σitems≈subtotal, VAT≈subtotal×rate%, sub+VAT≈total
  │             Failed receipts → re-extract with different model
  │
  └→ cheap OCR directly (or skip if single doc)
```

### Model Selection by Task

| Task | Primary Model | Fallback | Never Use |
|---|---|---|---|
| Count documents | Scaleway Mistral-small | GLM-4.6V-flashx | GLM-4.6V-flash (FREE) |
| Describe photo | GLM-4.6V | Mistral-small | Pixtral (hallucinates) |
| BBox detection | GLM-4.6V | — | Anyone else (no capability) |
| Receipt OCR (Latvian) | **Google Gemini 1.5 Pro** | GLM-OCR | Mistral-small, GPT-4o-mini |
| Receipt OCR (general) | GLM-OCR ($0.03/M) | Gemini | Mistral-small (hallucinates) |
| Validation | Arbiter (math checks) | — | Blind trust in OCR |
| Chat (general) | Scaleway Qwen3-Coder | DeepSeek V4-Flash | DeepSeek Chat V3 (deprecated) |
| Chat (budget) | DeepSeek V4-Flash | SF Qwen3-8B | — |
| Chat (EU/GDPR) | Scaleway Mistral-small | Scaleway Qwen3-Coder | SiliconFlow (data to CN) |
| Code generation | SF Qwen3-Coder-30B | DeepSeek V4-Flash | — |
| Complex agents | DeepSeek V4-Pro | GLM-5.2 | DeepSeek Reasoner (deprecated) |

---

*All benchmarks: July 7, 2026, from live API calls. Prices subject to change.*
*Data sources: api.z.ai, api.scaleway.ai, api.siliconflow.com, api.deepseek.com, api.openai.com*
