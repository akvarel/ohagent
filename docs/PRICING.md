# AI Model Provider Price Comparison — July 2026

Comprehensive cost analysis across all providers ohAgent supports, ranked by price.
Prices in EUR unless marked with $ (USD). SiliconFlow added July 7, 2026.

---

## 1. Serverless / Pay-per-Token (for inference via API)

| Rank | Provider | Model | Input €/M tok | Output €/M tok | Best For |
|---|---|---|---|---|---|
| 🥇 | **SiliconFlow** | Tencent Hy3-preview | **$0.066** | **$0.26** | Cheapest LLM inference, MoE |
| 🥇 | **SiliconFlow** | Qwen3-Coder-30B-A3B | **$0.07** | **$0.28** | Cheapest coding model |
| 🥇 | **SiliconFlow** | Qwen3-8B | **$0.06** | **$0.06** | Ultra-cheap general chat |
| 🥇 | **SiliconFlow** | Qwen3.5-9B | **$0.10** | **$0.15** | Multimodal, 201 languages |
| 🥇 | **SiliconFlow** | Step-3.5-Flash | **$0.10** | **$0.30** | MOE, 196B params |
| 🥇 | **SiliconFlow** | DeepSeek-V4-Flash | **$0.13** | **$0.28** | DeepSeek Flash via aggregator |
| 🥇 | **SiliconFlow** | gemma-4-26B (MoE) | **$0.12** | **$0.40** | Google open-source, fast |
| 🥈 | **DeepSeek** | deepseek-v4-flash | 0.14 | 0.28 | Direct API, MIT license |
| 🥈 | **Scaleway** | mistral-small-3.2 | 0.15 | 0.35 | General chat, GDPR-safe |
| 🥈 | **Scaleway** | qwen3-coder-30b | 0.20 | 0.80 | Code, EU-hosted |
| 🥈 | **SiliconFlow** | Ling-flash-2.0 | **$0.14** | **$0.57** | Lightweight MoE, 100B |
| | **SiliconFlow** | Nex-N2-Pro | **$0.50** | **$2.50** | Agentic coding SOTA |
| | **SiliconFlow** | Qwen3.5-397B-A17B | **$0.39** | **$2.34** | Heavy reasoning MoE |
| | **SiliconFlow** | Kimi-K2.7-Code | **$0.94** | **$4.00** | Agentic coding, 1M ctx |
| | **SiliconFlow** | LongCat-2.0 | **$0.75** | **$2.95** | Agentic, tool use native |
| | **DeepSeek** | deepseek-chat (V3) | 0.27 | 1.10 | Balanced |
| | **DeepSeek** | deepseek-reasoner (R1) | 0.55 | 2.19 | Complex reasoning |
| | **OpenAI** | gpt-4o-mini | 0.15 | 0.60 | Fast, cheap |
| | **SiliconFlow** | DeepSeek-V4-Pro | **$1.60** | **$3.14** | 1.6T params, 49B active |
| | **OpenAI** | gpt-4o | 2.50 | 10.00 | Best quality |
| | **Anthropic** | claude-haiku-3.5 | 1.00 | 5.00 | Fast Claude |
| | **Anthropic** | claude-sonnet-4 | 3.00 | 15.00 | Coding king |
| | **Anthropic** | claude-opus-4 | 15.00 | 75.00 | Ultra quality |

> **Verdict**: SiliconFlow crushes everyone on per-token price. Tencent Hy3 at $0.066/M input is **2x cheaper than DeepSeek direct** and 2.3x cheaper than Scaleway. Qwen3-Coder at $0.07/M is the cheapest coding model. For EU/GDPR, Scaleway remains best (data stays in Paris/AMS).

---

## 2. Embeddings / Reranking

| Provider | Model | €/M tokens | Notes |
|---|---|---|---|
| 🥇 | **SiliconFlow** | Qwen3-Embedding-0.6B | **$0.01** | 32K ctx, 1024-dim |
| 🥇 | **SiliconFlow** | Qwen3-Reranker-0.6B | **$0.01** | Reranking |
| 🥈 | **SiliconFlow** | Qwen3-Embedding-4B | **$0.02** | 32K ctx, 2560-dim |
| 🥈 | **SiliconFlow** | Qwen3-Embedding-8B | **$0.04** | MTEB #1, 4096-dim |
| | **Scaleway** | qwen3-embedding-8b | 0.10 | EU-hosted |

---

## 3. Image Generation

| Provider | Model | €/Image | Notes |
|---|---|---|---|
| 🥇 | **SiliconFlow** | FLUX.1-schnell | **$0.0014** | Fastest/cheapest |
| 🥇 | **SiliconFlow** | FLUX.1-dev | **$0.014** | Higher quality |
| 🥈 | **SiliconFlow** | Z-Image-Turbo | **$0.005** | Tongyi-MAI |
| | **SiliconFlow** | Qwen-Image | **$0.02** | Alibaba |
| | **SiliconFlow** | Qwen-Image-Edit | **$0.04** | Inpainting/editing |

---

## 4. Video Generation

| Provider | Model | €/Video |
|---|---|---|
| 🥇 | **SiliconFlow** | Wan2.2-T2V-A14B | **$0.29** | Text-to-video |
| 🥇 | **SiliconFlow** | Wan2.2-I2V-A14B | **$0.29** | Image-to-video |

---

## 5. Audio / TTS

| Provider | Model | Price | Notes |
|---|---|---|---|
| | **SiliconFlow** | IndexTTS-2 | **$7.15/M UTF-8 bytes** | Zero-shot, emotion control |
| | **SiliconFlow** | Fish-Speech-1.5 | **$15.00/M UTF-8 bytes** | Multilingual |
| | **SiliconFlow** | CosyVoice2-0.5B | **$7.15/M UTF-8 bytes** | Streaming 150ms latency |
| | **Scaleway** | whisper-large-v3 | €0.003/min | Transcription |

---

## 6. GPU Instances (for self-hosted / LoRA fine-tuning)

| Rank | Provider | GPU | VRAM | €/hr | €/mo | Best For |
|---|---|---|---|---|---|---|
| 🥇 | **Scaleway** | L4 | 24GB | **€0.93** | €679 | Small LoRA |
| 🥇 | **Scaleway** | H100 | 80GB | **€3.40** | €2,482 | Large models |
| 🥈 | **Hetzner** | A100-40 | 40GB | €1.85 | — | Mid-range |
| 🥈 | **Hetzner** | A100-80 | 80GB | €2.50 | — | Best raw GPU |
| | **SiliconFlow** | Reserved GPUs | Various | Custom | — | Long-running fine-tunes |

---

## 7. Cost Comparison: 100K chat requests/month (1K in + 2K out tokens)

| Rank | Provider | Model | Cost/month |
|---|---|---|---|
| 🥇 | **SiliconFlow** | Qwen3-8B ($0.06/0.06) | **$18** (€16) |
| 🥇 | **SiliconFlow** | Qwen3.5-9B ($0.10/0.15) | **$40** (€37) |
| 🥇 | **SiliconFlow** | Tencent Hy3 ($0.066/0.26) | **$59** (€54) |
| 🥈 | **SiliconFlow** | DeepSeek-V4-Flash ($0.13/0.28) | **$69** (€64) |
| | **Scaleway** | mistral-small (€0.15/0.35) | **€85** |
| | **DeepSeek** | v4-flash (€0.14/0.28) | **€70** |
| | **OpenAI** | gpt-4o-mini (€0.15/0.60) | **€135** |

---

## 8. Cost Comparison: 10K coding tasks/month (3K in + 8K out)

| Rank | Provider | Model | Cost/month |
|---|---|---|---|
| 🥇 | **SiliconFlow** | Qwen3-Coder-30B-A3B ($0.07/0.28) | **$24.50** (€22.50) |
| 🥇 | **SiliconFlow** | DeepSeek-V4-Flash ($0.13/0.28) | **$26.30** (€24) |
| 🥈 | **SiliconFlow** | Qwen3-Coder-480B-A35B ($0.25/1.00) | **$87.50** (€80) |
| | **DeepSeek** | deepseek-chat (€0.27/1.10) | **€96** |
| | **Scaleway** | qwen3-coder-30b (€0.20/0.80) | **€70** |
| | **Anthropic** | claude-sonnet-4 (€3.00/15.00) | **€1,290** |

---

## 9. Key Takeaways

1. **SiliconFlow is the cheapest API aggregator** — Tencent Hy3 at $0.066/M input beats everything. **2x cheaper than DeepSeek direct, 2.3x cheaper than Scaleway.**

2. **Coding models are absurdly cheap on SiliconFlow** — Qwen3-Coder-30B at $0.07/M input is 53x cheaper than Anthropic Claude Sonnet 4 for coding tasks.

3. **Image generation at $0.0014/image** (FLUX schnell) means you can generate 714 images for $1.

4. **Video generation at $0.29/video** (Wan2.2) is usable for production.

5. **Embeddings at $0.01/M tokens** (Qwen3-Embedding-0.6B) — 10x cheaper than Scaleway's embedding offer.

6. **Scaleway wins for EU/GDPR compliance** — all data stays in Paris/Amsterdam datacenters. SiliconFlow likely routes to China.

7. **SiliconFlow = OpenRouter for Chinese models** — 200+ models, single API, consistent pricing. Like OpenRouter but focused on Asian providers.

8. **For ohAgent strategy**: SiliconFlow for cheap general/cheap coding, Scaleway for GDPR workloads, DeepSeek direct for best balance, Hetzner/Scaleway GPU for custom LoRA.

## 10. Speed Comparison — Real Benchmarks + Estimates

✅ = measured (`ohagent-metrics benchmark`, July 7, 2026).
≈ = estimated from provider docs / community data.

### Streaming (TTF = time-to-first-token)

| Rank | Model | TTF ms | tok/s | Price €/M | Source |
|---|---|---|---|---|---|
| 🥇 | DeepSeek V4-Flash | **2,221** ✅ | **46.0** ✅ | 0.14 | Real |
| 🥈 | DeepSeek V4-Pro (1.6T MoE) | **6,506** ✅ | **18.3** ✅ | 1.60 | Real |
| | ~~DeepSeek Chat V3~~ | ~~1,889~~ ✅ | ~~55.3~~ ✅ | 0.27 | ⚠️ Deprecated |
| | ~~DeepSeek Reasoner R1~~ | ~~3,432~~ ✅ | ~~31.2~~ ✅ | 0.55 | ⚠️ Deprecated |
| | Groq Llama 3.3-70B | ~100 | ≈250 | — | LPU hardware |
| | GPT-4o-mini | ~200 | ≈100 | 0.15 | Docs |
| | GLM-5.2 (SiliconFlow) | ~500 | ≈58 | 1.30 | Agentic #1 |

### Non-Streaming Throughput (800 tokens output)

| Rank | Model | Total time | tok/s | Cost/req |
|---|---|---|---|---|
| 🥇 | DeepSeek V4-Flash | **10.84s** ✅ | **73.8** ✅ | €0.011 |
| 🥈 | ~~DeepSeek Chat V3~~ | ~~12.51s~~ ✅ | ~~63.9~~ ✅ | €0.022 |

> **Key finding**: V4-Flash wins on both price AND throughput for batch.
> Chat V3 had better streaming TTF but is deprecated — V4-Flash is the replacement.

### Full DeepSeek Lineup (July 7, 2026)

⚠️ **DeepSeek Chat (V3) and Reasoner (R1) are deprecated** — will be shut down within July 2026. Migrate to V4-Flash (replaces Chat) and V4-Pro (replaces Reasoner).

| Model | Active params | TTF ms | tok/s | €/M input | Status | Best for |
|---|---|---|---|---|---|---|
| **V4-Flash** | 13B (MoE) | 2,221 ✅ | 46.0 ✅ | 0.14 | ✅ Current | Budget, general chat |
| **V4-Pro** | 49B (MoE) | 6,506 ✅ | 18.3 ✅ | 1.60 | ✅ Current | Complex code, agents |
| ~~Chat V3~~ | ~37B (MoE) | 1,889 ✅ | 55.3 ✅ | 0.27 | ⚠️ Deprecated | Migrate to V4-Flash |
| ~~Reasoner R1~~ | ~37B (MoE) | 3,432 ✅ | 31.2 ✅ | 0.55 | ⚠️ Deprecated | Migrate to V4-Pro |

> **Migration guide**:
> - Chat V3 → V4-Flash: 17% more TTF (1.9→2.2s) but **48% cheaper** (€0.27→€0.14/M)
> - Reasoner R1 → V4-Pro: 90% more TTF (3.4→6.5s) but **3x more capable** (49B active vs 37B)
> - **Net**: Slightly slower, significantly cheaper or more capable. Worth the switch.

> **Trade-off matrix**: Speed vs Price vs Quality
> - **Budget king**: V4-Flash (2.2s TTF, 46 tok/s, €0.14/M) — cheapest AND fast enough
> - **Fastest (deprecated)**: Chat V3 (1.9s, 55 tok/s) — slightly faster but 2x pricier, shutting down
> - **Best quality**: V4-Pro (1.6T params, 49B active) — slow but top-tier reasoning
> - **Agentic value**: GLM-5.2 on SiliconFlow — Claude-quality, 1M ctx, ~58 tok/s estimated
> - **Rule of thumb**: V4-Flash for 90% of tasks, V4-Pro when you need top quality, GLM-5.2 for agents

## 11. Dynamic Routing with ohagent-provider-metrics

New module for automated provider selection:

```bash
# Daily price scrape
cargo run -p ohagent-provider-metrics -- scrape

# Route based on task
cargo run -p ohagent-provider-metrics -- route \
  --capabilities chat,code --prompt-tokens 3000 --output-tokens 8000 --tier balanced

# Run speed benchmark
cargo run -p ohagent-provider-metrics -- benchmark \
  --provider deepseek --model deepseek-v4-flash \
  --api-key $DEEPSEEK_KEY --api-base https://api.deepseek.com/v1

# Compare speed estimates
cargo run -p ohagent-provider-metrics -- speed-compare
```

Routing algorithm: `score = α·price_score + β·speed_score + γ·quality_score`
where (α,β,γ) = (0.7,0.2,0.1) Budget / (0.4,0.3,0.3) Balanced / (0.2,0.6,0.2) Performance / (0.1,0.1,0.8) Quality.

---

Data sources: scaleway.com, deepseek.com, openai.com, anthropic.com, hetzner.com, siliconflow.com/models (as of July 2026).
SiliconFlow prices in USD; approximate EUR conversion at ~0.92.
