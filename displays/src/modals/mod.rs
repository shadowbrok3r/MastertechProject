
use eframe::egui::{Align, Align2, Button, Color32, Context, Frame, Key, Layout, Margin, RichText, Shadow, Ui, Widget, Window};
use crate::{chats::ChatView, DisplayModal};
use task_modal::{ModalAction, TaskModal};
use create_task_modal::CreateTaskModal;
use serde::Serialize;
use std::ops::Deref;

pub mod task_modal;
pub mod create_task_modal;
pub mod tabs;
pub mod duplicate_merge_modal;
pub mod open_service_confirm_modal;
pub mod ai_attention_modal;
#[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
pub mod entity_link_resolution_modal;

pub use duplicate_merge_modal::DuplicateMergeModal;
#[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
pub use entity_link_resolution_modal::EntityLinkResolutionModal;
pub use open_service_confirm_modal::{
    OpenServiceConfirmApply, OpenServiceConfirmModal, OpenServiceConfirmOutcome,
};

#[derive(Serialize, Default, Clone, Debug)]
pub enum ModalType{
    CreateTaskModal(CreateTaskModal),
    TaskModal(TaskModal),
    ChatView(ChatView),
    DuplicateMergeModal(DuplicateMergeModal),
    #[default]
    Null
}

impl Deref for ModalType{
    type Target = str;
    fn deref(&self) -> &Self::Target {
        match self {
            ModalType::CreateTaskModal(create_task_modal) => &create_task_modal.title,
            ModalType::TaskModal(task_modal) => &task_modal.title,
            ModalType::ChatView(chat_view) => &chat_view.title,
            ModalType::DuplicateMergeModal(merge_modal) => &merge_modal.title,
            ModalType::Null => "",
        }
    }
}

impl ModalWindow for ModalType {
    fn ui(&mut self, ctx: &Context, title: String, _min_width: f32, _min_height: f32) -> Option<ModalAction> {
        let mut open = ctx.input(|i| !i.key_pressed(Key::Escape));
        let mut modal_action = None;
        let mut close_requested = false; // Decoupled flag for modal close
        let style= &ctx.global_style().visuals;
        let mut shadow = Shadow::default();
        shadow.blur = 1;
        shadow.spread = 3;
        shadow.color = style.window_stroke.color;
        let title_color = RichText::new(title.clone()).heading().color(style.warn_fg_color);

        let mut handle_action = |action: ModalAction| {
            if let ModalAction::Close = action {
                close_requested = true; // Update modal open state
            }
            modal_action = Some(action); // Capture the action
        };

        // Window width matches the active modal's fixed content width plus the frame margins.
        let window_width = match self {
            ModalType::CreateTaskModal(create_task_modal) => {
                create_task_modal.min_width.unwrap_or(500.0) + 8.0
            }
            _ => 723.0,
        };

        // Strict height cap: runaway content must scroll/clip inside the
        // window, never grow it past the viewport (which strands the title
        // bar + close button off-screen).
        let max_height = (ctx.content_rect().height() - 80.0).clamp(400.0, 900.0);

        Window::new(title_color)
            .frame(
                Frame::default()
                .inner_margin(Margin::symmetric(4, 4))
                .stroke(style.window_stroke)
                .fill(style.window_fill)
                .corner_radius(style.menu_corner_radius)
                .shadow(shadow)
            )
            .drag_to_scroll(eframe::egui::scroll_area::DragScroll::Never)
            .pivot(Align2::CENTER_CENTER)
            .default_height(715.)
            .max_width(window_width)
            .min_width(window_width)
            .max_height(max_height)
            .constrain(true)
            .open(&mut open)
            .title_bar(true)
            .show(ctx, |ui|
        {
            match self {
                ModalType::CreateTaskModal(create_task_modal) => create_task_modal.display(ui, &mut handle_action),
                ModalType::TaskModal(task_modal) => task_modal.display(ui, &mut handle_action),
                ModalType::ChatView(chat_view) => {
                    chat_view.ui(ui);
                    None
                },
                ModalType::DuplicateMergeModal(_merge_modal) => {
                    // DuplicateMergeModal handles its own window display via show()
                    // This branch should not be reached in practice
                    None
                },
                ModalType::Null => None,
            }
        });

        
        // Synchronize the `open` state after the window logic is processed
        if close_requested {
            open = false; // Close the modal based on the flag
        }

        if !open {
            return Some(ModalAction::Close);
        }
        modal_action
    }
}

pub trait ModalWindow {
    fn ui(&mut self, ctx: &Context, title: String, min_width: f32, min_height: f32) -> Option<ModalAction>;

    fn title_bar(ui: &mut Ui, title: &str, open: &mut bool) {
        let t: RichText = RichText::new(title).heading().strong();
        Frame::default()
            .fill(Color32::from_rgb(20, 20, 25))
            .corner_radius(eframe::egui::CornerRadius{nw: 15,ne: 15,sw: 0,se: 0})
            .inner_margin(Margin::same(0))
            .outer_margin(Margin::same(0))
            .show(ui, |ui| 
        {
            ui
            .with_layout(
                Layout::top_down(Align::Max), 
            |ui|{
                if Button::new(" X ").corner_radius(eframe::egui::CornerRadius::same(10))
                .fill(Color32::BLACK)
                    .ui(ui)
                    .clicked(){
                        *open = false;
                    }
            });

            ui
            .with_layout(
                Layout::top_down(Align::Center), 
            |ui|ui.heading(t));
        });
        ui.separator();
    }
}
