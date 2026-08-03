# backdrop-blur-wgpu

The **wgpu** backend for [**backdrop-blur**](https://github.com/abdu-benayad/backdrop-blur) — real
backdrop blur (frosted glass / vibrancy) as a reusable, toolkit-agnostic GPU capability for Rust
GUIs.

It implements `backdrop_blur_core`'s `BackdropBlur` seam with a safe, WGSL pipeline: a **separable
Gaussian** blur for small radii and **dual-Kawase** (down/up-sample, the production-compositor
algorithm) for large radii, selected by a radius threshold, followed by a tinted, rounded-rect-masked
composite. The crate is `#![forbid(unsafe_code)]` — the only place that could want `unsafe`, the
GPU-uniform `Pod` impls, uses bytemuck derives.

This is the **own-loop** path: a host driving its own render loop renders the UI into an offscreen
intermediate, hands it to the backend as a `SourceView` (the texture view **plus its size and color
space** — a `wgpu::TextureView` exposes neither, and the backend needs both), blurs a region, and
composites the frosted surface over the display target. The host owns the final target; the backend
owns only the internal ping-pong scratch.

Most egui hosts should depend on **`backdrop-blur-egui`** (which wraps this as `OwnLoopRenderer`)
rather than this crate directly. Reach for `backdrop-blur-wgpu` when you drive wgpu without egui.

## Compatibility

`wgpu = "29"`. See the workspace MSRV in the repository.

## Status

Pre-release (`0.3.x`). The API is **not yet stable** — pin an exact version.

**0.3.0 adds `BlurError::DeviceLost`** (own-loop path). `BlurError` is `#[non_exhaustive]`, so a `_`
match arm still compiles — but if you match `BlurError`, add a `DeviceLost` arm that **tears the
device down** rather than retrying, or a `_ => retry` arm will retry on a dead device.

See [`docs/IMPL.md`](https://github.com/abdu-benayad/backdrop-blur/blob/main/docs/IMPL.md) for the
build sequence and [`docs/DESIGN.md`](https://github.com/abdu-benayad/backdrop-blur/blob/main/docs/DESIGN.md)
for the type design.

## License

`MIT OR Apache-2.0`.
