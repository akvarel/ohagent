# ohAgent Sandbox Architecture

Isolated execution environment for agent sessions: write code, test it, access
isolated data — per-tenant, destroyable, resource-limited.

## Design Goals

1. **Per-tenant isolation** — Tenant A cannot see Tenant B's files/processes
2. **Resource limits** — CPU/memory caps prevent one session from starving others
3. **Ephemeral** — Filesystem destroyed after session ends (persistent state via SQLite/object store)
4. **Pre-installed tooling** — bash, python3, node, cargo, git, curl, jq
5. **Network policy** — Allow outbound (for package installs), block inbound
6. **Fast startup** — <2 seconds from session creation to ready

## Architecture

```text
┌─────────────────────────────────────────────────────────────────────┐
│                      ohAgent Daemon (host)                          │
│                                                                     │
│  Jcode Session → spawn_sandbox(tenant_id, session_id)              │
│                          │                                          │
│                          ▼                                          │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │              Sandbox Manager (Rust)                            │ │
│  │                                                                │ │
│  │  • Creates per-session Docker container                        │ │
│  │  • Mounts tenant data volume (SQLite, workspace)               │ │
│  │  • Enforces CPU/memory limits via cgroups                      │ │
│  │  • Streams stdin/stdout via Docker API                         │ │
│  │  • Destroys container on session end (or TTL expiry)           │ │
│  └───────────────────────────────────────────────────────────────┘ │
│                          │                                          │
│           ┌──────────────┼──────────────┐                           │
│           ▼              ▼              ▼                           │
│     ┌─────────┐   ┌─────────┐   ┌─────────┐                       │
│     │ tenant_a │   │ tenant_b │   │ tenant_c │                      │
│     │ container│   │ container│   │ container│                      │
│     │          │   │          │   │          │                       │
│     │ /workspace  /workspace  /workspace                           │
│     │ /data       /data       /data                                │
│     │ CPU: 1.0    CPU: 1.0    CPU: 1.0                             │
│     │ Mem: 512Mi  Mem: 512Mi  Mem: 512Mi                           │
│     └─────────┘   └─────────┘   └─────────┘                       │
└─────────────────────────────────────────────────────────────────────┘
```

## Container Image

```dockerfile
FROM rust:1.82-slim AS builder
# Pre-install tooling
RUN apt-get update && apt-get install -y --no-install-recommends \
    python3 python3-pip python3-venv \
    nodejs npm \
    git curl jq ca-certificates \
    build-essential pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Pre-warm cargo registry for faster first builds
RUN cargo install cargo-watch cargo-edit 2>/dev/null || true

# Non-root user for isolation
RUN useradd -m -s /bin/bash sandbox
USER sandbox
WORKDIR /workspace
```

**Image size**: ~450MB (Rust toolchain + Python + Node)

## Sandbox Lifecycle

```
1. CREATE:    POST /api/sandbox/{tenant_id}/create
              → docker create + start (2s)
              → returns container_id

2. EXEC:      POST /api/sandbox/{tenant_id}/{container_id}/exec
              {"command": "cargo build --release"}
              → streams stdout/stderr via WebSocket

3. HEALTH:    GET /api/sandbox/{tenant_id}/{container_id}/health
              → {"status":"running","cpu_pct":15,"mem_mb":120}

4. DESTROY:   DELETE /api/sandbox/{tenant_id}/{container_id}
              → docker stop + rm (1s)
              → or auto-destroy on TTL expiry (default: 30min idle)
```

## Resource Limits (per container)

| Resource | Default | Max | Notes |
|---|---|---|---|
| CPU | 1.0 core | 2.0 cores | CFS quota |
| Memory | 512 MiB | 1 GiB | RSS hard limit |
| Disk | 2 GiB | 5 GiB | tmpfs, destroyed on stop |
| PIDs | 64 | 128 | Prevents fork bombs |
| Network | outbound only | — | No ingress ports exposed |

## K8s Integration

In Kubernetes, each sandbox is a **separate Pod** (not Docker-in-Docker):

```yaml
apiVersion: v1
kind: Pod
metadata:
  name: sandbox-{tenant_id}-{session_id}
  namespace: ohagent-sandboxes
  labels:
    tenant: {tenant_id}
    session: {session_id}
    ttl: "1800"  # 30 minutes
spec:
  containers:
    - name: workspace
      image: rg.pl-waw.scw.cloud/orangehat/sandbox:latest
      resources:
        requests: {cpu: "250m", memory: "256Mi"}
        limits:   {cpu: "2",     memory: "1Gi"}
      securityContext:
        runAsNonRoot: true
        runAsUser: 1000
        allowPrivilegeEscalation: false
        capabilities: {drop: ["ALL"]}
        seccompProfile: {type: RuntimeDefault}
      volumeMounts:
        - name: workspace
          mountPath: /workspace
        - name: data
          mountPath: /data
  volumes:
    - name: workspace
      emptyDir: {medium: Memory, sizeLimit: 2Gi}
    - name: data
      persistentVolumeClaim:
        claimName: ohagent-tenant-{tenant_id}
  restartPolicy: Never
  terminationGracePeriodSeconds: 10
```

TTL controller deletes pods after 30 minutes of inactivity.

## API Endpoints

```
POST   /api/sandbox/create       → {container_id, ws_url}
POST   /api/sandbox/{id}/exec    → stream stdout/stderr
GET    /api/sandbox/{id}/health  → status + resource usage
DELETE /api/sandbox/{id}         → force destroy
GET    /api/sandbox/list         → list active (by tenant)
WS     /api/sandbox/{id}/ws      → bidirectional WebSocket
```

## Cost Estimate

| Component | Request | Limit | €/mo |
|---|---|---|---|
| Sandbox pod (per session) | 250m/256Mi | 2000m/1Gi | ~€15/mo if always-on |
| 10 concurrent sandboxes | | | ~€150/mo |
| With TTL (30 min idle) | avg 3 pods | | ~€45/mo |

Sandbox pods are **ephemeral** — they exist only during active agent sessions.
TTL-based cleanup keeps costs predictable.
