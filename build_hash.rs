// Shared build-script helper: emits `BUILD_HASH` as a rustc-env var so
// the consuming crate can `env!("BUILD_HASH")` at compile time and
// display it alongside `CARGO_PKG_VERSION`.
//
// Each crate's `build.rs` does
//
// ```ignore
// include!("../build_hash.rs");
//
// fn main() {
//     emit_build_hash();
//     // … other build-script work
// }
// ```
//
// Hash format: `{git7}{dirty}.{ts6}`
// - `git7` — `git rev-parse --short=7 HEAD`, or `"nogit"` if git isn't
//   reachable from the build environment.
// - `dirty` — single `d` if the working tree has uncommitted changes,
//   empty otherwise. Lets you instantly see "I'm running an
//   uncommitted local build."
// - `ts6` — last 24 bits of the build's UNIX time, hex-encoded. Ensures
//   two recompiles at the same git revision produce different
//   hashes — the original motivation: distinguish iterative dev builds
//   that share a version number.
//
// Example: `1a3f2b9d.e8a3c1` (10 chars + 6 chars = 17 chars total).

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn emit_build_hash() {
    let git_short = Command::new("git")
        .args(["rev-parse", "--short=7", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "nogit".to_string());

    // `git status --porcelain` prints one line per modified/untracked
    // path; empty output means a clean tree. Untracked files are
    // intentionally not ignored — they often reflect dev-only state.
    let dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);
    let dirty_mark = if dirty { "d" } else { "" };

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let ts6 = format!("{:06x}", ts & 0xFF_FFFF);

    println!("cargo:rustc-env=BUILD_HASH={git_short}{dirty_mark}.{ts6}");

    // Re-run the build script whenever the crate's `src/` directory
    // changes so the timestamp refreshes per iteration. Without this,
    // BUILD_HASH would freeze after the first compile and the whole
    // point of the helper is lost.
    println!("cargo:rerun-if-changed=src");
    // And whenever git's HEAD moves (commit, checkout, rebase) so the
    // git-short component stays correct without a manual touch.
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/index");
}
