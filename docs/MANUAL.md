# ohAgent Manual

## 1. Installation

### Prerequisites

- **Rust** 1.82+ (stable toolchain)
- **Git** for submodule checkout
- **HashiCorp Vault** or Vault Agent (for secrets)
- **DeepSeek API key** (primary provider) or **Anthropic/OpenAI API key**
- **Telegram Bot Token** (for Telegram gateway)
- Systemd (Linux) or launchd (macOS) for daemon persistence

### Step-by-step Setup

```bash
# Clone with submodules
git clone --recurse-submodules https://github.com/orangehat/ohAgent.git
cd ohAgent

# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Build
cargo build --release -p ohagent-daemon
```

### Vault Setup

ohAgent reads secrets from environment variables. In production, use Vault Agent to inject them:

```bash
# Store secrets in Vault
vault kv put secret/ohagent/deepseek api_key="sk-..."
vault kv put secret/ohagent/telegram bot_token="123456:ABC-DEF..."

# Vault Agent template (vault-agent.hcl):
template {
  destination = "/tmp/ohagent.env"
  contents = <<EOF
DEEPSEEK_API_KEY={{ with secret "secret/ohagent/deepseek" }}{{ .Data.data.api_key }}{{ end }}
TELEGRAM_BOT_TOKEN={{ with secret "secret/ohagent/telegram" }}{{ .Data.data.bot_token }}{{ end }}
EOF
}

# Source the env file before starting:
source /tmp/ohagent.env
```

### Quick Start (without Vault)

```bash
export DEEPSEEK_API_KEY="sk-your-key-here"
export TELEGRAM_BOT_TOKEN="123456:ABC-DEF"
cargo run --release -p ohagent-daemon
```

---

## 2. Configuration

### CLI Flags

| Flag | Default | Description |
|---|---|---|
| `--config` | `~/.ohagent/config.toml` | Path to config file |
| `--log-level` | `info` | Log level: trace, debug, info, warn, error |
| `--health-port` | `9090` | Health check HTTP port |
| `--telegram` | `true` | Enable Telegram gateway |

### Environment Variables (via Vault)

| Variable | Required | Description |
|---|---|---|
| `DEEPSEEK_API_KEY` | Yes* | DeepSeek API key |
| `ANTHROPIC_API_KEY` | Yes* | Anthropic API key (fallback) |
| `OPENAI_API_KEY` | Yes* | OpenAI API key (fallback) |
| `TELEGRAM_BOT_TOKEN` | For Telegram | Telegram Bot API token |

\* At least one provider key must be set.

### Health Check

```bash
curl http://localhost:9090/health
# {"status":"ok","service":"ohagent","version":"0.1.0"}
```

---

## 3. Usage

### Telegram Bot Commands

Once the daemon is running and the bot is connected:

| Command | Description |
|---|---|
| `/start` | Start the bot, show greeting |
| `/pair` | Generate a pairing code |
| `/confirm <CODE>` | Confirm pairing with the code |
| `/help` | Show available commands |
| `/new` | Start a fresh conversation |
| `/lang` | Cycle language: EN → LV → RU → EN |
| `/stop` | Stop the current task |
| `/status` | Check agent status |

### Pairing Flow

1. User sends `/pair` → receives a 6-character code
2. User sends `/confirm ABC123` → pairing confirmed
3. After pairing, any text message is processed by the agent

### Languages

ohAgent supports three languages from day one:
- **English** (default)
- **Latvian** (Latviešu)
- **Russian** (Русский)

Language is detected from Telegram's `language_code` and can be cycled with `/lang`.

### Multi-Tenant

Every paired user gets a unique `tenant_id` in format `telegram_{user_id}`. All agent interactions are scoped to this tenant.

---

## Deep Memory System (Phase 3)

ohAgent remembers conversations across sessions. The memory engine stores:
- **Conversation summaries** — key points and decisions from each session
- **Semantic search** — find relevant past context via meaning, not keywords
- **Proactive nudges** — the agent suggests relevant memories when they're useful

Memory is stored in SQLite at `~/.ohagent/memory.db` by default.

### Embeddings (optional)

For best results, enable embeddings:
```bash
cargo build --features ohagent-memory/embeddings
```
This loads Jcode's ONNX model (all-MiniLM-L6-v2, ~90MB download on first run).
Without embeddings, text-based keyword matching is used as a fallback.

### Memory Commands (planned for Telegram bot)

| Command | Description |
|---|---|
| `/remember <text>` | Explicitly save something to memory |
| `/recall <query>` | Search past memories |
| `/forget` | Clear memory for the current chat |

---

## 4. Troubleshooting

### Common Errors

**`TELEGRAM_BOT_TOKEN not set`**
→ Set the env var or disable Telegram with `--telegram=false`.

**`DEEPSEEK_API_KEY not set` / provider not configured**
→ Export your API key or set it via Vault.

**Bot not responding**
→ Check logs: `RUST_LOG=debug cargo run -p ohagent-daemon`
→ Verify the bot token is valid via `https://api.telegram.org/bot<TOKEN>/getMe`

**"No tokens/providers left" error**
→ The provider couldn't authenticate. Check your API key is valid and the Jcode submodule is at the correct commit.

**Compilation errors**
→ Ensure submodules are initialized: `git submodule update --init --recursive`
→ The Jcode fork (`akvarel/jcode`) must be at the commit tracked by ohAgent.

### Log Locations

- All logs go to **stdout/stderr** (structured with `tracing-subscriber`)
- Systemd journal: `journalctl -u ohagent -f`
- Log level can be increased: `--log-level debug` or `RUST_LOG=debug`

### Recovery

```bash
# Restart the daemon
systemctl restart ohagent

# Rebuild from clean
cargo clean && cargo build --release -p ohagent-daemon

# Check provider connectivity
curl -H "Authorization: Bearer $DEEPSEEK_API_KEY" https://api.deepseek.com/user/balance
```
