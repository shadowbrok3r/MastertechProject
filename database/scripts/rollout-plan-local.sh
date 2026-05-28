#!/usr/bin/env bash
# =============================================================================
# rollout-plan-local.sh — Generate a surrealkit rollout plan against localhost
#
# Usage (run from workspace root — MastertechProject/):
#   ./database/scripts/rollout-plan-local.sh <name>
#
# Reads DB_ROOT_USER, DB_ROOT_PASS, DB_URL_LOCAL, NS, DB from .env.
# Diffs the current local DB schema against the .surql files and writes
# a new manifest to database/rollouts/.
# =============================================================================
set -euo pipefail

ENV_FILE=".env"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
die()  { echo "ERROR: $*" >&2; exit 1; }
info() { echo ""; printf '\033[36m>>> %s\033[0m\n' "$*"; }

env_get() {
    grep -m1 "^${1}=" "$ENV_FILE" 2>/dev/null | cut -d= -f2-
}

# ---------------------------------------------------------------------------
# Arg parsing
# ---------------------------------------------------------------------------
if [[ $# -lt 1 || -z "${1:-}" ]]; then
    die "Name required. Usage: $0 <name>  (e.g. $0 slice10_embeddings)"
fi
NAME="$1"

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

info "Planning rollout '$NAME' against ws://$DB_URL_LOCAL"

surrealkit \
    --host "ws://${DB_URL_LOCAL}" \
    --ns   "$NS" \
    --db   "$DB" \
    --user "$DB_ROOT_USER" \
    --pass "$DB_ROOT_PASS" \
    rollout plan --name "$NAME"

echo ""
printf '\033[32mManifest written to database/rollouts/.\033[0m\n'
echo "Review the generated DDL, replace auto-generated statements with"
echo "custom DDL if needed (VALUE fn::embed_text, HNSW params, backfills),"
echo "then run: ./database/scripts/migrate-local.sh"
