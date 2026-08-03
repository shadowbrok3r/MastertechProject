//! Tripwire pinning `wgpu-core` to the version the fatal/non-fatal OOM classification was
//! verified against.
//!
//! Every `scoped_oom(OomOutcome::…)` tag in `backdrop-blur-wgpu/src/lib.rs` and
//! `backdrop-blur-egui/src/own_loop.rs` is a **static claim** about which wgpu-core error handler a
//! creation routes through (the fatal `handle_hal_error`, which `device.lose()`s before returning,
//! vs the non-fatal variant that skips `lose()` on out-of-memory). That split is a wgpu-core
//! internal with no runtime probe, so a wgpu bump must re-verify the mapping deliberately instead
//! of letting it drift silently — this test fails the moment the resolved version moves.
//!
//! Limitations, deliberately accepted:
//! - Guards **this workspace's** resolved version only. A downstream consumer resolving a different
//!   `29.x` under the `wgpu = "29"` caret is not caught; the deferred exact-`=` pin (design open
//!   decision 3) would close that at the cost of blocking routine wgpu updates — the maintainer's
//!   call.
//! - The mapping itself is inspection-verified, not runtime-tested: there is no OOM-injection
//!   vehicle on this stack (consistent with `tests/error_scope.rs`'s documented policy). This
//!   tripwire guards the *version* the inspection was done against, not the tag logic.

/// The wgpu-core version the fatal/non-fatal classification was traced against, firsthand,
/// including the internal sub-allocations named in the failure message.
const EXPECTED: &str = "29.0.3";

#[test]
fn wgpu_core_version_matches_the_verified_oom_mapping() {
    let lockfile = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock");
    let lock = std::fs::read_to_string(lockfile)
        .unwrap_or_else(|err| panic!("could not read the workspace lockfile at {lockfile}: {err}"));

    // Naive stanza scan: the exact line `name = "wgpu-core"` (the `wgpu-core-deps-*` siblings
    // cannot match it), then the stanza's `version = "…"` on the immediately-following line.
    let mut lines = lock.lines();
    let found = lines
        .by_ref()
        .find(|line| line.trim() == "name = \"wgpu-core\"")
        .and_then(|_| lines.find_map(|line| line.trim().strip_prefix("version = ")))
        .map(|version| version.trim_matches('"'))
        .unwrap_or_else(|| panic!("no wgpu-core package stanza found in {lockfile}"));

    assert_eq!(
        found, EXPECTED,
        "wgpu-core resolved to {found}, not the {EXPECTED} the fatal/non-fatal OOM mapping was \
         verified against. Before bumping: RE-VERIFY every scoped_oom(OomOutcome::…) tag in lib.rs \
         and own_loop.rs against wgpu-core's resource.rs handlers — tracing INTO internal helper \
         calls (StagingBuffer::new at resource.rs:1130/1132, the clear-view loop at \
         device/resource.rs:1668), not just the top-level HAL dispatch. A kind wrongly tagged \
         Recoverable reintroduces device-lost-on-OOM (issue device-lost-on-fatal-arm-oom); \
         buffer/texture/sampler are the load-bearing tags."
    );
}
