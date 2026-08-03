# backdrop-blur-core

The backend-agnostic heart of [**backdrop-blur**](https://github.com/abdu-benayad/backdrop-blur) —
real backdrop blur (frosted glass / vibrancy) as a reusable, toolkit-agnostic GPU capability for
Rust GUIs.

This crate owns the *vocabulary* of frosted glass and the *seam* every GPU backend implements, and
nothing else. It has **no GPU dependency**, is fully headless-testable, and is
`#![forbid(unsafe_code)]`. That makes it the one crate in the workspace that cannot break a backend:
the material/geometry types, the `BlurError` model, the liveness policy, and the backdrop-blur seam
trait all live here, while `wgpu`/`glow` resource types stay out.

## The shape of a blur

A caller describes a frosted surface with a `BlurRequest` — *where* the backdrop lives and the
surface goes (`Region`s in physical pixels), and *what kind of glass* it is (`BlurRadius`, `Tint`,
`CornerRadius`). Core resolves the algorithm-agnostic parts (a physical blur radius; a clamped
`ResolvedMask`); a backend resolves the algorithm-specific parts (kernel offsets, pipelines) and
does the GPU work.

## Where this sits

You usually don't depend on this crate directly — you pick an adapter:

| Crate | Path | Pulls |
| --- | --- | --- |
| `backdrop-blur-egui` | egui adapter (grab-pass + own-loop) | glow or wgpu, by feature |
| `backdrop-blur-glow` | OpenGL / GLES / WebGL2 grab-pass backend | glow |
| `backdrop-blur-wgpu` | wgpu own-loop backend | wgpu |
| **`backdrop-blur-core`** | **the seam + vocabulary** | **nothing** |

Depend on `backdrop-blur-core` directly only to implement a **new backend** against the seam.

## Status

Pre-release (`0.2.x`). The API is **not yet stable** — expect breaking changes before `1.0`, and pin
an exact version.

See [`docs/DESIGN.md`](https://github.com/abdu-benayad/backdrop-blur/blob/main/docs/DESIGN.md) (§4 is
the load-bearing type design) and [`docs/IMPL.md`](https://github.com/abdu-benayad/backdrop-blur/blob/main/docs/IMPL.md).

## License

`MIT OR Apache-2.0`.
