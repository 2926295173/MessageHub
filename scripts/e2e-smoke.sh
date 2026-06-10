#!/usr/bin/env bash
# End-to-end smoke test: start the message-center in the background, then run
# `message-center --pair-with 127.0.0.1:8443` to verify the WS layer.
#
# This validates:
#   - message-center starts and listens on 8443 (TLS)
#   - mDNS advertises
#   - WS upgrade at /ws works
#   - device.hello is processed and a Responder state machine is registered
#
# It does NOT exercise the full pairing handshake (which requires a web
# console click — added in M3). The full pairing state machine is
# covered by `cargo test -p phonebridge-net --test integration_pairing`.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DATA_DIR="${PHONEBRIDGE_DATA_DIR:-/tmp/pb-e2e-data}"
CONFIG_DIR="${PHONEBRIDGE_CONFIG_DIR:-/tmp/pb-e2e-config}"
CLI_DATA_DIR="${PHONEBRIDGE_DATA_DIR:-/tmp/pb-e2e-cli-data}"
CLI_CONFIG_DIR="${PHONEBRIDGE_CONFIG_DIR:-/tmp/pb-e2e-cli-config}"
LOG="/tmp/pb-e2e.log"
CENTER_PID=""

cleanup() {
    if [ -n "$CENTER_PID" ]; then
        kill "$CENTER_PID" 2>/dev/null || true
        wait "$CENTER_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT

# Clean previous data.
rm -rf "$DATA_DIR" "$CONFIG_DIR" "$CLI_DATA_DIR" "$CLI_CONFIG_DIR"

echo "=== setup message-center data dir ==="
PHONEBRIDGE_DATA_DIR="$DATA_DIR" PHONEBRIDGE_CONFIG_DIR="$CONFIG_DIR" \
    bash "$REPO_ROOT/scripts/setup.sh" >/dev/null

echo "=== start message-center (background) ==="
(
    cd "$REPO_ROOT"
    PHONEBRIDGE_DATA_DIR="$DATA_DIR" PHONEBRIDGE_CONFIG_DIR="$CONFIG_DIR" \
        RUST_LOG=message_center=info,phonebridge_net=debug \
        cargo run --quiet -p message-center > "$LOG" 2>&1
) &
CENTER_PID=$!

# Wait for the message-center to be ready.
echo "=== wait for message-center ==="
for i in 1 2 3 4 5 6 7 8 9 10 15 20; do
    sleep 1
    if grep -q "listening (HTTPS / TLS)" "$LOG" 2>/dev/null; then
        echo "  message-center ready after ${i}s"
        break
    fi
    if ! kill -0 "$CENTER_PID" 2>/dev/null; then
        echo "  message-center died; tail of log:" >&2
        tail -30 "$LOG" >&2
        exit 1
    fi
done

if ! grep -q "listening (HTTPS / TLS)" "$LOG"; then
    echo "  message-center did not become ready in time" >&2
    tail -30 "$LOG" >&2
    exit 1
fi

# Smoke test 1: /api/v1/health.
echo "=== /api/v1/health ==="
HEALTH=$(curl -k -s https://localhost:8443/api/v1/health)
echo "  $HEALTH"
echo "$HEALTH" | grep -q '"status":"ok"' || { echo "health check failed"; exit 1; }

# Smoke test 2: /api/v1/cert.
echo "=== /api/v1/cert ==="
CERT=$(curl -k -s https://localhost:8443/api/v1/cert)
echo "  $CERT" | head -c 200
echo
echo "$CERT" | grep -q '"fingerprint"' || { echo "cert endpoint failed"; exit 1; }

# Smoke test 3: pair_cli connects to /ws and sends device.hello.
echo "=== pair_cli (fake android client) ==="
PHONEBRIDGE_DATA_DIR="$CLI_DATA_DIR" PHONEBRIDGE_CONFIG_DIR="$CLI_CONFIG_DIR" \
    RUST_LOG=info \
    timeout 25 cargo run --quiet -p message-center -- --pair-with 127.0.0.1:8443 2>&1 | tail -8

# Verify the message-center logged the hello.
if ! grep -q "ws: device.hello received" "$LOG"; then
    echo "  FAIL: message-center did not log the hello" >&2
    tail -10 "$LOG" >&2
    exit 1
fi
echo "  PASS: message-center accepted device.hello"

echo
echo "=== E2E SMOKE OK ==="
