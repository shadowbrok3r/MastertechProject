# backdrop-blur

**Real backdrop blur — frosted glass / vibrancy — as a reusable, toolkit-agnostic GPU
capability for Rust GUIs.**

A frosted-glass surface is one whose background is the blurred, tinted copy of the content
behind it (macOS vibrancy, Windows Acrylic, CSS `backdrop-filter`). It is a first-class
material that **no Rust GUI toolkit ships today** — `window-vibrancy` does whole-window *OS*
vibrancy, Vello does *shape* blur; neither is in-app, arbitrary-surface backdrop blur. This
crate fills that gap once, so each toolkit needs only a thin adapter instead of re-deriving
the render-to-texture + multi-tap convolution every time.

> **Status: pre-release (`0.2.x`).** The safe wgpu/own-loop slice and the glow grab-pass backend are
> built and tested. The API is **not yet stable** — expect breaking changes before `1.0`, and pin an
> exact version. See [`docs/IMPL.md`](docs/IMPL.md) and [`docs/GLOW_IMPL.md`](docs/GLOW_IMPL.md) for
> the build sequence.

## Using it (grab-pass / eframe-on-glow)

The mainstream path. Build the renderer once from eframe's GL context, frost a surface each frame
*before* painting its foreground, and free it on exit. Full runnable example:
[`examples/eframe-glow-panel`](examples/eframe-glow-panel).

```toml
[dependencies]
backdrop-blur-egui = { version = "0.2", default-features = false, features = ["grab-pass"] }
```

```rust,ignore
use backdrop_blur_egui::{
    BlurRadius, CornerRadius, GrabPassRenderer, Presence, RepaintPolicy, Surface, Tint,
};

// Once, in eframe's creation closure (glow backend):
let renderer = GrabPassRenderer::new(cc.gl.as_ref().expect("glow backend"))?;

// Each frame, inside your panel — FROST FIRST, then paint the foreground on top:
let surface = Surface {
    rect: panel_rect,                                  // dynamic rect? pass LAST frame's (see below)
    blur_radius: BlurRadius::new(16.0),                 // logical points
    tint: Tint::from_srgb_unmultiplied([255, 255, 255, 40]), // film: alpha = tint vs. blur mix
    corner_radius: CornerRadius::new(12.0),
    presence: Presence::FULL,                          // fade dial — drive per frame, NOT multiply_opacity
    repaint: RepaintPolicy::Static,                    // still content behind the glass
};
renderer.frost(ui, surface);
// ...now paint the panel's text/controls so they land on top of the blur...

// In eframe::App::on_exit, while the context is still current:
// renderer.destroy(gl);
```

Three contracts the types can't enforce — read them before shipping: **frost before foreground**,
**fade with `Presence` (egui's `multiply_opacity` no-ops on paint callbacks)**, and for a
dynamically-sized surface **pass last frame's rect** (the rect is unknown until content lays out, but
the frost must enqueue before it paints — stash it in egui temp memory). The crate-root rustdoc
("Grab-pass contracts") has the worked detail.

## Compatibility

An egui-ecosystem crate is bound to one egui minor. Pin to a row:

| `backdrop-blur-*` | egui | egui_glow / egui-wgpu | wgpu | glow | MSRV |
|---|---|---|---|---|---|
| `0.2.x` | `0.34` | `0.34` | `29` | `0.17` | `1.92` |

- **Feature flags** (`backdrop-blur-egui`): `grab-pass` → the glow/eframe path (pulls glow, **no
  wgpu** — the kiosk-light config); `own-loop` (default) → the egui-wgpu path (pulls the wgpu stack).
  Set `default-features = false` + `grab-pass` for the kiosk build.
- **MSRV `1.92`** is set by egui/egui-wgpu 0.34 and wgpu 29; it is a floor a CI job verifies, not a
  promise to never raise it.

## What it is (and is not)

- **Is:** one material — backdrop blur + tint + rounded-rect mask — as a GPU pass with a
  backend-agnostic seam, plus thin per-toolkit adapters. Built for *surfaces* (tooltip,
  dialog, drawer, popover): a handful visible at once, each paying one grab + blur.
- **Is not:** a general effects/filter graph, OS/compositor vibrancy, or a renderer. It
  composites *into* a target the host owns; it never owns the frame.

## Scope, stated plainly

v1 was the disciplined minimal slice on the safe wgpu/own-loop backend; the **glow grab-pass
backend has since landed** (originally deferred past v1). The current state is both paths over
one shared seam:

```
backdrop-blur-core
  ├─ backdrop-blur-wgpu  → backdrop-blur-egui (own-loop)   → examples/{egui-wgpu-panel, frost-gallery}
  └─ backdrop-blur-glow  → backdrop-blur-egui (grab-pass)  → examples/eframe-glow-panel
```

- **`unsafe` is quarantined to one crate.** Every crate is `#![forbid(unsafe_code)]` *except*
  `backdrop-blur-glow`, where raw GL is inherently `unsafe`. There it is held to
  `#![deny(unsafe_op_in_unsafe_fn)]` + `#![deny(clippy::undocumented_unsafe_blocks)]` — every
  block carries a `// SAFETY:` justification or the build fails — and a wgpu/own-loop consumer
  never compiles it (Cargo's additive features). The glow GL is verified by headless
  EGL-surfaceless readback tests on a real context.
- **Two egui paths over one `Surface`.** The **own-loop** path drives `egui-winit` + `egui-wgpu`
  directly (caller-chosen attachment, no fork). The **grab-pass** path is `eframe`-on-glow and the
  `cage` Wayland kiosk: grab a region of the live framebuffer, blur it, composite the frosted
  surface back, all in an egui paint callback.
- A **single frosted surface over a once-rendered backdrop**, non-overlapping. Ordered, stacked
  glass is named future work.

## Workspace layout

| Crate | Role | Status |
|---|---|---|
| `backdrop-blur-core` | seam trait + material/geometry/error/liveness vocabulary; no GPU dep; `#![forbid(unsafe_code)]` | v1 |
| `backdrop-blur-wgpu` | wgpu backend (WGSL), safe | v1 |
| `backdrop-blur-egui` | egui adapter — own-loop (→ wgpu) **and** grab-pass (→ glow) | built |
| `backdrop-blur-glow` | glow backend (OpenGL 3.3 / GLES 3.0 / WebGL2) — the one `unsafe` crate, quarantined | built |
| `backdrop-blur` | optional thin facade (re-exports) | deferred |

Backends and adapters are separate crates on purpose: distinct public resource types
(`wgpu::TextureView` vs a glow texture) plus Cargo's additive-feature rule mean a consumer
compiles exactly the backends and toolkits it names — a wgpu user never builds glow's
`unsafe`.

## Verification

Two tiers (see [`docs/IMPL.md`](docs/IMPL.md) §8):

```bash
# Default tier — runs on any machine, GPU or not:
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test  --workspace
cargo build --workspace

# Kiosk (grab-pass) config — must build and pull no wgpu:
cargo build -p backdrop-blur-egui --no-default-features --features grab-pass
cargo build -p backdrop-blur-glow --target wasm32-unknown-unknown   # WebGL2 readiness

# Gated GPU tiers (a real renderer; never in the default tier):
cargo test -p backdrop-blur-wgpu --features image-snapshots -- --test-threads=1  # lavapipe Vulkan
cargo test -p backdrop-blur-glow --features gl-snapshots    -- --test-threads=1  # EGL-surfaceless GL
```

## Documentation

The full design trail travels with the crate:

- [`docs/DESIGN.md`](docs/DESIGN.md) — the *what & why*; §4 is the load-bearing type design.
- [`docs/IMPL.md`](docs/IMPL.md) — the *how & in what order*; each sub-step is a compiling, testable increment.
- [`docs/STRUCTURE.md`](docs/STRUCTURE.md) — the workspace topology and its ecosystem precedents.
- [`docs/RESEARCH.md`](docs/RESEARCH.md) — feasibility and the blur algorithm (dual-Kawase tap offsets).

## License

Dual-licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your option.
