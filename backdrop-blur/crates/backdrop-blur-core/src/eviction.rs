//! Last-frame-used eviction for the per-size scratch caches the GPU backends hold.
//!
//! A frosted surface that is dragged or changes DPI asks for a new `(size, levels)` scratch chain at
//! every distinct size; without eviction each backend's cache would grow one chain per size forever
//! and leak GPU memory. Both backends key their caches by [`PingPongKey`] and stamp each chain with
//! the frame it was last touched; this module owns the **pure decision** of which chains are now
//! stale, so it is unit-tested here with no GPU (DESIGN §14, Tier 0) and *shared* by
//! `backdrop-blur-wgpu` and `backdrop-blur-glow` rather than re-derived per backend.
//!
//! [`PingPongKey`]: crate::PingPongKey

/// How many frames a scratch chain may go untouched before it is evicted. Small: a surface that
/// stops being frosted should not pin its memory for long, but a chain must survive a few frames of
/// churn (a drag resizes every frame) without thrashing.
pub const RETENTION_FRAMES: u64 = 8;

/// Given each cached chain's key and the frame it was last used, the current frame, and the
/// retention window, return the keys whose age `now - last_used` has reached `retention` and so
/// should be evicted. Uses wrapping-aware distance, so a `u64` frame counter that rolls over does
/// not spuriously evict a fresh chain; a chain touched *this* frame (distance 0) is never evicted.
///
/// Pure and generic over the key type: each backend passes its own cache's `(key, last_used)` pairs
/// and frees the returned keys' GPU objects.
pub fn evict_decision<K: Copy>(
    chains: impl IntoIterator<Item = (K, u64)>,
    now: u64,
    retention: u64,
) -> Vec<K> {
    chains
        .into_iter()
        .filter(|&(_, last_used)| now.wrapping_sub(last_used) >= retention)
        .map(|(key, _)| key)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PingPongKey;

    fn key(w: u32, h: u32, levels: u32) -> PingPongKey {
        PingPongKey {
            size: [w, h],
            levels,
        }
    }

    #[test]
    fn evict_decision_keeps_a_chain_used_this_frame() {
        let k = key(100, 80, 1);
        // last_used == now → distance 0 → never evicted.
        let stale = evict_decision([(k, 5)], 5, RETENTION_FRAMES);
        assert!(stale.is_empty());
    }

    #[test]
    fn evict_decision_keeps_a_chain_inside_the_window() {
        let k = key(100, 80, 1);
        // Used 3 frames ago, retention 8 → kept.
        let stale = evict_decision([(k, 2)], 5, RETENTION_FRAMES);
        assert!(stale.is_empty());
    }

    #[test]
    fn evict_decision_evicts_a_chain_past_the_window() {
        let k = key(100, 80, 1);
        // Used 8 frames ago, retention 8 → evicted (>= boundary).
        let stale = evict_decision([(k, 0)], 8, RETENTION_FRAMES);
        assert_eq!(stale, vec![k]);
    }

    #[test]
    fn evict_decision_splits_fresh_from_stale() {
        let fresh = key(100, 80, 1);
        let stale = key(200, 100, 5);
        let out = evict_decision([(fresh, 9), (stale, 0)], 10, RETENTION_FRAMES);
        assert_eq!(out, vec![stale]);
    }

    #[test]
    fn evict_decision_is_wrap_safe_across_a_u64_rollover() {
        // The frame counter wrapped: now is small, last_used is near u64::MAX, but the true elapsed
        // distance is tiny — a naive `now - last_used` would underflow-panic or evict spuriously.
        let k = key(100, 80, 1);
        let last_used = u64::MAX - 1;
        let now = 2; // wrapping distance = 4 frames
        let stale = evict_decision([(k, last_used)], now, RETENTION_FRAMES);
        assert!(
            stale.is_empty(),
            "a 4-frame wrap distance is inside the 8-frame window"
        );
    }
}
