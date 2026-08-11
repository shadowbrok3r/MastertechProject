//! Keyboard focus discipline for remote-view canvases.
//!
//! A remote canvas forwards raw keystrokes to another machine, so the host UI must not also act on
//! them. Two separate leaks have to be plugged:
//!
//! - egui resolves `Tab`, the arrow keys and `Escape` into focus movement in `Focus::begin_pass`,
//!   before any widget runs. The only thing that stops it is an [`EventFilter`] registered against
//!   the *currently focused* widget, so the canvas has to actually hold egui focus and re-assert its
//!   filter every frame.
//! - Every other widget in the frame still reads `Event::Key` / `Event::Text` out of the shared
//!   [`InputState`](eframe::egui::InputState), so a forwarded `Enter` can activate a host button in
//!   the same pass. Those events have to be dropped once forwarded.
//!
//! [`RemoteViewFocus`] does both against a canvas [`Response`]. Focus is taken by a pointer press
//! inside the canvas and released by a press outside it, so the operator's click is what arms
//! keyboard capture — matching how the desktop viewers already gate their own forwarding.

use eframe::egui::{Event, EventFilter, Response, Ui};

/// Navigation keys the canvas keeps for itself while focused, instead of letting egui move focus.
const REMOTE_VIEW_FILTER: EventFilter = EventFilter {
    tab: true,
    horizontal_arrows: true,
    vertical_arrows: true,
    escape: true,
};

/// Keyboard capture state for one remote canvas.
#[derive(Debug, Clone, Copy, Default)]
pub struct RemoteViewFocus {
    focused: bool,
}

impl RemoteViewFocus {
    pub fn new() -> Self {
        Self::default()
    }

    /// `true` while the canvas owns keyboard focus and callers should forward keystrokes.
    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// Take focus on a pointer press inside `response`, release it on a press outside. Re-asserts
    /// the navigation-key filter while focused and returns the new focus state.
    ///
    /// `response` must come from a focusable sense (`Sense::click`/`click_and_drag`), otherwise egui
    /// drops the focus request on the next pass.
    pub fn update(&mut self, response: &Response) -> bool {
        let ctx = &response.ctx;
        let pressed_somewhere = ctx.input(|i| i.pointer.any_pressed());
        if pressed_somewhere {
            if response.contains_pointer() {
                response.request_focus();
            } else if response.has_focus() {
                response.surrender_focus();
            }
        }

        self.focused = response.has_focus();
        if self.focused {
            ctx.memory_mut(|m| m.set_focus_lock_filter(response.id, REMOTE_VIEW_FILTER));
        }
        self.focused
    }

    /// Drop every key and text event so host widgets later in the frame never see the keystrokes
    /// this canvas just forwarded. No-op while unfocused.
    pub fn swallow_keys(&self, ui: &mut Ui) {
        if !self.focused {
            return;
        }
        ui.input_mut(|i| {
            i.events
                .retain(|e| !matches!(e, Event::Key { .. } | Event::Text(_)));
        });
    }
}
