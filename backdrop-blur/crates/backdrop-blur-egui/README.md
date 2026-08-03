# backdrop-blur-egui

The **egui** adapter for [**backdrop-blur**](https://github.com/abdu-benayad/backdrop-blur) — real
backdrop blur (frosted glass / vibrancy) as a reusable, toolkit-agnostic GPU capability for Rust
GUIs.

A frosted-glass surface is one whose background is the blurred, tinted copy of the content behind it
(macOS vibrancy, Windows Acrylic, CSS `backdrop-filter`). This crate brings it to egui over **two
paths sharing one `Surface` vocabulary**.

![Frosted glass dialog, dark theme](https://raw.githubusercontent.com/abdu-benayad/backdrop-blur/main/docs/screenshots/glass-dialog-dark.png)

## The two paths

- **`grab-pass`** (the mainstream path: `eframe`-on-glow and the `cage` Wayland kiosk). The host owns
  the GL loop; `GrabPassRenderer` rides an egui **paint callback** that grabs the live framebuffer
  behind a surface, blurs it, and composites the frosted surface back. Pulls glow, never wgpu.
- **`own-loop`** (default feature). For a host driving `egui-winit` + `egui-wgpu` directly (not
  eframe), `OwnLoopRenderer` renders the UI into an offscreen intermediate, blurs a region, and
  composites a frosted `Surface` over the display target. Pulls the wgpu stack.

Pick the path with a feature: a kiosk build is `--no-default-features --features grab-pass` and
compiles neither wgpu nor egui-wgpu; an own-loop build is the default.

## Using it (grab-pass / eframe-on-glow)

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
**fade with `Presence`** (egui's `multiply_opacity` no-ops on paint callbacks), and for a
dynamically-sized surface **pass last frame's rect** (the rect is unknown until content lays out, but
the frost must enqueue before it paints — stash it in egui temp memory). The crate-root rustdoc
("Grab-pass contracts") has the worked detail.

The crate owns only a surface's *background*. The surface's content, foreground, and accessibility
stay the host's: a frosted `Surface` is a post-render composite, never an egui widget, so it adds
nothing to the AccessKit tree.

## The three dials: blur, tint, presence

Independent knobs — conflating them is the most common "my glass looks wrong":

- **`BlurRadius`** — in logical points. `0` = no blur (a plain tinted pane).
- **`Tint`** — the glass *film* over the blur, a linear-light color whose **alpha is the film mix**
  (how much tint shows vs. how much blurred backdrop shows through).
- **`Presence`** — the fade dial. Drive it per frame; do not reach for `multiply_opacity`.

## Status

Pre-release (`0.3.x`). The API is **not yet stable** — expect breaking changes before `1.0`, and pin
an exact version.

**0.3.0 adds `BlurError::DeviceLost`** (own-loop path). `BlurError` is `#[non_exhaustive]`, so a `_`
match arm still compiles — but if you match `BlurError`, add a `DeviceLost` arm that **tears the
device down** rather than retrying, or a `_ => retry` arm will retry on a dead device.

A full runnable example lives at
[`examples/eframe-glow-panel`](https://github.com/abdu-benayad/backdrop-blur/tree/main/examples/eframe-glow-panel).

## License

`MIT OR Apache-2.0`.
