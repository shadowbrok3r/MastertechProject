# Vendored crates

Ported to egui 0.36; upstream targets 0.35 and has no 0.36 branch.

| crate | upstream | version | local changes |
|---|---|---|---|
| `egui-snarl` | https://github.com/zakarumych/egui-snarl | 0.11.0 | `egui` -> workspace dep; nested `[workspace]` and `[dev-dependencies]` stripped; optional `egui-probe` dep and its 19 `cfg_attr` uses removed (it pins egui 0.35 into the lockfile) |
| `egui-scale` | https://github.com/zakarumych/egui-scale | 0.5.0 | `egui` -> workspace dep; dropped the deprecated `Visuals::clip_rect_margin` scale |

Neither needed source changes for the 0.36 API itself.
