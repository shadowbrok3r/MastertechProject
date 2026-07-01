# MastertechMobile

Dioxus (0.7.9) mobile/desktop/web client for Mastertech: technician task board
plus remote-client session control. Styled with the **AMOLED Crimson** theme
(true-black OLED base, hot pinkish-red accents) matched to the Mastertech4.0 TUI
`TuiColorScheme::amoled_crimson` scheme.

## Workspace

This crate is intentionally **excluded** from the root `MastertechProject`
workspace (its Dioxus dependency tree is kept out of `cargo build --workspace`
and CI). It is its own workspace root — see `[workspace]`,
`[workspace.dependencies]`, and the `[patch.crates-io]` egui-phosphor pin in
`Cargo.toml`. Path deps (`database`, `displays`) still belong to the root
workspace and resolve their own `workspace = true` deps there.

`surrealdb` is pinned to the workspace-root version (`3.2.0-beta.2`,
`protocol-ws`).

## Features

- `mobile` / `desktop` / `web` — selects the Dioxus render backend.
- `client-sessions` (default) — remote-client listing + control. Pulls in the
  `displays` crate to reuse its `Cmd` wire type, bincode framing, and
  `AdminTransport` (direct-TCP with WebSocket-relay fallback) so mobile speaks
  the identical protocol to the Mastertech agent. Disable for a lean bundle:
  `--no-default-features --features mobile`.

## Structure

```
src/
├─ main.rs            # App entry, auth/session, top-level layout + routing
├─ theme.rs           # ThemeConfig (AMOLED Crimson defaults)
├─ components/        # navbar, modal, toast, and vendored dioxus-primitives wrappers
├─ pages/
│  ├─ tasks.rs        # Task board (My/Store/Completed)
│  ├─ login.rs        # Login
│  └─ clients.rs      # Remote-client sessions (feature: client-sessions)
└─ services/
   ├─ tasks.rs        # Task queries/mutations
   ├─ helpers.rs      # Task list helpers
   └─ clients.rs      # ClientSession over displays' AdminTransport (feature: client-sessions)
assets/styles.css     # AMOLED Crimson stylesheet (linked at runtime)
```

## Serving

```bash
dx serve --platform desktop          # or: --platform web / android / ios
```

Native check without the heavy displays build:

```bash
cargo check --no-default-features --features desktop
```

Full check including remote sessions:

```bash
cargo check --no-default-features --features desktop,client-sessions
```
