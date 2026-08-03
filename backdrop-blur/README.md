# backdrop-blur (MasterTech fork)

Vendored fork of [abdu-benayad/backdrop-blur](https://github.com/abdu-benayad/backdrop-blur),
forked at upstream `827bd3a2a4e8bfde7259426e289a153beb9edf63` (2026-07-19), ported to **egui 0.35**.

Real GPU frosted glass: grab a region of the live framebuffer, blur it, composite a tinted
surface back. `displays::ui_tools::glass_backdrop` is this workspace's consumer — it drives the
grab-pass path from the theme's glass parameters.

Upstream's own README is kept verbatim as [`UPSTREAM-README.md`](UPSTREAM-README.md); the design
docs are under [`docs/`](docs).

## What changed in the fork

| | Upstream | Fork |
|---|---|---|
| egui / egui-wgpu / egui_glow | 0.34 | **0.35** |
| `backdrop-blur-egui` default feature | `own-loop` | **`grab-pass`** |
| `egui_kittest` dev-dependency | declared | dropped (no test referenced it) |
| Cargo manifests | `workspace = true` inheritance | explicit pins, this workspace's members |
| `examples/` | 4 workspace-excluded packages | only `eframe-glow-panel`, as a workspace member |

**No library source changed.** The 0.34 → 0.35 upgrade is source-compatible across both adapter
paths: `PaintCallbackInfo`, `ViewportInPixels`, `PaintCallback`, `egui_glow::CallbackFn` /
`Painter::gl`, and the `egui_wgpu::Renderer` surface all kept their 0.34 shapes, and egui-wgpu
0.35 still resolves wgpu 29. All 108 default-tier tests pass and clippy is clean.

The port was confirmed end to end by running `eframe-glow-panel` on Windows/OpenGL: the panel
visibly blurs the animated backdrop behind it.

```bash
cargo run -p eframe-glow-panel
```

The default feature flip is the one behavior change: MasterTech's apps are eframe-on-glow, so
grab-pass is the path that gets used, and making it the default keeps the wgpu 29 + naga stack
out of a plain `cargo check` of this workspace. Build the own-loop path explicitly:

```bash
cargo check -p backdrop-blur-egui --no-default-features --features own-loop
```

`backdrop-blur-wgpu` is a workspace member but not a default-member for the same reason.

## Crates

- `backdrop-blur-core` — material/geometry vocabulary, error model, the backend seam. No GPU
  dependency, `#![forbid(unsafe_code)]`.
- `backdrop-blur-glow` — the grab-pass backend (OpenGL 3.3 / GLES 3.0 / WebGL2). The one crate
  with `unsafe`, quarantined behind documented safety blocks.
- `backdrop-blur-egui` — the egui adapter. `GrabPassRenderer` for eframe-on-glow;
  `OwnLoopRenderer` for a host driving egui-winit + egui-wgpu itself.
- `backdrop-blur-wgpu` — the own-loop backend (WGSL separable Gaussian). Unused here.

## Gated test tiers

Both GPU tiers need hardware this workspace's CI does not have, so they are feature-gated and
were **not** run for the port:

```bash
cargo test -p backdrop-blur-glow --features gl-snapshots -- --test-threads=1   # needs EGL
cargo test -p backdrop-blur-wgpu --features image-snapshots                    # needs lavapipe
```

## Re-syncing with upstream

The vendored tree is `crates/` + `docs/` verbatim except for the manifests. To take a new
upstream revision, copy `crates/*/src` over and re-apply the manifest table above.
