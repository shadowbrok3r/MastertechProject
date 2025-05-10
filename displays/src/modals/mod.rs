
use eframe::egui::{Align, Align2, Button, Color32, Context, Frame, Key, Layout, Margin, RichText, Shadow, Ui, Widget, Window};
use crate::{chats::ChatView, DisplayModal};
use task_modal::{ModalAction, TaskModal};
use create_task_modal::CreateTaskModal;
use serde::Serialize;
use std::ops::Deref;

pub mod task_modal;
pub mod create_task_modal;

#[derive(Serialize, Default, Clone, Debug)]
pub enum ModalType{
    CreateTaskModal(CreateTaskModal),
    TaskModal(TaskModal),
    ChatView(ChatView),
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
            ModalType::Null => "",
        }
    }
}

impl ModalWindow for ModalType {
    fn ui(&mut self, ctx: &Context, title: String, _min_width: f32, _min_height: f32) -> Option<ModalAction> {
        let mut open = ctx.input(|i| !i.key_pressed(Key::Escape));
        let mut modal_action = None;
        let mut close_requested = false; // Decoupled flag for modal close
        let style= &ctx.style().visuals;
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

        let window = Window::new(title_color)
            .frame(
                Frame::default()
                .inner_margin(Margin::symmetric(4, 4))
                .stroke(style.window_stroke)
                .fill(style.window_fill)
                .corner_radius(style.menu_corner_radius)
                .shadow(shadow)
            )
            // .scroll([false, true])
            .drag_to_scroll(false)
            // .scroll_bar_visibility(eframe::egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
            .pivot(Align2::CENTER_CENTER)
            // .max_width(722.)
            // .resizable([false, false])
            .max_height(800.)
            .default_width(722.0)
            .open(&mut open)
            .title_bar(true);

        
        window.show(ctx, |ui| {
            match self {
                ModalType::CreateTaskModal(create_task_modal) => create_task_modal.display(ui, &mut handle_action),
                ModalType::TaskModal(task_modal) => task_modal.display(ui, &mut handle_action),
                ModalType::ChatView(chat_view) => {
                    chat_view.ui(ui);
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
        // if let Some(x) = &response {
        //     if let Some(y) = &x.inner {
        //         if let Some(action) = &y {
        //             if let &ModalAction::Close = action {
        //                 open = false;
        //             }
        //         }
        //     }
        // }

        // if !open {
        //     return Some(ModalAction::Close);
        // }

        // response.and_then(|response| response.inner.and(Some(ModalAction::None)))
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
