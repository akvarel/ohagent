#!/bin/bash
# build-pii.sh — multi-platform build for ohagent-pii-redactor
# Usage: ./scripts/build-pii.sh [version]
#   version: semantic version (default: from git describe)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
VERSION="${1:-0.1.0}"

# Generate license signing secret if in production build
if [ -n "${OHAGENT_PII_SECRET:-}" ]; then
    echo "==> Using OHAGENT_PII_SECRET from environment"
else
    echo "==> DEV BUILD: no license validation (set OHAGENT_PII_SECRET for production)"
fi

echo "==> Building ohagent-pii-redactor v${VERSION}"

# Linux x86_64
echo "  -> x86_64-unknown-linux-gnu"
cargo build --release -p ohagent-pii-redactor --target x86_64-unknown-linux-gnu 2>&1 | tail -1

# Linux ARM64
echo "  -> aarch64-unknown-linux-gnu"
cargo build --release -p ohagent-pii-redactor --target aarch64-unknown-linux-gnu 2>&1 | tail -1

# macOS x86_64 (if on macOS)
if [[ "$(uname)" == "Darwin" ]]; then
    echo "  -> x86_64-apple-darwin"
    cargo build --release -p ohagent-pii-redactor --target x86_64-apple-darwin 2>&1 | tail -1
    echo "  -> aarch64-apple-darwin"
    cargo build --release -p ohagent-pii-redactor --target aarch64-apple-darwin 2>&1 | tail -1
fi

DIST_DIR="${PROJECT_DIR}/dist/pii-redactor/${VERSION}"
mkdir -p "${DIST_DIR}"

echo "==> Collecting binaries"

copy_if_exists() {
    local src="$1" dst="$2"
    if [ -f "$src" ]; then
        cp "$src" "$dst"
        echo "  -> $dst ($(du -h "$src" | cut -f1))"
    fi
}

copy_if_exists "${PROJECT_DIR}/target/x86_64-unknown-linux-gnu/release/libohagent_pii_redactor.so" \
    "${DIST_DIR}/libohagent_pii_redactor-linux-x86_64.so"
copy_if_exists "${PROJECT_DIR}/target/aarch64-unknown-linux-gnu/release/libohagent_pii_redactor.so" \
    "${DIST_DIR}/libohagent_pii_redactor-linux-aarch64.so"
copy_if_exists "${PROJECT_DIR}/target/x86_64-apple-darwin/release/libohagent_pii_redactor.dylib" \
    "${DIST_DIR}/libohagent_pii_redactor-darwin-x86_64.dylib"
copy_if_exists "${PROJECT_DIR}/target/aarch64-apple-darwin/release/libohagent_pii_redactor.dylib" \
    "${DIST_DIR}/libohagent_pii_redactor-darwin-aarch64.dylib"

# Generate license tool
echo "==> Building license generator"
cargo run --release -p ohagent-license-gen -- --help 2>/dev/null || true

echo "==> Done! Distributables in ${DIST_DIR}/"
ls -lh "${DIST_DIR}/"
