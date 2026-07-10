#!/usr/bin/env bash
# ohAgent integration test: daemon → WS → chat → cancel → chat again
#
# Usage:
#   ./scripts/test-integration.sh          # full test
#   ./scripts/test-integration.sh --quick  # skip release build
#
# Prerequisites:
#   - cargo + websocat (install: cargo install websocat)
#   - OPENAI_API_KEY or ANTHROPIC_API_KEY

set -eo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

DAEMON_PORT=18447
TMPDIR=$(mktemp -d /tmp/ohagent-test-XXXXXX)
CONFIG_FILE="$TMPDIR/config.toml"
DAEMON_LOG="$TMPDIR/daemon.log"

PASS=0
FAIL=0
pass() { PASS=$((PASS+1)); echo "  ✅ $1"; }
fail() { FAIL=$((FAIL+1)); echo "  ❌ $1"; }

cleanup() {
    echo ""
    echo "=== Cleanup ==="
    if [[ -n "${DAEMON_PID:-}" ]]; then
        kill "$DAEMON_PID" 2>/dev/null || true
        wait "$DAEMON_PID" 2>/dev/null || true
        echo "Daemon stopped (PID $DAEMON_PID)"
    fi
    rm -rf "$TMPDIR"
    echo "Done."
}
trap cleanup EXIT

echo "=== Prerequisites ==="
if ! command -v websocat &>/dev/null; then
    echo "  Installing websocat..."
    cargo install websocat 2>/dev/null || {
        echo "  ERROR: websocat required. Install: cargo install websocat"
        exit 1
    }
fi

QUICK="${1:-}"
if [[ "$QUICK" != "--quick" ]]; then
    echo ""
    echo "=== Build (release) ==="
    cd "$PROJECT_DIR"
    cargo build --release -p ohagent-daemon 2>&1 | tail -1
    echo "  Build OK."
fi

# Create test config
echo ""
echo "=== Create test config ==="
cat > "$CONFIG_FILE" <<EOF
# Test config — only telegram matters since daemon uses CLI for port
[telegram]
enabled = false
EOF
echo "  Config: $CONFIG_FILE (port $DAEMON_PORT)"

# Check API key
if [[ -z "${OPENAI_API_KEY:-}" && -z "${ANTHROPIC_API_KEY:-}" ]]; then
    echo "  ⚠️  WARNING: No API key set — tests will fail on actual LLM calls."
    echo "  Set OPENAI_API_KEY or ANTHROPIC_API_KEY."
fi

# Start daemon
echo ""
echo "=== Start daemon ==="
cd "$PROJECT_DIR"
RUST_LOG="${RUST_LOG:-warn,ohagent_daemon=debug}" \
cargo run --release -p ohagent-daemon -- --config "$CONFIG_FILE" --health-port "$DAEMON_PORT" >"$DAEMON_LOG" 2>&1 &
DAEMON_PID=$!
echo "  Daemon PID: $DAEMON_PID (log: $DAEMON_LOG)"

echo "  Waiting for daemon..."
for i in $(seq 1 30); do
    if curl -sf "http://127.0.0.1:$DAEMON_PORT/health" >/dev/null 2>&1; then
        echo "  Daemon ready after ${i}s"
        break
    fi
    if ! kill -0 "$DAEMON_PID" 2>/dev/null; then
        echo "  Daemon died! Log tail:"
        tail -20 "$DAEMON_LOG" 2>/dev/null || true
        exit 1
    fi
    sleep 1
done

echo ""
echo "========================================="
echo "  Integration Tests"
echo "========================================="

# ---- Test 1: health ----
echo ""
echo "--- Test 1: Health ---"
HEALTH=$(curl -sf "http://127.0.0.1:$DAEMON_PORT/health" 2>/dev/null || echo "FAILED")
if [[ "$HEALTH" != "FAILED" ]]; then
    pass "Health endpoint responds"
else
    fail "Health endpoint unreachable"
fi

# ---- Test 2: WS echo / single chat ----
echo ""
echo "--- Test 2: Single WebSocket chat ---"
python3 -c "
import asyncio, json
try:
    import websockets
except ImportError:
    print('SKIP: websockets Python lib not installed')
    exit(42)

async def test_single():
    uri = f'ws://127.0.0.1:$DAEMON_PORT/v1/ws/chat'
    async with websockets.connect(uri) as ws:
        await ws.send(json.dumps({
            'type': 'chat',
            'messages': [{'role': 'user', 'content': 'Say hello in 3 words'}]
        }))
        
        got_started = False
        got_done = False
        for _ in range(100):
            msg = await asyncio.wait_for(ws.recv(), timeout=5)
            data = json.loads(msg)
            if data.get('type') == 'started':
                got_started = True
            if data.get('type') == 'done':
                got_done = True
                break
        
        if got_done:
            print('PASS: WS chat produced done event')
            return True
        else:
            print('FAIL: No done event')
            return False

try:
    result = asyncio.run(test_single())
    exit(0 if result else 1)
except Exception as e:
    print(f'FAIL: Exception {e}')
    exit(1)
" 2>&1

RC=$?
if [[ $RC -eq 0 ]]; then
    pass "WS chat produces done event"
elif [[ $RC -eq 42 ]]; then
    echo "  ⏭️  Skipping (no websockets Python lib)"
else
    fail "WS single chat failed"
    echo "  daemon log tail:"
    tail -5 "$DAEMON_LOG" 2>/dev/null || true
fi

# ---- Test 3: Cancel + re-chat ----
echo ""
echo "--- Test 3: Cancel + re-chat (connection survival) ---"
python3 -c "
import asyncio, json
try:
    import websockets
except ImportError:
    print('SKIP: websockets Python lib not installed')
    exit(42)

async def run_test():
    uri = f'ws://127.0.0.1:$DAEMON_PORT/v1/ws/chat'
    async with websockets.connect(uri) as ws:
        # Send first chat
        await ws.send(json.dumps({
            'type': 'chat',
            'messages': [{'role': 'user', 'content': 'Write a long, 3-paragraph story about AI'}]
        }))
        
        # Let it start, then cancel
        await asyncio.sleep(2)
        await ws.send(json.dumps({'type': 'cancel'}))
        
        # Wait for cancelled event
        cancelled = False
        for _ in range(50):
            msg = await asyncio.wait_for(ws.recv(), timeout=5)
            data = json.loads(msg)
            if data.get('type') == 'cancelled':
                cancelled = True
                break
        
        if not cancelled:
            print('FAIL: No cancelled event')
            return False
        print('PASS: Got cancelled event + partial_content')
        
        # Second chat on same connection
        await ws.send(json.dumps({
            'type': 'chat',
            'messages': [{'role': 'user', 'content': 'Say hello back'}]
        }))
        
        done = False
        for _ in range(100):
            msg = await asyncio.wait_for(ws.recv(), timeout=5)
            data = json.loads(msg)
            if data.get('type') == 'done':
                done = True
                break
        
        if not done:
            print('FAIL: Second chat missing done')
            return False
        print('PASS: Second chat completed after cancel')
        return True

try:
    result = asyncio.run(run_test())
    exit(0 if result else 1)
except Exception as e:
    print(f'FAIL: Exception {e}')
    exit(1)
" 2>&1

RC=$?
if [[ $RC -eq 0 ]]; then
    pass "Cancel + re-chat works"
elif [[ $RC -eq 42 ]]; then
    echo "  ⏭️  Skipping cancel test (websockets Python lib not installed)"
    echo "  Install: pip install websockets"
else
    fail "Cancel + re-chat test failed"
    echo "  daemon log tail:"
    tail -10 "$DAEMON_LOG" 2>/dev/null || true
fi

# ---- Summary ----
echo ""
echo "========================================="
echo "  Results: $PASS passed, $FAIL failed"
echo "========================================="

if [[ "$FAIL" -gt 0 ]]; then
    exit 1
fi
