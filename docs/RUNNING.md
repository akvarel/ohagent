# ohAgent — Quick Start (Deployed)

> **Status:** ✅ Running on K8s (`ohagent` namespace)
> **Node:** em-avion-dev-8t-96m (Scaleway, Paris)
> **Version:** 0.1.0

---

## How to use (right now)

### 1. Pair with Telegram

Open Telegram and send `/start` to your bot:
```
https://t.me/your_bot_username
```
The bot generates a 6-character pairing code. Pick it up and confirm with `/pair <code>`.

### 2. Chat

Any message to the bot starts a Jcode AI session. Full coding, research, file operations — all backed by DeepSeek V4-Flash.

### 3. Commands

| Command | What it does |
|---|---|
| Any message | Jcode agent session (coding, research, files) |
| `/ocr` + photo | Extract receipts via Gemini — FREE, 4s |
| `/skills` | List learned skills with quality scores |
| `/skill <name>` | Skill detail + usage stats |
| `/remember <text>` | Save to persistent memory |
| `/recall <query>` | Search memory |
| `/new` | Fresh session (memory preserved) |
| `/lang` | Toggle EN → LV → RU |
| `/help` | Show all commands |
| `/model` | Show/set model preferences |

### 4. REST API

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

---

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
  --from-literal=VAULT_KV_PATH="secret"

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
