
use eframe::egui::{Align, Align2, Button, Color32, Context, Frame, Key, Layout, Margin, RichText, Rounding, Shadow, Ui, Widget, Window};
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
        let style= &ctx.style().visuals;
        let mut shadow = Shadow::default();
        shadow.blur = 1.0;
        shadow.spread = 3.0;
        shadow.color = style.window_stroke.color;
        let title_color = RichText::new(title.clone()).color(style.warn_fg_color);
        let window = Window::new(title_color)
            .frame(
                Frame::default()
                .inner_margin(Margin::symmetric(4., 4.))
                .stroke(style.window_stroke)
                .fill(style.window_fill)
                .rounding(style.menu_rounding)
                .shadow(shadow)
            )
            .pivot(Align2::CENTER_TOP)
            .default_width(680.0)
            .open(&mut open)
            .title_bar(true);

        let response = window.show(ctx, |ui| {
            
            let item_spacing_y = ui.spacing().item_spacing.y;
            ui.spacing_mut().item_spacing.y = 0.0;

            Frame {
                inner_margin: Margin::same(0.0),
                ..Default::default()
            }
            .show(ui, |ui| {
                ui.add_space(item_spacing_y);

                Frame {
                    inner_margin: Margin::same(0.0),
                    ..Default::default()
                }
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = item_spacing_y;
                    match self {
                        ModalType::CreateTaskModal(create_task_modal) => create_task_modal.display(ui),
                        ModalType::TaskModal(task_modal) => task_modal.display(ui),
                        ModalType::ChatView(chat_view) => {
                            chat_view.ui(ui);
                            None
                        },
                        ModalType::Null => None,
                    }
                })
                .inner
            })
            .inner
        });

        if open.eq(&false){
            return Some(ModalAction::Close)
        }

        response.and_then(|response| response.inner.and(Some(ModalAction::None)))
    }
}


pub trait ModalWindow {
    fn ui(&mut self, ctx: &Context, title: String, min_width: f32, min_height: f32) -> Option<ModalAction>;

    fn title_bar(ui: &mut Ui, title: &str, open: &mut bool) {
        let t: RichText = RichText::new(title).heading().strong();
        Frame::default()
            .fill(Color32::from_rgb(20, 20, 25))
            .rounding(Rounding{nw: 15.0,ne: 15.0,sw: 0.0,se: 0.0})
            .inner_margin(Margin::same(0.0))
            .outer_margin(Margin::same(0.0))
            .show(ui, |ui| 
        {
            ui
            .with_layout(
                Layout::top_down(Align::Max), 
            |ui|{
                if Button::new(" X ").rounding(Rounding::same(10.0))
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
