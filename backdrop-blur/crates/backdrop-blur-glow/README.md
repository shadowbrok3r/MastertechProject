# backdrop-blur-glow

The **glow** (OpenGL 3.3 / GLES 3.0 / WebGL2) **grab-pass** backend for
[**backdrop-blur**](https://github.com/abdu-benayad/backdrop-blur) — real backdrop blur (frosted
glass / vibrancy) as a reusable, toolkit-agnostic GPU capability for Rust GUIs.

It is the path the own-loop/wgpu backend never served: real frosted glass for an `eframe`-on-glow app
and the `cage` Wayland kiosk, where the host owns the GL loop and the blur must **grab** a region of
the live framebuffer rather than receive a ready-made intermediate. The version triple is a
construction-time contract: `GlowBlur::new` refuses an older context with
`BlurError::UnsupportedContext`.

![Frosted glass dialog, dark theme](https://raw.githubusercontent.com/abdu-benayad/backdrop-blur/main/docs/screenshots/glass-dialog-dark.png)

*A frosted glass surface composited by the grab-pass over a busy backdrop — the colour behind blurs
through pure clear glass.*

## The one `unsafe` crate

Every other crate in the workspace is `#![forbid(unsafe_code)]`. This one cannot be: glow's API is
`unsafe` end to end (raw GL is unsynchronized global state). The `unsafe` is **quarantined here** and
held to rules enforced at build time:

- `#![deny(unsafe_op_in_unsafe_fn)]` — every GL call needs an explicit `unsafe` block with a
  `// SAFETY:` justification.
- `#![deny(clippy::undocumented_unsafe_blocks)]` — a missing justification fails the build rather
  than slips through review.
- **No GL in `Drop`.** GL objects are freed only by an explicit `GlowBlur::destroy` the host calls
  from `eframe::App::on_exit` (where the context is still current). A dropped-without-destroy
  blurrer `log::warn!`s and leaks rather than calling GL on a possibly-gone context (undefined
  behavior).

## Portability

glow is build-script-free (runtime-loaded function pointers), so this crate **compiles on any runner
with no GL present**. Everything that needs a live context sits behind the `gl-snapshots` feature, so
plain `cargo test` stays GPU-free.

Most egui hosts should depend on **`backdrop-blur-egui`** (which wraps this as `GrabPassRenderer`)
rather than this crate directly.

## Status

Pre-release (`0.2.x`). The API is **not yet stable** — pin an exact version.

See [`docs/GLOW_DESIGN.md`](https://github.com/abdu-benayad/backdrop-blur/blob/main/docs/GLOW_DESIGN.md)
and [`docs/GLOW_IMPL.md`](https://github.com/abdu-benayad/backdrop-blur/blob/main/docs/GLOW_IMPL.md).

## License

`MIT OR Apache-2.0`.
