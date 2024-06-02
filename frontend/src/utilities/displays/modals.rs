use serde::Serialize;
use crate::utilities::ModalTypes;

use super::task_modal::ModalAction;

#[derive(Default, Serialize)]
pub struct ModalHandler<M: ModalTypes>{
    modal: Option<M>,
    should_open: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct ModalState {
    pub min_width: Option<f32>,
    pub min_height: Option<f32>,
    pub default_height: Option<f32>,
    pub full_span_content: bool,
    pub computer_info_page: bool,
}

/// Response returned by [`Modal::ui`].
pub struct ModalResponse<R> {
    /// What the content closure returned, if it was actually run.
    pub inner: Option<R>,
    /// Whether the modal should remain open.
    pub open: bool,
}

impl Default for ModalState {
    fn default() -> Self {
        Self { min_width: None, min_height: None, default_height: None, full_span_content: false , computer_info_page: false}
    }
}

impl <M: ModalTypes>ModalHandler<M> {
    pub fn set_state(&mut self) {
        
    }
    /// Open the model next time the [`ModalHandler::ui`] method is called.
    pub fn open(&mut self) {
        self.should_open = true;
    }

    /// Draw the modal window, creating/destroying it as required.
    pub fn ui<R>(
        &mut self,
        ctx: &egui::Context,
        make_modal: impl FnOnce(&mut ModalAction) -> M,
        content_ui: impl FnOnce(&mut egui::Ui, &mut bool) -> R,
    ) -> Option<R> {
        if self.modal.is_none() && self.should_open {
            self.modal = Some(make_modal(&mut ModalAction::None));
            self.should_open = false;
        }
        if let Some(modal) = &mut self.modal {
            let ModalResponse { inner, open } = modal.ui(ctx, content_ui);
            if !open {
                self.modal = None;
            }

            inner
        } else {
            None
        }
    }
}