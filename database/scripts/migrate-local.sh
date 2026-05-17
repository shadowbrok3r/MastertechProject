#!/usr/bin/env bash
# =============================================================================
# migrate-local.sh — Apply a surrealkit rollout to localhost
#
# Usage (run from workspace root — MastertechProject/):
#   ./database/scripts/migrate-local.sh [rollout-id]
#
# Reads DB_ROOT_USER, DB_ROOT_PASS, DB_URL_LOCAL, NS, DB from .env.
# If rollout-id is omitted, auto-detects the newest pending manifest.
# =============================================================================
set -euo pipefail

ROLLOUTS_DIR="database/rollouts"
ENV_FILE=".env"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
die()  { echo "ERROR: $*" >&2; exit 1; }
info() { echo ""; echo ">>> $*"; }

# Read a single KEY from the .env file (returns empty string if not found)
env_get() {
    grep -m1 "^${1}=" "$ENV_FILE" 2>/dev/null | cut -d= -f2-
}

# ---------------------------------------------------------------------------
# Pre-flight
# ---------------------------------------------------------------------------
[[ -f "surrealkit.toml" ]] || die "Run this script from the workspace root (MastertechProject/)"
[[ -f "$ENV_FILE"       ]] || die ".env not found at workspace root"

if ! command -v surrealkit &>/dev/null; then
    die "surrealkit not found. Install with: cargo binstall surrealkit"
fi

# ---------------------------------------------------------------------------
# Load connection values from .env
# ---------------------------------------------------------------------------
DB_ROOT_USER=$(env_get DB_ROOT_USER)
DB_ROOT_PASS=$(env_get DB_ROOT_PASS)
DB_URL_LOCAL=$(env_get DB_URL_LOCAL)
NS=$(env_get NS)
DB=$(env_get DB)

[[ -n "$DB_ROOT_USER" ]] || die "DB_ROOT_USER not set in $ENV_FILE"
[[ -n "$DB_ROOT_PASS" ]] || die "DB_ROOT_PASS not set in $ENV_FILE"
[[ -n "$DB_URL_LOCAL" ]] || die "DB_URL_LOCAL not set in $ENV_FILE"
[[ -n "$NS"           ]] || die "NS not set in $ENV_FILE"
[[ -n "$DB"           ]] || die "DB not set in $ENV_FILE"

CONN_ARGS=(
    --host "ws://${DB_URL_LOCAL}"
    --ns   "$NS"
    --db   "$DB"
    --user "$DB_ROOT_USER"
    --pass "$DB_ROOT_PASS"
)

# ---------------------------------------------------------------------------
# Resolve rollout ID
# ---------------------------------------------------------------------------
if [[ $# -ge 1 ]]; then
    ROLLOUT_ID="$1"
else
    ROLLOUT_MANIFEST=$(
        grep -rl 'state = "planned"\|state = "ready_to_complete"' "$ROLLOUTS_DIR"/*.toml 2>/dev/null \
        | sort | tail -1
    )
    if [[ -z "$ROLLOUT_MANIFEST" ]]; then
        echo "No pending rollout manifests found in $ROLLOUTS_DIR/. Nothing to do."
        exit 0
    fi
    ROLLOUT_ID=$(basename "$ROLLOUT_MANIFEST" .toml)
    echo "Auto-detected rollout: $ROLLOUT_ID"
fi

MANIFEST_PATH="$ROLLOUTS_DIR/$ROLLOUT_ID.toml"
[[ -f "$MANIFEST_PATH" ]] || die "Manifest not found: $MANIFEST_PATH"

# ---------------------------------------------------------------------------
# Read current state from manifest
# ---------------------------------------------------------------------------
CURRENT_STATE=$(grep -m1 '^state' "$MANIFEST_PATH" | sed 's/state = "\(.*\)"/\1/')

echo ""
echo "Rollout : $ROLLOUT_ID"
echo "State   : $CURRENT_STATE"
echo "Target  : ws://${DB_URL_LOCAL} / ${NS} / ${DB}"

# ---------------------------------------------------------------------------
# Run rollout
# ---------------------------------------------------------------------------
case "$CURRENT_STATE" in
    completed)
        echo "Already completed — nothing to do."
        exit 0
        ;;
    planned)
        info "Lint"
        surrealkit "${CONN_ARGS[@]}" rollout lint "$MANIFEST_PATH"

        info "Start phase"
        surrealkit "${CONN_ARGS[@]}" rollout start "$ROLLOUT_ID"
        ;;&
    ready_to_complete|running_start|planned)
        info "Complete phase"
        surrealkit "${CONN_ARGS[@]}" rollout complete "$ROLLOUT_ID"
        ;;
    *)
        die "Unexpected rollout state: '$CURRENT_STATE'. Inspect $MANIFEST_PATH manually."
        ;;
esac

echo ""
echo "=== Migration complete ==="
echo "Verify with: INFO FOR TABLE <table>;"
