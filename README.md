# MastertechProject

Rust workspace for **PC Laptops** shop tooling: the Mastertech desktop client (tech bench machines), MtechServer web UI ([master-tech.app](https://master-tech.app)), SurrealDB schema, remote-control backends, QC / PXE / UEFI helpers, WASM plugins, and MCP/AI integration.

Download the latest Windows `.exe` from [master-tech.app](https://master-tech.app) (served by MtechServer). For technicians new to the app, see [docs/technician-quickstart-guide.md](docs/technician-quickstart-guide.md).

---

## System map

```text
┌─────────────────────┐     WS :8081      ┌──────────────────────┐
│  MasterTech.exe     │◄─────────────────►│  websocket_server2   │  room relay
│  (Mastertech4.0)    │                   │  (admin ↔ client)    │
│  + MCP :9003/:9004  │                   └──────────────────────┘
│  + displays UI/MCP  │
└─────────┬───────────┘                   ┌──────────────────────┐
          │ HTTP API                      │  axum_server :8082   │  REST / QC /
          ├──────────────────────────────►│  orders, builds, …   │  firmware, …
          │                               └──────────┬───────────┘
          │                                          │
          │                               ┌──────────▼───────────┐
          └──────────────────────────────►│  SurrealDB           │
                                          │  (database crate +   │
┌─────────────────────┐                   │   schema/rollouts)   │
│  MtechServer2.0     │  static :8080     └──────────────────────┘
│  (WASM / Trunk)     │◄── mtechserver image
└─────────────────────┘

┌─────────────────────┐   build_job LIVE  ┌──────────────────────┐
│  plugin_builder     │◄─────────────────►│  SurrealDB           │
│  (WASM compile)     │                   └──────────────────────┘
└─────────────────────┘

Bench / dead-box path:  pxe-bench → WinPE/MasterTech  |  uefi-app → preboot-relay → axum
```

| Port / endpoint | Service |
|-----------------|---------|
| `:8080` | MtechServer (web UI) |
| `:8081/websocket` | Admin ↔ client room relay |
| `:8082` | axum REST / QC / build / firmware |
| `:9003` | Mastertech MCP (raw TCP stream) |
| `:9004/mcp` | Mastertech MCP (Streamable HTTP — use this for Cursor) |

---

## How to find things

| I need… | Look here |
|---------|-----------|
| Desktop app entry / tabs (TUR, Scripts, Minidump, …) | `Mastertech4.0/src/` — `main.rs`, `tabs/`, `terminal_mode/` |
| Shared egui tabs, modals, MCP tools, plugins host | `displays/src/` — especially `tabs/`, `mcp/`, `plugins/` |
| SurrealDB table schemas | `database/schema/*.surql` |
| Rust types / DB helpers for those tables | `database/src/schema/` |
| Apply schema changes | `database/rollouts/` + `database/scripts/` — see `database/MIGRATIONS.md` |
| HTTP API routes | `axum_server/src/routes/api/` |
| WebSocket relay | `websocket_server2/src/main.rs` |
| Wire protocol / tunnel constants | `tcp_protocol/` |
| Stressors + sensors | `stress-kit/` |
| Stress run lifecycle + Surreal persistence | `stress-runner/` |
| QC bench app | `qc-app/src/` |
| Kernel dump triage (pure Rust) | `dump-triage/` |
| Author a WASM plugin | `mtech-plugin-sdk/` + `plugins.md` |
| Example / local plugin crates | `plugins/` |
| Remote WASM compile worker | `plugin_builder/` |
| PXE netboot into Mastertech PE | `pxe-bench/` (+ `PXE.md`) |
| UEFI pre-OS diagnostic app | `uefi/` |
| HTTP→HTTPS relay for firmware (no TLS in UEFI) | `preboot-relay/` |
| Web UI (Trunk / WASM) | `MtechServer2.0/` |
| Mobile / Dioxus client | `MastertechMobile/` (own workspace) |
| Plugin / MCP architecture deep dive | `plugins.md` |
| Agent skill for shop MCP workflows | `.cursor/skills/mastertech-mcp-service/` |
| Surrealkit schema skill | `.claude/skills/surrealkit/` |

**Build selection:** do not comment crates in/out of `Cargo.toml`. Use `cargo build -p <crate>` / `cargo run -p <crate>`. Workspace `default-members` excludes the wasm-only `MtechServer2.0` so native `cargo check` stays clean.

---

## Workspace crates (Cargo members)

### Applications

#### `Mastertech4.0/` — package `MasterTech`
Primary **desktop client** techs run on shop / customer machines (egui + optional ratatui terminal mode).

| Path | Purpose |
|------|---------|
| `src/main.rs` | Process entry; registers plugins, starts MCP servers |
| `src/tabs/` | Client-side dock tabs: TUR sheet, Scripts, Minidump, File Browser, QC, Resource Mon, Stress, Part Order, … |
| `src/terminal_mode/` | Full TUI mode (ratatui): tabs, modals, websockets, widgets |
| `src/filesystem/` | Local machine identity / system info / OA serial helpers |
| `src/data/` | Inbound DB / GitHub / PrestaShop receive paths |
| `src/utilities/` | AI helpers, crypto, scripts runner, Windows helpers, restart |
| `src/tcp_listener.rs`, `transport.rs`, `tunnel_session.rs`, `remote_desktop.rs` | Admin connectivity / remote control |
| `mcp-proxy/` | Node stdio proxy that keeps Cursor/Claude MCP alive across Mastertech restarts (`:9004`) |
| `docker/` | Reproducible Linux (`linux-x64`) and Windows container build images |
| `BUILD.md`, `build-with-skia.ps1` | Native / skia-render build notes |

#### `MtechServer2.0/` — package `mtechserver`
**Web frontend** for [master-tech.app](https://master-tech.app) — Trunk + `wasm32-unknown-unknown`. Shares UI logic via `displays` (wasm feature). Build with Trunk, not bare workspace `cargo build`.

| Path | Purpose |
|------|---------|
| `src/mtechserver.rs`, `lib.rs`, `app_state.rs` | WASM app shell |
| `src/workers/` | Web workers for deser / background work |
| `Trunk.toml`, `index.html`, `assets/` | Trunk bundling |
| `Dockerfile.prod`, `k3s.yaml` | Production deploy |

#### `qc-app/` — package `qc_app`
Dedicated **QC / new-build** bench app: provisioning, driver/OS steps, stress panel, fleet client, MCP surface, reporting.

| Path | Purpose |
|------|---------|
| `src/provisioning/` | Manifests, drivers, software, OS config, vendor steps |
| `src/terminal_mode/` | TUI for QC flow |
| `src/hw_monitor/`, `hw_sampler.rs`, `telemetry.rs` | Live hardware sampling |
| `src/stress_panel.rs`, `qc_benchmark.rs` | Stress / benchmark UI |
| `src/mcp.rs`, `fleet_client.rs` | MCP + fleet registration |
| `src/reporting.rs`, `report_view.rs`, `checklist_*` | QC reports / checklists |

#### `MastertechMobile/` — package `mastertech-mobile` (**excluded** from root workspace)
Dioxus mobile/desktop/web client: task boards + optional remote-client sessions. Has its own `Cargo.toml` workspace; see its [README](MastertechMobile/README.md).

---

### Shared libraries

#### `displays/`
Largest shared crate: **egui UI**, dock tabs, modals, themes, MCP server/tools, WASM plugin host, remote egui, scripts executor. Used by MasterTech, MtechServer (wasm), and (selectively) MastertechMobile.

| Path | Purpose |
|------|---------|
| `src/tabs/` | Admin/server-side tabs: Tasks, Admin Console, Scripts, Web Console, AI Playground, KOTH, Stock, Stress Lab, Plugins, … |
| `src/plugins/` | `PluginManager`, wasmtime host, MCP bridge, remote egui, crash/driver intel hooks |
| `src/mcp/` | MCP server, tool registry, OpenAI bridge types |
| `src/ai/` | Chat / model / tool-call UI helpers |
| `src/modals/` | Task modal, AI attention, create-task, entity-link, … |
| `src/scripts/` | Script catalog categories, queue, executor, MCP channel |
| `src/ui_tools/` | Themes, icons (`icons.rs` / egui-phosphor — required for egui icons), toasts |
| `src/ui_data/` | Channel receivers for live DB/task/client/notification updates |
| `src/remote_viewer/` | Remote frame / preboot / ratatui line viewer |

#### `database/`
SurrealDB **client library** + schema-as-code.

| Path | Purpose |
|------|---------|
| `schema/*.surql` | Desired table definitions (source of truth) |
| `rollouts/*.toml` | Ordered surrealkit rollout manifests |
| `snapshots/` | surrealkit catalog/schema snapshots (commit these) |
| `migrations/` | Older / tranche SQL notes (prefer rollouts going forward) |
| `scripts/` | `migrate-local` / `migrate-prod` / `rollout-plan-*` (ps1 + sh) |
| `src/schema/` | Rust models & helpers (customer, task, computer, crash intel, plugins, stress, …) |
| `src/orders/` | Checklist / gate / PrestaShop & Shopify backends / spec check |
| `src/live_data.rs`, `clock_sync.rs` | Live queries / clock |
| `k3s/` | SurrealDB k8s PV/PVC / Helm bits |
| `MIGRATIONS.md` | How to change schema safely |

Root `surrealkit.toml` points surrealkit at this layout. Credentials come from `.env` (see `.env.example`).

#### `database-tools/`
Operational binaries against the live DB (e.g. `src/bin/audit_references.rs`).

#### `mtech-ui/`
Small shared **egui** helpers: theme, dock chrome, in-app logger, GitHub widget bits.

#### `mtech-tui/`
Shared **ratatui** infrastructure: events, widgets, styling, fx — used by Mastertech / QC terminal modes.

#### `tcp_protocol/`
Shared **wire-protocol** constants and TCP socket config for the admin↔client direct TCP path (optional tunnel / fingerprint features).

#### `stress-kit/`
CPU / memory / disk / GPU **stressor primitives** + Windows telemetry (thermal, SuperIO rails, WHEA, TDR, …).

| Path | Purpose |
|------|---------|
| `src/stressors/` | Individual stressors (cpu, memory, gpu_*, disk, …) |
| `src/telemetry/` | Sensor collectors |
| `drivers/` | Supporting driver assets (e.g. WinRing0-related) |
| `examples/` | Standalone stress demos |

#### `stress-runner/`
**Run lifecycle** on top of stress-kit: presets, QC benchmark, script catalog names, persistence into `stress_test_run` / `metric` / `event`.

| Path | Purpose |
|------|---------|
| `presets/` | Named scenario TOMLs (cert bronze→platinum, power-virus, …) |
| `src/script_catalog.rs` | Names used by Scripts tab / MCP `scripts_run*` |
| `src/controller.rs`, `runtime.rs`, `drive.rs` | Orchestration |

#### `dump-triage/`
Pure-Rust **Windows kernel dump** (BSOD) triage — header, bugcheck, module blame, optional `kdmp` full-list walk. Used by MCP `minidump_analyze` and plugins.

#### `mtech-plugin-sdk/`
Authoring SDK for **wasm32-wasip1** guest plugins (`mtech_plugin!` macro, host imports, JSON ABI). See crate docs and `plugins.md`.

#### `displays` consumers note
Icons in egui **must** come from `displays/src/ui_tools/icons.rs` (egui-phosphor) so they render with the app fonts.

---

### Backend / infra binaries

#### `axum_server/`
HTTP API on **:8082** (Docker / k3s).

| Path | Purpose |
|------|---------|
| `src/routes/api/` | `admin`, `orders`, `parts`, `build`, `firmware`, `preboot`, `qc_*`, `surreal`, scheduled jobs, … |
| `src/middleware/` | Request context / logging / recorder |
| `Dockerfile`, `k3s.yaml` | Deploy |

#### `websocket_server2/`
Room-based **WebSocket relay** on **:8081** — admin (`role=master`) ↔ client (`role=client`) binary/text fan-out; also legacy plugin_builder fallback transport.

#### `plugin_builder/`
Long-running **WASM compile worker**. Claims `build_job` rows from SurrealDB (default) or speaks legacy WS builder protocol. Registers as `connected_client` with `client_kind = build_worker`.

#### `pxe-bench/`
Bench **ProxyDHCP + TFTP + HTTP** appliance: netboot dead machines into Mastertech PE. See `PXE.md` and `deploy/`.

#### `preboot-relay/` (**excluded** workspace)
Plain HTTP → HTTPS reverse proxy so **UEFI firmware** (no TLS/DNS) can reach `axum.master-tech.app`.

#### `uefi/` (**excluded** — own toolchain/workspace)
UEFI application (`uefi-app`): boot diagnostics, stress, charts, capsule/firmware paths, wasm runtime experiments. Subcrates: `crates/ratatui-uefi`, `crates/terminput-uefi`.

---

### Plugin samples

#### `plugins/`
Local WASM plugin package trees (not always workspace members):

| Folder | Role |
|--------|------|
| `com_mastertech_driverstore` | Driver inventory / export tooling |
| `com_mastertech_screenshot` | Screenshot capture guest |
| `com_mastertech_mcp-demo-quick` | Minimal MCP demo plugin |

Published fleet plugins also live in SurrealDB `plugin_registry` (discover via MCP `search_plugins` / `list_registry_plugins`). Architecture: **`plugins.md`**.

---

## Top-level folders & files (non-crate)

| Path | Purpose |
|------|---------|
| `docs/` | Human docs: technician quickstart, MCP roadmaps, plugin-copilot reports, cowork workflows |
| `.cursor/` | Cursor rules, plans, notes, skills (`mastertech-mcp-service`) |
| `.claude/` | Claude settings + `surrealkit` skill |
| `.cargo/` | Workspace cargo config |
| `.github/workflows/` | CI / release (`release.yml` — tags `4.*`, multi-platform builds) |
| `.vscode/` | Editor tasks/settings |
| `docker-compose.yaml` | Local/stack: mtechserver, websocketserver, axumserver, pluginbuilder (+ caches) |
| `Dockerfile`, `Dockerfile.beta` | MtechServer / web image builds |
| `build-linux-compat.sh` | glibc-compatible Linux MasterTech build via container (see `Mastertech4.0/BUILD.md`) |
| `build_hash.rs` | Shared build-hash helper included by crate `build.rs` files |
| `surrealkit.toml` | Surrealkit project config (`database/schema`, rollouts, snapshots) |
| `.env` / `.env.example` | DB URLs, WS URLs, PrestaShop, Odoo, buckets, guest password, … (**never commit secrets**) |
| `plugins.md` | Plugin + MCP + remote egui design doc |
| `dev-audit.json` | Dev audit artifact |
| `target/` | Shared cargo output (gitignored / local) |

---

## Common workflows

### Run the desktop app (dev)
```powershell
cargo run -p MasterTech
# or faster iterative profile:
cargo run -p MasterTech --profile release-fast
```
Skia software renderer (WinPE / no-GL fallback): see `Mastertech4.0/BUILD.md` / `build-with-skia.ps1`.

### Apply DB schema
```powershell
# from workspace root; uses .env
.\database\scripts\migrate-local.ps1
```
Full guide: [`database/MIGRATIONS.md`](database/MIGRATIONS.md).

### Build web UI
```powershell
cd MtechServer2.0
trunk serve   # or trunk build --release
```

### Docker stack
```powershell
docker compose up --build
```
Services: web UI `:8080`, websocket `:8081`, axum `:8082`, plugin_builder.

### MCP from Cursor / Claude
1. Run MasterTech (hosts MCP on `:9004/mcp`).
2. Prefer `Mastertech4.0/mcp-proxy` as the stdio entry so reconnects survive recompiles.
3. Deep tool/workflow reference: `.cursor/skills/mastertech-mcp-service/SKILL.md` and `plugins.md`.

### Author / compile a plugin
1. Read `plugins.md` and `mtech-plugin-sdk`.
2. Prefer `search_plugins` / registry before writing a new one.
3. Local: `plugin_source` → `plugin_compile` → `plugin_deploy` (via MCP), or develop under `plugins/`.
4. No local Rust: `plugin_compile_remote` → `plugin_builder` worker.

---

## Technician onboarding (short)

1. Create an account with your PC Laptops email (store + initials). Password reset is limited — contact Logan if locked out.
2. Sign in on [master-tech.app](https://master-tech.app) and in MasterTech.exe; download the latest exe from **Downloads**.
3. Core job loop: finish a checked-in PC → fill a **TUR sheet** (recommendations for sales) → that creates a **Task** on the web with hardware / customer context.
4. Layout is egui dock tabs — drag title bars to split or stack views. TUR Sheet + File Browser are the usual starting layout.
5. Everest: service order # → Get Ticket. PrestaShop: order digits (no letters / leading zeros) → Get Prestashop.

Longer walkthrough: [docs/technician-quickstart-guide.md](docs/technician-quickstart-guide.md).

---

## Related docs

| Doc | Topic |
|-----|--------|
| [`plugins.md`](plugins.md) | Plugins, MCP ports, WASM guests, remote egui |
| [`Mastertech4.0/BUILD.md`](Mastertech4.0/BUILD.md) | Desktop build, Linux glibc, skia |
| [`database/MIGRATIONS.md`](database/MIGRATIONS.md) | Schema / surrealkit rollouts |
| [`pxe-bench/PXE.md`](pxe-bench/PXE.md) | Bench PXE appliance |
| [`Mastertech4.0/mcp-proxy/README.md`](Mastertech4.0/mcp-proxy/README.md) | Persistent MCP proxy |
| [`MastertechMobile/README.md`](MastertechMobile/README.md) | Dioxus mobile client |
| [`docs/`](docs/) | Quickstart, MCP roadmaps, plugin-copilot notes |
