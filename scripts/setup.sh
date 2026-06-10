#!/usr/bin/env bash
# PhoneBridge setup script
# - Creates message-center runtime directories
# - Writes a sample config.toml if absent
# - Pre-creates the SQLite database file parent
# - Idempotent: safe to run multiple times

set -euo pipefail

CONFIG_DIR="${PHONEBRIDGE_CONFIG_DIR:-$HOME/.config/phonebridge}"
DATA_DIR="${PHONEBRIDGE_DATA_DIR:-$HOME/.local/share/phonebridge}"
LOG_DIR="${PHONEBRIDGE_LOG_DIR:-$DATA_DIR}"

# Resolve ~ in paths (the message-center does this too, but we mirror it here for clarity)
expand_path() {
    local p="$1"
    case "$p" in
        "~") echo "$HOME" ;;
        "~/"*) echo "$HOME/${p#~/}" ;;
        *) echo "$p" ;;
    esac
}

CONFIG_DIR="$(expand_path "$CONFIG_DIR")"
DATA_DIR="$(expand_path "$DATA_DIR")"
LOG_DIR="$(expand_path "$LOG_DIR")"

echo "PhoneBridge setup"
echo "  config dir : $CONFIG_DIR"
echo "  data dir   : $DATA_DIR"
echo "  log dir    : $LOG_DIR"
echo

mkdir -p "$CONFIG_DIR" "$DATA_DIR" "$LOG_DIR"

CONFIG_FILE="$CONFIG_DIR/config.toml"
if [ -e "$CONFIG_FILE" ]; then
    echo "[ok] config already exists: $CONFIG_FILE (not overwriting)"
else
    cat > "$CONFIG_FILE" <<'EOF'
# PhoneBridge message-center configuration
# Edit and restart the message-center after changes.

[server]
# 8443 is the HTTPS-convention port; recommended for TLS mode.
# When you run the message-center with `--no-tls` (plain HTTP, dev
# only), the binary auto-shifts from this default to "0.0.0.0:8080"
# unless you pass `--bind` explicitly. Override here to lock the
# port regardless of which mode you run in.
bind = "0.0.0.0:8443"
# Path to TLS cert and key. Auto-generated on first run if missing.
cert_path = ""
key_path  = ""

[discovery]
service_type = "_phonebridge._tcp"
enabled = true
# Optional: restrict mDNS advertisement to a specific network interface
# interface = "wlan0"

[storage]
# SQLite file path. Empty = use {data_dir}/phonebridge.db
db_path = ""

[logging]
level = "info"
file = ""
# max_log_size_mb = 10
# max_log_files  = 5
EOF
    echo "[ok] wrote sample config: $CONFIG_FILE"
fi

# A .keep file in data dir so the directory survives a fresh clone
touch "$DATA_DIR/.keep"

echo
echo "Setup complete. Next steps:"
echo "  cargo run -p message-center              # start the message-center"
echo "  open https://localhost:8443/console/     # open the web console"
