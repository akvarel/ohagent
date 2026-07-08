#!/bin/bash
# setup-keys.sh — interactive key setup for ohAgent
# NEVER sends keys anywhere. Writes directly to disk.
set -euo pipefail

KEYS_FILE="$HOME/.ohagent/keys.toml"
VAULT_MODE=false

# ── Help ──
if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
    cat <<EOF
ohAgent Key Setup
Usage: ./scripts/setup-keys.sh [--vault]

Options:
  --vault    Write keys to Vault instead of ~/.ohagent/keys.toml
  --help     Show this help

Key priority: Vault → env vars → ~/.ohagent/keys.toml
EOF
    exit 0
fi

if [[ "${1:-}" == "--vault" ]]; then
    VAULT_MODE=true
    if ! command -v vault &>/dev/null; then
        echo "❌ vault CLI not found. Install: https://developer.hashicorp.com/vault/downloads"
        exit 1
    fi
    if [[ -z "${VAULT_ADDR:-}" ]]; then
        echo "❌ VAULT_ADDR not set. Example: export VAULT_ADDR=http://localhost:8200"
        exit 1
    fi
fi

echo "╔══════════════════════════════════════════════╗"
echo "║        ohAgent Key Setup                     ║"
echo "║  Keys are written to DISK ONLY.              ║"
echo "║  Nothing is sent to any LLM or API.          ║"
echo "╚══════════════════════════════════════════════╝"
echo ""

if $VAULT_MODE; then
    echo "📦 Mode: Vault (${VAULT_ADDR})"
else
    echo "📄 Mode: File (${KEYS_FILE})"
fi
echo ""

# ── Provider list ──
declare -A PROVIDERS=(
    [DEEPSEEK_API_KEY]="DeepSeek — primary LLM provider (V4-Flash/Pro)"
    [SF_API_KEY]="SiliconFlow — 200+ models, cheapest API"
    [ZAI_API_KEY]="Z.ai (Zhipu international) — GLM models, api.z.ai"
    [SCW_SECRET_KEY]="Scaleway — EU/GDPR serverless + GPU (IAM secret key)"
    [SCW_PROJECT_ID]="Scaleway — Project ID (UUID: api.scaleway.ai/<id>/v1)"
    [ANTHROPIC_API_KEY]="Anthropic — Claude models (best coding quality)"
    [OPENAI_API_KEY]="OpenAI — GPT-4o, fallback"
    [GROQ_API_KEY]="Groq — fastest inference (LPU hardware)"
    [TELEGRAM_BOT_TOKEN]="Telegram Bot — messaging gateway"
    [HETZNER_API_TOKEN]="Hetzner Cloud — GPU instances"
    [GOOGLE_API_KEY]="Google AI Studio — Gemini models (best LV receipt OCR)"
)

# ── Collect keys ──
declare -A KEYS
SKIPPED=0
SET=0

for VAR in "${!PROVIDERS[@]}"; do
    DESC="${PROVIDERS[$VAR]}"

    # Check if already set in env
    CURRENT="${!VAR:-}"
    if [[ -n "$CURRENT" ]]; then
        MASKED="${CURRENT:0:8}...${CURRENT: -4}"
        echo "✅ $VAR — already in environment ($MASKED)"
        read -p "   Replace? [y/N] " REPLACE
        if [[ ! "$REPLACE" =~ ^[Yy] ]]; then
            echo "   ⏭️  Skipping"
            ((SKIPPED++)) || true
            echo ""
            continue
        fi
    fi

    # Prompt for key
    echo "🔑 $VAR — $DESC"
    read -s -p "   Key: " VALUE
    echo ""
    if [[ -z "$VALUE" ]]; then
        echo "   ⏭️  Skipped (empty)"
        ((SKIPPED++)) || true
    else
        KEYS[$VAR]="$VALUE"
        ((SET++)) || true
        echo "   ✅ Set (${#VALUE} chars)"
    fi
    echo ""
done

# ── Nothing to write ──
if [[ $SET -eq 0 ]]; then
    echo "No new keys to write. Done."
    exit 0
fi

# ── Write ──
if $VAULT_MODE; then
    echo "━━━ Writing to Vault ━━━"
    for VAR in "${!KEYS[@]}"; do
        VAULT_PATH="secret/ohagent/providers/${VAR,,}/api-key"
        # Map provider-specific paths
        case "$VAR" in
            ZAI_API_KEY)        VAULT_PATH="secret/ohagent/providers/zai/api-key" ;;
            SCW_SECRET_KEY)     VAULT_PATH="secret/ohagent/providers/scaleway/secret-key" ;;
            TELEGRAM_BOT_TOKEN) VAULT_PATH="secret/ohagent/telegram/bot-token" ;;
            HETZNER_API_TOKEN)  VAULT_PATH="secret/ohagent/providers/hetzner/api-token" ;;
        esac
        echo "   $VAULT_PATH"
        echo -n "${KEYS[$VAR]}" | vault kv put "$VAULT_PATH" api-key=-
    done
    echo "✅ $SET keys written to Vault"
else
    echo "━━━ Writing to ${KEYS_FILE} ━━━"

    # Ensure directory exists
    mkdir -p "$(dirname "$KEYS_FILE")"

    # Load existing keys if file exists
    if [[ -f "$KEYS_FILE" ]]; then
        echo "   Merging with existing keys..."
    fi

    # Collect existing keys (simple TOML parse for [keys] section)
    EXISTING=""
    if [[ -f "$KEYS_FILE" ]]; then
        # Extract [keys] section values
        EXISTING=$(awk '/^\[keys\]/{found=1;next} /^\[/{found=0} found && /=/{print}' "$KEYS_FILE" 2>/dev/null || true)
    fi

    # Build the TOML
    {
        echo "# ohAgent Provider Keys"
        echo "# Generated: $(date -u +"%Y-%m-%dT%H:%M:%SZ")"
        echo "# NEVER commit this file. It is in .gitignore by default."
        echo ""
        echo "[keys]"

        # Write existing keys first (that we're not replacing)
        if [[ -n "$EXISTING" ]]; then
            while IFS= read -r line; do
                KEY_NAME=$(echo "$line" | cut -d= -f1 | xargs)
                if [[ -z "${KEYS[$KEY_NAME]:-}" ]]; then
                    echo "$line"
                fi
            done <<< "$EXISTING"
        fi

        # Write new keys
        for VAR in "${!KEYS[@]}"; do
            echo "$VAR = \"${KEYS[$VAR]}\""
        done
    } > "$KEYS_FILE"

    chmod 600 "$KEYS_FILE"
    echo "✅ $SET keys written to $KEYS_FILE (permissions: 600)"
fi

echo ""
echo "━━━ Next Steps ━━━"
echo ""

if [[ -n "${KEYS[TELEGRAM_BOT_TOKEN]:-}" ]]; then
    echo "  1. Start daemon:  cargo run --release -p ohagent-daemon"
else
    echo "  1. Set Telegram token for bot: ./setup-keys.sh"
fi

if [[ -n "${KEYS[DEEPSEEK_API_KEY]:-}" ]]; then
    echo "  2. Verify:        RUST_LOG=info cargo run -p ohagent-daemon 2>&1 | grep -i provider"
    echo "  3. Health check:  curl http://localhost:9090/health"
fi

if [[ -n "${KEYS[SF_API_KEY]:-}" ]]; then
    echo "  4. Benchmark SF:  cargo run -p ohagent-provider-metrics -- benchmark \\"
    echo "                     --provider siliconflow --model Qwen/Qwen3-8B \\"
    echo "                     --api-key \$SF_API_KEY --api-base https://api.siliconflow.cn/v1"
fi

echo ""
echo "🔒 Keys file permissions: $(stat -c '%a' "$KEYS_FILE" 2>/dev/null || echo 'N/A')"
echo "✅ Done. Keys never left this machine."
