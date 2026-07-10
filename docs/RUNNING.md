# ohAgent — Quick Start (Deployed)

> **Status:** ✅ Running on K8s (`ohagent` namespace)
> **Node:** em-avion-dev-8t-96m (Scaleway, Paris)
> **Version:** 0.1.0

---

## How to use (right now)

### 1. Pair with Telegram (secure admin flow)

Only the **admin** (owner) can generate pairing codes. Random users cannot self-pair.

**Admin:** send `/pair` → get a 6-character code valid for 10 minutes.

**User:** send `/pair <code>` → paired, ready to chat.

> Admin user ID is set via `OHAGENT_ADMIN_USER_ID` in ConfigMap. You can find your Telegram user ID in bot logs: `kubectl -n ohagent logs deploy/ohagent-daemon | grep "user_id"`.

### 2. Chat

Any message to the bot starts a Jcode AI session. Full coding, research, file operations — all backed by DeepSeek V4-Flash.

### 3. Commands

| Command | What it does |
|---|---|
| Any message | Jcode agent session (coding, research, files) |
| `/pair` | (Admin only) Generate pairing code for new user |
| `/pair <code>` | Confirm pairing code and activate |
| `/ocr` + photo | Extract receipts via Gemini — FREE, 4s |
| `/skills` | List learned skills with quality scores |
| `/skill <name>` | Skill detail + usage stats |
| `/remember <text>` | Save to persistent memory |
| `/recall <query>` | Search memory |
| `/new` | Fresh session (memory preserved) |
| `/lang` | Toggle EN → LV → RU |
| `/help` | Show all commands |
| `/model` | Show/set model preferences |
| `/sandbox compile-java repo=<url>` | External VM for GraalVM native-image compilation |

### 4. Sandbox (external VMs)

Provision isolated VMs on-demand for heavy workloads. VMs are firewalled
from the main server — they **cannot** modify ohAgent configuration.

| Command | VM | Cost | Use case |
|---|---|---|---|
| `/sandbox compile-java repo=<url>` | CPX41 (8 vCPU, 16 GB) | €0.022/hr | GraalVM native-image |
| `/sandbox rust-build repo=<url>` | CPX41 (8 vCPU, 16 GB) | €0.022/hr | cargo build --release |
| `/sandbox k3s-test cmd=<cmd>` | CPX51 (16 vCPU, 32 GB) | €0.048/hr | K8s cluster test |
| `/sandbox run cmd=<cmd>` | CPX41 (8 vCPU, 16 GB) | €0.022/hr | Any shell command |

```text
# Example
/sandbox compile-java repo=https://github.com/user/spring-app ttl=30m

# Result: VM created, GraalVM installed, repo cloned, native-image runs
# VM auto-destroyed after TTL (or /sandbox destroy <job_id>)
```

See [SANDBOX-SERVERS.md](SANDBOX-SERVERS.md) for full architecture and isolation guarantees.

### 5. REST API

```bash
# Health (no auth)
curl http://51.159.106.193:30090/health

# Status (needs API key)
curl -H "X-API-Key: $OHAGENT_API_KEY" \
  http://51.159.106.193:30090/api/status
```

API key is auto-generated at startup. Find it in K8s logs:
```bash
kubectl -n ohagent logs deploy/ohagent-daemon | grep "generated random key"
```

---

## Architecture

```text
Telegram → ohAgent Daemon (K8s pod)
              ├── Jcode sessions (DeepSeek V4-Flash)
              ├── Memory (SQLite + vector)
              ├── Skills (auto-learns)
              ├── Gemini OCR (receipts, FREE)
              └── Version checker (daily)
```

### What's running

| Component | Status | Details |
|---|---|---|
| Daemon | ✅ Running | 1 replica, 128Mi/100m |
| Telegram bot | ✅ Active | Long-polling mode |
| Provider | ✅ DeepSeek | V4-Flash (€0.14/M) |
| Model router | ✅ | 16 models tracked |
| Memory engine | ✅ | SQLite WAL |
| Skills engine | ✅ | Cron every 5/10min |
| Gemini OCR | ✅ | Attached to Telegram |
| Version checker | ✅ | Daily, push notifications |
| Health check | ✅ | `:9090/health` |
| Prometheus | ✅ | `:9090/metrics` |
| Security guard | ✅ | 5-layer defense |

---

## Security model

The agent **cannot** modify its own configuration — regardless of whether
the request comes from Telegram, REST API, or any other channel.

### 5-layer defense

| Layer | What it blocks |
|---|---|
| **1. NetworkPolicy** | Pod cannot reach K8s API server at all |
| **2. No automount token** | `automountServiceAccountToken: false` — zero API access |
| **3. Zero capabilities** | `CapEff: 0000000000000000` — no root, no ptrace |
| **4. Code guard** | `security_guard.rs` blocks: `kubectl apply/delete/patch`, `systemctl`, `docker run`, `export KEY=...`, writes to `/home/jcode/.ohagent`, `helm`, `strace/gdb` |
| **5. readOnlyRootFilesystem** | Container FS is read-only — no binary injection |

### How to make changes

All configuration changes must be made **from outside the pod**:

```bash
# Change ConfigMap
kubectl -n ohagent edit configmap ohagent-config

# Or use a CI/CD pipeline / external agent
kubectl -n ohagent set env deploy/ohagent-daemon \
  OHAGENT_ADMIN_USER_ID=123456

# Restart to apply
kubectl -n ohagent rollout restart deploy/ohagent-daemon
```

## How to deploy from scratch

### Prerequisites
- Docker + K8s cluster
- Scaleway Container Registry access
- Telegram bot token from @BotFather
- DeepSeek API key

### Deploy

```bash
# 1. Clone
git clone --recurse-submodules https://github.com/orangehat/ohAgent.git
cd ohAgent

# 2. Build & push
DOCKER_BUILDKIT=1 docker build \
  -t rg.pl-waw.scw.cloud/orangehat/ohagent-daemon:stable .
docker push rg.pl-waw.scw.cloud/orangehat/ohagent-daemon:stable

# 3. Create K8s resources
kubectl create ns ohagent

# 4. Create ConfigMap with your keys
kubectl -n ohagent create configmap ohagent-config \
  --from-literal=RUST_LOG="info,ohagent=debug" \
  --from-literal=DEEPSEEK_API_KEY="sk-YOUR-KEY" \
  --from-literal=TELEGRAM_BOT_TOKEN="123:your-token" \
  --from-literal=SF_API_KEY="sk-..." \
  --from-literal=GOOGLE_API_KEY="..." \
  --from-literal=OPENAI_API_KEY="sk-..." \
  --from-literal=ZAI_API_KEY="..." \
  --from-literal=SCW_SECRET_KEY="..." \
  --from-literal=SCW_PROJECT_ID="..." \
  --from-literal=VAULT_ADDR="http://vault:8200" \
  --from-literal=VAULT_KV_PATH="secret" \
  --from-literal=OHAGENT_ADMIN_USER_ID="YOUR_TELEGRAM_ID"

# 5. Create registry secret
kubectl -n ohagent create secret docker-registry registry-scaleway \
  --docker-server=rg.pl-waw.scw.cloud \
  --docker-username=YOUR_USER \
  --docker-password=YOUR_PASSWORD

# 6. Deploy
kubectl -n ohagent apply -f - <<'EOF'
apiVersion: apps/v1
kind: Deployment
metadata:
  name: ohagent-daemon
spec:
  replicas: 1
  selector:
    matchLabels:
      app: ohagent
  template:
    metadata:
      labels:
        app: ohagent
    spec:
      imagePullSecrets:
        - name: registry-scaleway
      containers:
        - name: daemon
          image: rg.pl-waw.scw.cloud/orangehat/ohagent-daemon:stable
          imagePullPolicy: Always
          ports:
            - containerPort: 9090
          envFrom:
            - configMapRef:
                name: ohagent-config
          resources:
            requests: {memory: "128Mi", cpu: "100m"}
            limits: {memory: "1Gi", cpu: "1000m"}
          livenessProbe:
            httpGet: {path: /health, port: 9090}
            initialDelaySeconds: 30
            periodSeconds: 30
          readinessProbe:
            httpGet: {path: /health, port: 9090}
            initialDelaySeconds: 10
            periodSeconds: 10
EOF

# 7. Verify
kubectl -n ohagent get pods
kubectl -n ohagent logs deploy/ohagent-daemon --tail=20
kubectl -n ohagent exec deploy/ohagent-daemon -- curl -s :9090/health
```

### Update

```bash
cd ohAgent && git pull --recurse-submodules
DOCKER_BUILDKIT=1 docker build -t rg.pl-waw.scw.cloud/orangehat/ohagent-daemon:stable .
docker push rg.pl-waw.scw.cloud/orangehat/ohagent-daemon:stable
kubectl -n ohagent rollout restart deploy/ohagent-daemon
```

---

## Troubleshooting

### Pod CrashLoopBackOff

```bash
kubectl -n ohagent logs deploy/ohagent-daemon --tail=50
```

Common causes:
- `DEEPSEEK_API_KEY not set` → Check ConfigMap
- `Cannot start runtime from within runtime` → Fixed in latest build
- `TerminatedByOtherGetUpdates` → Kill competing bot instance
- `Insufficient cpu` → Scale down other pods or increase node

### Telegram bot not responding

1. Check token: `kubectl -n ohagent get configmap ohagent-config -o jsonpath='{.data.TELEGRAM_BOT_TOKEN}'`
2. Check logs for update errors
3. Ensure only 1 replica (multiple replicas = competing polling)

### API key lookup

```bash
kubectl -n ohagent logs deploy/ohagent-daemon | grep "generated random key"
```

### Memory/Skills data

Located in the pod at `/home/jcode/.ohagent/`:
```bash
kubectl -n ohagent exec deploy/ohagent-daemon -- ls /home/jcode/.ohagent/
```

---

## Costs

| Item | Monthly |
|---|---|
| Pod (128Mi/100m) | ~€3 (shared node) |
| DeepSeek V4-Flash | €0.14/M tokens |
| Gemini Flash (OCR) | FREE tier |
| Total (personal use) | **~€5/mo** |
