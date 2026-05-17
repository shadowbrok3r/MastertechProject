# Database Migration Guide

Schema changes are managed via [surrealkit](https://github.com/surrealdb/surrealkit) rollouts.

## File layout

| Path | Purpose | Commit? |
|---|---|---|
| `database/schema/*.surql` | Desired schema state — source of truth | ✅ always |
| `database/rollouts/*.toml` | Rollout manifests (auto-generated or hand-authored) | ✅ always |
| `database/snapshots/` | surrealkit state snapshots | ✅ always |
| `surrealkit.toml` | Project config (workspace root) | ✅ always |

Connection credentials are read from `.env` at the workspace root — never commit secrets here.

---

## Setup

```powershell
# Install surrealkit (one-time)
cargo binstall surrealkit
```

---

## Running a migration

Use the scripts in `database/scripts/`. Always run from the **workspace root** (`MastertechProject/`).
Credentials are pulled automatically from `.env`.

| Script | OS | Target |
|---|---|---|
| `database/scripts/migrate-local.ps1` | Windows | localhost |
| `database/scripts/migrate-local.sh`  | Linux / WSL | localhost |
| `database/scripts/migrate-prod.ps1`  | Windows | production |
| `database/scripts/migrate-prod.sh`   | Linux / WSL | production |

```powershell
# Apply the latest pending rollout to localhost (Windows):
.\database\scripts\migrate-local.ps1

# Apply the latest pending rollout to production (Windows):
.\database\scripts\migrate-prod.ps1

# Target a specific rollout ID instead of auto-detecting:
.\database\scripts\migrate-local.ps1 -RolloutId 20260516120000__tranche1_relax_fks
./database/scripts/migrate-local.sh  20260516120000__tranche1_relax_fks
```

The production scripts prompt for confirmation before touching anything.
All scripts are state-aware: they skip completed rollouts and resume `ready_to_complete` without re-running `start`.

---

## Workflow: making a schema change

```
1. Edit database/schema/*.surql  ← desired state, not migration SQL
        │
        ▼
2. surrealkit rollout plan --name <name>
        │  generates database/rollouts/<timestamp>__<name>.toml
        │  (if surrealkit refuses — non-additive change — hand-author the manifest)
        │
        ▼
3. Review the manifest
        │  open the .toml and verify the DDL looks right
        │  non-additive DDL must use DEFINE ... OVERWRITE (see below)
        │
        ▼
4. Test on localhost
        │  .\database\scripts\migrate-local.ps1
        │  verify with INFO FOR TABLE <table>;
        │
        ▼
5. Commit schema files + manifest + snapshot
        │
        ▼
6. Apply to production
           .\database\scripts\migrate-prod.ps1
```

**Deploy order:** schema migration first, then app deploy. Old code + new schema = safe. New code + old schema = risky.

---

## Critical: DEFINE ... OVERWRITE in rollout DDL

Rollout manifest DDL runs **verbatim**. Without `OVERWRITE`, SurrealDB returns
"The field already exists" for any previously-managed entity — and surrealkit
**swallows that error** and marks the rollout completed anyway. The schema
silently doesn't change.

**Rule:** every `DEFINE FIELD`, `DEFINE INDEX`, and `DEFINE TABLE` in a
`[start]` or `[complete]` DDL block that touches an existing entity must use
the `OVERWRITE` keyword. Schema `.surql` files do **not** need it (surrealkit
handles that internally for `sync`).

```toml
[complete]
ddl = """
DEFINE FIELD OVERWRITE customer ON computer TYPE none | record<customer> PERMISSIONS FULL;
DEFINE INDEX OVERWRITE strun_computer ON stress_test_run FIELDS computer;
"""
```

---

## Manual rollout manifests

surrealkit auto-generates manifests for **additive** changes (new tables, new
fields). For anything that modifies an existing managed entity it refuses with:

> "automatic rollout planning refuses modified managed entities: …"

In that case, hand-author the manifest. Required fields:

```toml
id = "<timestamp>__<name>"          # matches filename
name = "<name>"
state = "planned"
source_schema_hash = "<hash>"       # metadata only — not validated
target_schema_hash = "<hash>"       # must match hash of current schema files
compatibility = "none"              # or "backward" / "forward"

[start]
ddl = ""   # additive-only DDL, or empty

[complete]
ddl = """
-- modification DDL here, with OVERWRITE
"""
```

Get the correct `target_schema_hash` by running:
```powershell
surrealkit rollout lint database/rollouts/<manifest>.toml
# The hash appears in the error message if it mismatches
```

---

## Baseline snapshot

After setting up surrealkit on a new environment, capture a baseline before
creating any rollouts:

```powershell
# Run from workspace root, with connection flags for the target environment
surrealkit --host ws://localhost:8000 --ns Mastertech --db MastertechDB \
           --user <user> --pass <pass> \
           rollout baseline
```

This writes `database/snapshots/schema_snapshot.json`. Commit it.

---

## surrealkit gotchas

| Symptom | Cause | Fix |
|---|---|---|
| Rollout "completed" but schema unchanged | DDL missing `OVERWRITE` | Re-apply with `DEFINE ... OVERWRITE` directly in Surrealist |
| `target_schema_hash` mismatch on lint | Any `.surql` file changed (even a comment) since last lint | Re-run lint, copy new hash into manifest |
| VIEW fails to sync (`table X does not exist`) | Alphabetical file order: view processed before its source table | Add a bare `DEFINE TABLE <source>` dependency guard at top of view file |
| `rollout plan` refuses changes | Non-additive modifications detected | Hand-author the manifest (see above) |
| `rollout start` shows "ready to complete" immediately | `[start]` DDL was empty or all-additive — expected | Run `rollout complete` next |
