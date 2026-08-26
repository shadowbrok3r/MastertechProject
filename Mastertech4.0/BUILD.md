# Building Mastertech4.0

## Linux release builds — use the container

Binaries take on the glibc symbol versions of the machine that links them.
Build on a rolling-release host (Manjaro/Arch) and the result dies on
customer machines with:

```
version 'GLIBC_2.43' not found (required by ./MasterTech-linux)
```

glibc can't be statically linked (NSS/dlopen, and the GPU drivers the app
must dlopen are glibc-linked .so files) — the Linux equivalent of static
vcruntime is building against the **oldest** glibc you ship to. glibc is
forward-compatible, so a binary linked against 2.35 runs everywhere newer.

From the workspace root:

```bash
./build-linux-compat.sh                  # release-fast (default)
./build-linux-compat.sh release          # or any other profile
```

This builds `Mastertech4.0/docker/linux-x64/Dockerfile` (Ubuntu 22.04,
glibc 2.35) and compiles inside it via podman (set `CONTAINER_ENGINE=docker`
to use docker). Output: `target-linux-compat/<profile>/MasterTech`. The
script prints the binary's actual glibc floor at the end; ship only
binaries from this path, never from a native `cargo build` on a rolling
host. Native `cargo build` stays fine for local dev.

# Building natively on Windows

Skia (the `egui_skia` CPU/software renderer fallback for machines with no working
GPU GL stack) is behind the `skia-render` Cargo feature and **off by default**.

- Normal dev loop — no skia, no ninja, no setup: `cargo run -p MasterTech` /
  `cargo build -p MasterTech`.
- Need the skia-enabled build (matches what ships): use `skia-render`, which
  requires the setup below.

Release CI (`.github/workflows/release.yml`) passes `features: skia-render` for
the Windows `MasterTech` binary, so the shipped exe has the middle rung of the
`GPU -> software -> terminal` ladder. A build without it answers `--cpu` with an
error and drops to terminal mode; it does not silently pretend to have a
software renderer.

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

# Diagnosing a blank or missing window

The client always writes a log, and the renderer can be degraded from the
command line without a rebuild.

## Where the log is

`%LOCALAPPDATA%\Mastertech\logs\output.log`, rotated to `output.1.log` ..
`output.5.log` on each launch. If that directory is not writable the app falls
back to the exe's own directory and then to temp; the chosen path is printed in
the third line of every log. Override with `MTECH_LOG_DIR`. File logging is
unconditional — `--log` is accepted but no longer gates it.

## What to read first

Every launch writes a banner (version, pid, exe path, args, log path, renderer
knobs), then `GL context: vendor=... renderer=... version=...` naming the driver
and adapter the GL context actually landed on, then a `render loop:` heartbeat
carrying the viewport rect, `pixels_per_point`, and the previous frame's frost
result.

- `render loop:` lines present, `frost=Failed` — the backdrop-blur grab pass is
  failing. Three consecutive failures switch frosting off for the rest of the
  process and surfaces paint unfrosted.
- `render loop:` lines present with a degenerate `viewport_rect` — the window is
  laying out into nothing; not a blur problem.
- No `render loop:` lines at all — the frame loop never started; the eframe error
  is above.

## Turning the glass off

`MTECH_NO_FROST=1` (or `--no-frost`) skips building the grab-pass backend
entirely, so no paint callback is ever enqueued. If the UI draws with this set
and not without it, the fault is in the backdrop-blur path.

## Forcing the software renderer

`--cpu` (alias `--software`) skips the GPU attempt. On a build without
`skia-render` this logs an explicit error and falls through to terminal mode.
