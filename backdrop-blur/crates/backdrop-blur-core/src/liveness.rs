//! Backdrop freshness as a *typed adapter obligation*, not a footnote. Whether a blurred
//! backdrop stays correct as content behind it changes does **not** generalize across toolkits
//! (egui is reactive-by-default: it does not repaint when behind-surface content changes, and
//! has no region invalidation — "fresh as long as the host repaints" can be *zero* frames). So
//! the adapter must state its intent with a domain type rather than assume the host repaints
//! (DESIGN §4.6).

use std::time::Duration;

/// How often the frosted surface's backdrop must be re-grabbed and re-blurred. The adapter —
/// not core — drives the host's `request_repaint` from this; core only names the obligation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepaintPolicy {
    /// The backdrop is over still content (dialog, tooltip): grab once, never refresh. The
    /// default — cheapest, and correct whenever the content behind the surface is static.
    ///
    /// `Static` means the adapter does not *drive* repaints for this surface — **not** that the
    /// surface stops compositing. If the host repaints for any other reason (input, animation
    /// elsewhere, another surface's `Live`), the frost still re-grabs and re-composites that frame.
    /// It is "don't request frames," not "don't paint."
    Static,
    /// The backdrop is over animating content: refresh every frame. Names an idle-power cost
    /// and is **required** for glass over moving content (otherwise the blur goes stale).
    Live,
    /// Refresh periodically — for content that changes on a known cadence.
    Bounded(Duration),
}

impl Default for RepaintPolicy {
    /// `Static` — the safe, cheap default for the common dialog/tooltip-over-still-content case.
    fn default() -> Self {
        Self::Static
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_repaint_policy_is_static() {
        assert_eq!(RepaintPolicy::default(), RepaintPolicy::Static);
    }

    #[test]
    fn bounded_policy_carries_its_interval() {
        let policy = RepaintPolicy::Bounded(Duration::from_millis(250));
        assert_eq!(policy, RepaintPolicy::Bounded(Duration::from_millis(250)));
        assert_ne!(policy, RepaintPolicy::Bounded(Duration::from_millis(500)));
    }
}
