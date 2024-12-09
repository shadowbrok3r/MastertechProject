use std::sync::{atomic::{AtomicBool, Ordering}, Arc};

use eframe::egui::{Context, ViewportBuilder, ViewportId};
use log::info;

use crate::{app_state::SharedContext, modals::{task_modal::ModalAction, ModalType, ModalWindow}};



impl SharedContext {
    pub fn handle_viewports(&mut self, ctx: &Context) {
        for (id, viewport_data) in self.show_tasks_viewport.iter_mut() {
            info!("ID: {id:?}\nviewport: {:?}", viewport_data.is_visible);
            if viewport_data.is_visible.load(Ordering::Relaxed) {
                let viewport_state = viewport_data.is_visible.clone();
                let viewport_id = ViewportId::from_hash_of(id.key().to_string());
                let viewport_builder = ViewportBuilder::default()
                    .with_title("Task Viewport")
                    .with_inner_size([750.0, 850.0]); // Match modal dimensions

                ctx.show_viewport_immediate(
                    viewport_id,
                    viewport_builder,
                    |ctx, _class| 
                {
                    // Render the TaskModal UI
                    let action = viewport_data.modal.ui(ctx, "Test".to_string(), 750., 850.);
                    if let Some(ModalAction::Close) = action {
                        viewport_state.store(false, Ordering::Relaxed); // Close viewport
                    }
                    if ctx.input(|i| i.viewport().close_requested()) {
                        viewport_state.store(false, Ordering::Relaxed); // Handle viewport close
                    }
                });
            }
        }
    }
}


#[derive(Default, Debug)]
pub struct ViewportData {
    pub is_visible: Arc<AtomicBool>,
    pub modal: ModalType,
}