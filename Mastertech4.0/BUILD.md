# Building Mastertech4.0 natively on Windows

Skia (the `egui_skia` CPU/software renderer fallback for machines with no working
GPU GL stack) is behind the `skia-render` Cargo feature and **off by default**.

- Normal dev loop — no skia, no ninja, no setup: `cargo run -p MasterTech` /
  `cargo build -p MasterTech`.
- Need the skia-enabled build (matches what ships for Windows PE fallback): use
  `skia-render`, which requires the setup below.

## Building with skia-render

Easiest: run `Mastertech4.0/build-with-skia.ps1` (defaults assume `ninja.exe` at
`C:\Users\Owner\tools\ninja\ninja.exe` and `CARGO_TARGET_DIR=C:\ct`; override with
`-NinjaPath`/`-TargetDir`/`-Profile`):

```powershell
.\Mastertech4.0\build-with-skia.ps1
```

Equivalent by hand, from the workspace root:

```powershell
$env:SKIA_NINJA_COMMAND = "C:\path\to\ninja.exe"
$env:CARGO_TARGET_DIR = "C:\ct"
cargo build -p MasterTech --profile release-fast --features skia-render
```

(cmd.exe: `set` instead of `$env:...=`. Note `set VAR=value` in PowerShell does
NOT set a real environment variable — it aliases to `Set-Variable`.)

### Prerequisites (skia-render builds only)

- **Developer Mode enabled** (Settings -> Privacy & Security -> For developers).
  Required to extract Skia's source tarball, which contains symlinks.
- **ninja** — not bundled with Rust/MSVC. Download `ninja-win.zip` from
  https://github.com/ninja-build/ninja/releases and extract `ninja.exe` anywhere.
- LLVM/clang-cl at `C:\Program Files\LLVM` (already required by this project).
- `CARGO_TARGET_DIR` must point somewhere short. Building inside this repo's own
  deeply-nested `target\` directory hits Windows' 260-character path limit while
  ninja compiles Skia's source tree and fails with
  `GetFullPathNameA(...): The filename or extension is too long.`

`skia-bindings` has no published `-static` prebuilt binary for this project's
feature set (jpeg decode/encode + pdf), so a clean build always compiles Skia
from source via `gn`/`ninja` (~8 minutes, cached per `CARGO_TARGET_DIR` after that).

## Why this matters

The workspace `.cargo/config.toml` sets `-C target-feature=+crt-static` for
`x86_64-pc-windows-msvc`, which `skia-bindings` needs to link Skia statically.
Skip the setup above for a `skia-render` build (e.g. no `CARGO_TARGET_DIR`, or no
`ninja`) and it can silently fall back to a dynamically-linked Skia — the shipped
exe then fails on customer machines with `MSVCP140.dll is not found`.

Verify a skia-render build is actually static before shipping it:

```
dumpbin /dependents path\to\MasterTech.exe
```

Should show no `MSVCP140.dll` or `VCRUNTIME140.dll`.

## After the first build

The from-source Skia compile takes ~8 minutes. It's cached per `CARGO_TARGET_DIR` —
reuse the same one and later builds are incremental and fast.
