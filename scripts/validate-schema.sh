#!/usr/bin/env bash
# Validate the protocol JSON Schema and run the test fixtures against it.
#
# Requires:
#   - ajv-cli globally installed (bun add -g ajv-cli@5  OR  npm i -g ajv-cli@5)
#   - ajv-formats installed locally  (bun add ajv-formats@3)
#
# Run from repo root:  bash scripts/validate-schema.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCHEMA="$REPO_ROOT/schema/protocol.schema.json"
FIX_DIR="$REPO_ROOT/scripts/test-fixtures"

# Resolve ajv-formats location. Try repo-local node_modules first, then /tmp.
if [ -d "$REPO_ROOT/frontend/node_modules/ajv-formats" ]; then
    FORMATS_DIR="$REPO_ROOT/frontend/node_modules"
elif [ -d "/tmp/node_modules/ajv-formats" ]; then
    FORMATS_DIR="/tmp/node_modules"
else
    echo "ajv-formats not found. Installing into /tmp..."
    (cd /tmp && bun add ajv-formats@3 >/dev/null 2>&1)
    FORMATS_DIR="/tmp/node_modules"
fi

# A small ajv-cli config that loads ajv-formats.
CONFIG_FILE="$(mktemp -t ajv-conf.XXXXXX.cjs)"
trap "rm -f $CONFIG_FILE" EXIT
cat >"$CONFIG_FILE" <<EOF
module.exports = (ajv) => {
    const addFormats = require('$FORMATS_DIR/ajv-formats');
    addFormats(ajv);
};
EOF

if ! command -v ajv >/dev/null 2>&1; then
    echo "ajv-cli not found. Install with:  bun add -g ajv-cli@5" >&2
    exit 1
fi

echo "Validating $SCHEMA against test fixtures in $FIX_DIR"
echo

PASS=0
FAIL=0
for f in "$FIX_DIR"/*.json; do
    name="$(basename "$f")"
    expected="valid"
    case "$name" in
        *_invalid.json) expected="invalid" ;;
    esac
    set +e
    out=$(ajv --spec=draft2019 validate -s "$SCHEMA" -d "$f" -c "$CONFIG_FILE" 2>&1)
    rc=$?
    set -e
    if [ "$expected" = "valid" ] && [ $rc -eq 0 ]; then
        echo "  PASS  $name (valid)"
        PASS=$((PASS+1))
    elif [ "$expected" = "invalid" ] && [ $rc -ne 0 ]; then
        echo "  PASS  $name (correctly rejected)"
        PASS=$((PASS+1))
    else
        echo "  FAIL  $name (expected=$expected, got exit=$rc)"
        echo "         $out"
        FAIL=$((FAIL+1))
    fi
done

echo
echo "Summary: $PASS pass, $FAIL fail"
exit $FAIL
