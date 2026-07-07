# AI Model Provider Price Comparison — July 2026

Comprehensive cost analysis across all providers ohAgent supports, ranked by price.

---

## 1. Serverless / Pay-per-Token (for inference via API)

| Rank | Provider | Model | Input €/M tok | Output €/M tok | Best For |
|---|---|---|---|---|---|
| 🥇 | **Scaleway** | mistral-small-3.2 | **0.15** | **0.35** | General chat, GDPR-safe |
| 🥇 | **Scaleway** | qwen3-coder-30b | **0.20** | **0.80** | Code generation |
| 🥇 | **Scaleway** | gemma-4-26b | **0.25** | **0.50** | Vision + chat |
| 🥇 | **Scaleway** | pixtral-12b | **0.20** | **0.20** | Multimodal vision |
| 🥈 | **Scaleway** | llama-3.3-70b | 0.90 | 0.90 | Enterprise chat |
| 🥈 | **Scaleway** | qwen3.5-397b (MoE) | 0.60 | 3.60 | Heavy reasoning |
| | **DeepSeek** | deepseek-v4-flash | 0.14 | 0.28 | General, best price for small models |
| | **DeepSeek** | deepseek-chat (V3) | 0.27 | 1.10 | Balanced |
| | **DeepSeek** | deepseek-reasoner (R1) | 0.55 | 2.19 | Complex reasoning |
| | **OpenAI** | gpt-4o-mini | 0.15 | 0.60 | Fast, cheap |
| | **OpenAI** | gpt-4o | 2.50 | 10.00 | Best quality |
| | **Anthropic** | claude-haiku-3.5 | 1.00 | 5.00 | Fast Claude |
| | **Anthropic** | claude-sonnet-4 | 3.00 | 15.00 | Coding king |
| | **Anthropic** | claude-opus-4 | 15.00 | 75.00 | Ultra quality |

> **Verdict**: Scaleway serverless beats everyone on price/quality ratio for European workloads. DeepSeek is cheaper per-token but Scaleway has more models and GDPR compliance.

---

## 2. GPU Instances (for self-hosted inference / LoRA fine-tuning)

| Rank | Provider | GPU | VRAM | €/hr | €/mo | Tok/s* | Best For |
|---|---|---|---|---|---|---|---|
| 🥇 | **Scaleway** | L4 | 24GB | **€0.93** | €679 | 1,500 | Small LoRA, cheap inference |
| 🥇 | **Scaleway** | L40S | 48GB | **€1.72** | €1,255 | 3,000 | Medium models |
| 🥇 | **Scaleway** | H100 | 80GB | **€3.40** | €2,482 | 8,000 | Large models, full fine-tune |
| 🥈 | **Hetzner** | A100-40 | 40GB | €1.85 | — | 4,000 | Mid-range GPU sweet spot |
| 🥈 | **Hetzner** | A100-80 | 80GB | €2.50 | — | 8,000 | Best raw GPU price |
| | **PaperSpace** | A100-80 | 80GB | €2.48 | — | 8,000 | Similar to Hetzner |
| | **Hetzner** | CCX13 | 40GB | ~€1.85 | — | 4,000 | Value A100 |
| | **Hetzner** | CCX23 | 80GB | ~€2.50 | — | 8,000 | Value A100-80 |
| | **Scaleway** | H100-SXM-8 | 640GB | €30.06 | €21,944 | 64,000 | Massive training |
| | **AWS** | p4d (A100) | 40GB | €3.91 | — | 4,000 | Enterprise lock-in |
| | **GCP** | a2-highgpu-1g | 40GB | €3.68 | — | 4,000 | GCP ecosystem |

*\*Estimated tokens/second for a 7B-parameter model at batch=1*

> **Verdict**: Scaleway L4 at €0.93/hr is unbeatable for small LoRA. Hetzner A100-80 at €2.50/hr is the best raw GPU deal. AWS/GCP are 2-3x more for the same hardware.

---

## 3. Cost Comparison: 2-hour LoRA fine-tuning session

| Provider | GPU | Cost for 2h | Model quality |
|---|---|---|---|
| **Scaleway** | L4 24GB | **€1.86** | Qwen2.5-7B, Llama-8B |
| **Hetzner** | A100-40 | **€3.70** | Qwen2.5-14B, Llama-13B |
| **Scaleway** | H100 80GB | **€6.80** | Qwen2.5-72B, Llama-70B |
| AWS | A100 | €7.82 | Same as A100-40 |
| GCP | A100 | €7.36 | Same as A100-40 |

---

## 4. Cost Comparison: 100K chat requests/month

| Provider | Model | Cost/month |
|---|---|---|
| **Scaleway** | mistral-small (serverless) | **€50** | (1K tok in + 2K tok out × 100K) |
| **Scaleway** | qwen3-coder (serverless) | **€180** |
| **DeepSeek** | v4-flash | **€70** |
| **OpenAI** | gpt-4o-mini | **€135** |
| **Anthropic** | claude-haiku | **€1,100** |
| **Scaleway** | llama-3.3-70b | **€360** |
| **Scaleway** | L4 GPU (24/7) | **€679** (flat) |
| **OpenAI** | gpt-4o | **€2,250** |

---

## 5. Cost Comparison: 10K coding tasks/month

| Provider | Model | Cost/month |
|---|---|---|
| **DeepSeek** | deepseek-chat | **€46** | (3K tok in + 8K tok out × 10K) |
| **Scaleway** | qwen3-coder-30b | **€70** |
| **Anthropic** | claude-sonnet-4 | **€1,290** |

---

## 6. Recommendation Matrix

| Use Case | Best Provider | Why |
|---|---|---|
| **General chat, budget** | Scaleway mistral-small / DeepSeek v4-flash | €0.15-0.14/M tok input |
| **Code generation** | DeepSeek chat | Best code quality/price ratio |
| **GDPR compliance** | Scaleway (Paris/AMS) | All data stays in EU |
| **Complex reasoning** | DeepSeek reasoner / Scaleway qwen3.5-397b | Chain-of-thought at reasonable price |
| **Top quality coding** | Anthropic Claude Sonnet 4 | Unbeatable code quality (€€€) |
| **Custom LoRA (cheap)** | Scaleway L4 GPU | €0.93/hr — cheapest managed GPU |
| **Custom LoRA (fast)** | Hetzner A100-80 | €2.50/hr — best raw GPU price |
| **Large model fine-tune** | Scaleway H100-80 | €3.40/hr — managed, no DevOps |
| **Batch processing** | Scaleway batches API | -50% discount on serverless |
| **Vision/multimodal** | Scaleway pixtral-12b / gemma-4 | €0.20-0.25/M tok, EU-hosted |

---

## Key Takeaways

1. **Scaleway serverless is the best deal in Europe**: €0.15/M tok input with GDPR, free tier, and -50% batches.

2. **Hetzner GPU is cheapest raw compute** but Scaleway L4 at €0.93/hr is close and managed.

3. **DeepSeek is cheapest globally** (€0.14/M tok) but your data goes to China unless using EU endpoint.

4. **Anthropic/OpenAI are for quality, not price** — 10-50x more expensive than Scaleway/DeepSeek.

5. **For ohAgent**: default to Scaleway serverless for general chat + DeepSeek for code + Hetzner/Scaleway GPU for custom LoRA deployment.

---

Data sources: scaleway.com/pricing, deepseek.com/pricing, openai.com/pricing, anthropic.com/pricing, hetzner.com/cloud (as of July 2026)
