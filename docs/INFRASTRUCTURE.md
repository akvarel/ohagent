# ohAggregator Server Requirements — Honest Estimate

July 2026. We proxy requests to providers — **we do NOT run inference**.
This makes the aggregator fundamentally different from GPU-heavy deployments.

---

## TL;DR

| Users | Requests/day | Servers | Monthly infra cost |
|---|---|---|---|
| 10 (dev) | 100 | **1 × Scaleway PRO2** (€28/mo) | **€28** |
| 100 (beta) | 10,000 | **1 × PRO2** (€28/mo) | **€28** |
| 1,000 (launch) | 100,000 | **2 × PRO2** (€56/mo) | **€56** |
| 10,000 (scale) | 1,000,000 | **3 × PRO2 + LB** (€120/mo) | **€120** |
| 100,000 (Series A) | 10M | **5 × ENT1 + Redis + PG** (€800/mo) | **€800** |

---

## What consumes resources

### 1. Request proxying (CPU)
Each request: validate API key (SHA256 hash lookup) → route to provider → stream response back.
- SHA256 hash: ~1μs per key
- Provider API call: **we don't process tokens, we proxy bytes**
- Cost: ~0.1ms CPU per request — negligible

**Bottleneck**: provider API latency (200ms-6s), not our CPU.

### 2. Billing (SQLite writes)
Each request: 1 INSERT into usage_records + 1 UPDATE on api_keys (last_used_at).
- SQLite WAL mode: ~10,000 writes/sec on NVMe SSD
- Our workload: 1 write per request

**Bottleneck**: SQLite handles 1,000 req/s easily. Beyond 10K req/s → switch to PostgreSQL.

### 3. Concurrent connections
Each streaming request holds a TCP connection open for 2-30 seconds (provider response time).
- Rust async (tokio): handles 10,000+ concurrent connections on 4GB RAM
- Each connection: ~10KB memory (buffer + state)

**Bottleneck**: file descriptors (ulimit -n). Default 1024. Increase to 65535.

### 4. Bandwidth
- Each request: ~1KB headers + ~2-10KB response (streaming SSE)
- 100K req/day: ~500MB/day = ~0.05 Mbps average

**Bottleneck**: none. Even 10M req/day = 5 Mbps. Provider bandwidth dominates.

---

## Real hardware requirements

### Phase 1: Dev (10 users)

```
Scaleway PRO2-X4C-16G (€28/mo)
  4 vCPU AMD EPYC, 16 GB RAM, 200 GB NVMe
  Runs: ohagent-daemon + SQLite
  Handles: 10K req/day comfortably
```

We're already running this. Zero additional cost.

### Phase 2: Beta (100-1000 users)

```
1 × Scaleway PRO2-X8C-32G (€86/mo)
  8 vCPU, 32 GB RAM, 400 GB NVMe
  Handles: 1M req/day
```

Single instance. No load balancer needed at this scale.

### Phase 3: Launch (1K-10K users)

```
2 × Scaleway PRO2-X8C-32G (€172/mo)
  + Scaleway Load Balancer (€11/mo)
  + Managed PostgreSQL (€35/mo Starter)
  Total: ~€218/mo
```

Add PostgreSQL when SQLite write contention appears (~5K+ concurrent req/s).
Add LB for zero-downtime deploys and health checks.

### Phase 4: Scale (10K-100K users)

```
3 × Scaleway ENT1-X16C-64G (€345/mo)
  + Load Balancer (€23/mo)
  + Managed PostgreSQL (€110/mo Business)
  + DragonflyDB for rate limiting (€50/mo)
  Total: ~€528/mo
```

Separate billing DB from API serving. Redis/Dragonfly for per-key rate limiting.

---

## What we DON'T need

| Resource | Why we don't need it |
|---|---|
| **GPU** | We proxy, don't run models |
| **Massive RAM** | Each connection ~10KB, not model weights |
| **100 Gbps network** | Provider bandwidth dominates, not ours |
| **Kubernetes** | 3 instances on bare metal = simpler, cheaper |
| **CDN for API** | Dynamic content per user, can't cache |
| **S3/object storage** | SQLite/PostgreSQL for structured billing data |

---

## SQLite → PostgreSQL migration trigger

SQLite in WAL mode handles ~500 concurrent readers + 1 writer without contention.
Migration needed when:

- Write throughput exceeds ~5,000 INSERT/sec (about 5M concurrent req/s)
- Database reaches ~10GB (about 500M billing records)
- Need replication for HA (SQLite has no built-in replication)

**Realistic timeline**: SQLite works until ~50K paying users. Then switch to PG.

---

## Revenue vs Cost at Scale

| Tier | Users | Monthly Revenue | Infra Cost | Infrastructure Margin |
|---|---|---|---|---|
| Dev | 10 | €0 | €28 | — |
| Beta | 100 | €1,900 | €28 | **98.5%** |
| Launch | 1,000 | €19,000 | €218 | **98.8%** |
| Scale | 10,000 | €990,000 | €528 | **99.95%** |

The real cost is **provider API fees** (what we pay DeepSeek/etc.), not our infrastructure.
At 10K Pro users (€99/mo each): revenue €990K/mo, our infra €528/mo, provider costs ~€300K/mo.

---

## Cheapest possible deployment

```bash
# Literally: one $5 VPS handles 1000 users
Hetzner CX22 (€3.99/mo): 2 vCPU, 4 GB RAM, 40 GB SSD
  → Handles ~500 req/s (43M req/day) — more than enough for 100K users

Scaleway DEV1-M (€9.99/mo): 2 vCPU, 4 GB RAM
  → Same capacity, but in EU (GDPR)

# That's it. One server. No GPU. No Kubernetes. No CDN.
```

---

## Summary

**ohAggregator infrastructure is trivially cheap** because we proxy, not compute.
A single €4/month VPS can handle 1000 paying customers.
Our real cost is what we pay providers — which customers cover via markup.

Infrastructure at 99%+ margin. The business is in the arbitrage, not the hosting.
