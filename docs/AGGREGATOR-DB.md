# ohAggregator — Multi-Tenant API Key Management & Billing

## Database: PostgreSQL (primary) + DragonflyDB (cache)

The aggregator manages multiple customers, each with their own API keys
for different providers. SQLite is NOT suitable — we need concurrent writes,
row-level security, and complex billing queries.

### Why PostgreSQL over SQLite

- **Concurrent writes**: Multiple tenants recording usage simultaneously
- **Row-level security**: `tenant_id` based RLS policies
- **Billing queries**: `SUM(...) GROUP BY tenant_id WHERE created_at BETWEEN`
- **Connection pooling**: PgBouncer for 100+ concurrent connections
- **JSONB columns**: Flexible per-tenant config without schema changes

### Why DragonflyDB (Redis-compatible) over Memcached

- **Rate limiting**: Sliding window counters per tenant/provider
- **Session tokens**: JWT blacklist/whitelist with TTL
- **Usage cache**: 60-second bucket before flush to PostgreSQL
- **Dragonfly is 25x faster than Redis** for multi-key operations

## Schema Design

### PostgreSQL Tables

```sql
-- ── Tenants ──
CREATE TABLE tenants (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL,
    slug        TEXT UNIQUE NOT NULL,           -- URL-safe identifier
    email       TEXT,
    plan        TEXT NOT NULL DEFAULT 'free',   -- 'free', 'pro', 'enterprise'
    status      TEXT NOT NULL DEFAULT 'active', -- 'active', 'suspended', 'closed'
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── API Keys (per tenant, per provider) ──
CREATE TABLE api_keys (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id   UUID NOT NULL REFERENCES tenants(id),
    provider    TEXT NOT NULL,                  -- 'deepseek', 'gemini', 'openai', 'zai', 'siliconflow', 'scaleway'
    key_hash    TEXT NOT NULL,                  -- SHA256 of API key (NEVER store plaintext)
    key_prefix  TEXT,                           -- First 8 chars for UI display
    is_default  BOOLEAN NOT NULL DEFAULT false,
    is_active   BOOLEAN NOT NULL DEFAULT true,
    rate_limit  INT,                            -- Requests per minute (NULL = provider default)
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(tenant_id, provider, key_hash)
);

CREATE INDEX idx_api_keys_tenant ON api_keys(tenant_id);
CREATE INDEX idx_api_keys_provider ON api_keys(tenant_id, provider);

-- ── Usage Records (raw, unbilled) ──
CREATE TABLE usage_records (
    id          BIGSERIAL PRIMARY KEY,
    tenant_id   UUID NOT NULL REFERENCES tenants(id),
    provider    TEXT NOT NULL,
    model_id    TEXT NOT NULL,
    prompt_tokens   BIGINT NOT NULL DEFAULT 0,
    completion_tokens BIGINT NOT NULL DEFAULT 0,
    cost_eur    NUMERIC(12,8) NOT NULL DEFAULT 0,  -- Our cost (from provider)
    billed_eur  NUMERIC(12,8) NOT NULL DEFAULT 0,  -- Customer bill (with markup)
    markup_pct  NUMERIC(5,2),                      -- Applied markup percentage
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_usage_tenant_time ON usage_records(tenant_id, created_at DESC);
CREATE INDEX idx_usage_provider_time ON usage_records(provider, created_at DESC);

-- Partition by month for query performance
SELECT create_hypertable('usage_records', 'created_at',
    chunk_time_interval => INTERVAL '1 day');

-- ── Billing Periods ──
CREATE TABLE billing_periods (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id   UUID NOT NULL REFERENCES tenants(id),
    period_start TIMESTAMPTZ NOT NULL,
    period_end   TIMESTAMPTZ NOT NULL,
    total_cost   NUMERIC(12,4) NOT NULL DEFAULT 0,   -- Our cost
    total_billed NUMERIC(12,4) NOT NULL DEFAULT 0,   -- Customer bill
    status      TEXT NOT NULL DEFAULT 'open',         -- 'open', 'closed', 'paid'
    invoice_url TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    closed_at   TIMESTAMPTZ,
    UNIQUE(tenant_id, period_start)
);

-- ── Markup Tiers ──
CREATE TABLE markup_tiers (
    id          SERIAL PRIMARY KEY,
    plan        TEXT NOT NULL,                   -- 'free', 'pro', 'enterprise'
    provider    TEXT,                            -- NULL = all providers
    model_pattern TEXT,                          -- 'gemini-*' = all Gemini models
    markup_pct  NUMERIC(5,2) NOT NULL,           -- e.g. 20.00 = 20% markup
    min_volume  BIGINT NOT NULL DEFAULT 0,       -- Apply above N tokens/month
    is_active   BOOLEAN NOT NULL DEFAULT true
);

-- Default tiers
INSERT INTO markup_tiers (plan, provider, markup_pct) VALUES
    ('free',     NULL, 0),       -- Free tier: no markup (cost = bill)
    ('pro',      NULL, 20),      -- Pro: 20% markup on all providers
    ('enterprise', NULL, 15);    -- Enterprise: 15% (volume discount)

-- ── Row-Level Security ──
ALTER TABLE api_keys ENABLE ROW LEVEL SECURITY;
ALTER TABLE usage_records ENABLE ROW LEVEL SECURITY;

CREATE POLICY tenant_isolation ON api_keys
    USING (tenant_id = current_setting('app.current_tenant_id')::UUID);

CREATE POLICY tenant_isolation ON usage_records
    USING (tenant_id = current_setting('app.current_tenant_id')::UUID);
```

### DragonflyDB (Cache) Keys

```
# Rate limiting (per tenant, per provider, per minute)
rate_limit:{tenant_id}:{provider}:{YYYYMMDDHHMM} → counter (TTL 60s)

# Usage cache (aggregated every 60s, then flushed to PG)
usage:{tenant_id}:{provider}:{YYYYMMDDHH} → JSON {
  "prompt_tokens": 125000,
  "completion_tokens": 89000,
  "cost_eur": 0.0035,
  "requests": 42
} (TTL 3600s)

# Session tokens (API auth)
session:{token_hash} → {tenant_id, expires_at} (TTL = session duration)

# Provider status cache
provider:{name}:status → {"available":true,"latency_ms":45} (TTL 30s)
```

## API Endpoints (Aggregator Gateway)

```
# Tenant management
POST   /api/v1/tenants                    → create tenant
GET    /api/v1/tenants/{id}              → tenant details
PUT    /api/v1/tenants/{id}              → update plan

# API Key management
POST   /api/v1/keys                       → add provider key
GET    /api/v1/keys                       → list keys (masked)
DELETE /api/v1/keys/{id}                  → revoke key
POST   /api/v1/keys/{id}/rotate           → rotate key

# Usage & Billing
GET    /api/v1/usage?from=X&to=Y          → usage summary
GET    /api/v1/usage/daily/{date}         → daily breakdown
GET    /api/v1/billing/current            → current period
GET    /api/v1/billing/history            → past periods
GET    /api/v1/billing/{id}/invoice       → download invoice PDF

# Chat (proxy — the core product)
POST   /api/v1/chat/completions            → OpenAI-compatible, routes to cheapest provider
POST   /api/v1/chat/completions/{provider} → force specific provider
```

## Cost Estimate

| Component | Specs | €/mo |
|---|---|---|
| PostgreSQL (Scaleway Managed) | 2 vCPU, 4GB RAM, 50GB SSD | €25 |
| DragonflyDB | 1 vCPU, 2GB RAM | €15 |
| PgBouncer | Sidecar, 0 resources | €0 |
| **Total database** | | **€40/mo** |

At 10M API calls/month, database load is:
- ~10 writes/sec (usage records)
- ~100 reads/sec (key lookup + routing)
- PostgreSQL handles this on 1 vCPU easily

## Migration from SQLite

Current ohAgent uses SQLite at `~/.ohagent/*.db`. For the aggregator:

1. **Keep SQLite** for agent-internal data (memory, skills, sessions) — no change
2. **Add PostgreSQL** for aggregator data (tenants, keys, usage, billing)
3. **Add DragonflyDB** for rate limiting + usage cache

This is an additive change — existing daemon behavior is unchanged.
Only `/api/v1/*` aggregator endpoints use PG; everything else stays SQLite.
