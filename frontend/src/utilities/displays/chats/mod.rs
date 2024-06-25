use database::schema::{TaskId, TaskNotePayload, User};
use eframe::emath::Vec2;
use egui::{
    epaint::Shadow, Align, Button, CentralPanel, Color32, Frame, Label, Layout, Margin, Rangef, Rect, RichText, Rounding, ScrollArea, Shape, Stroke, TextEdit, TopBottomPanel, Ui, Widget
};
use log::info;
use markdown_editor::{EasyMarkEditor, SHORTCUT_ENTER};
use wasm_bindgen_futures::spawn_local;
use super::modals::ModalState;

pub mod markdown_editor;
pub mod highlighter;
pub mod parser;
pub mod viewer;

#[derive(Debug, Clone, Default)]
pub struct ChatMessage {
    pub note: String,
    pub from: String,
}

#[derive(Debug, Clone)]
pub struct ChatView{
    pub state: ModalState,
    pub title: String,
    pub messages: Vec<ChatMessage>,
    pub current_user: Option<User>,
    pub markdown_editor: EasyMarkEditor,
    pub task_id: Option<TaskId>
}

impl Default for ChatView{
    fn default() -> Self {
        Self { 
            state: ModalState::default(), 
            title: "Chat".to_string(), 
            messages: Vec::new(), 
            current_user: None, 
            markdown_editor: EasyMarkEditor::default(),
            task_id: None
        }
    }
}

impl ChatView {
    pub fn new(messages: Vec<TaskNotePayload>, current_user: User, task_id: TaskId) -> Self {
        let messages = messages
        .into_iter()
        .map(|chat_message| 
            ChatMessage {
                note: chat_message.note,
                from: chat_message.everest_initials
            }
        )
        .collect();
    
        ChatView {
            current_user: Some(current_user),
            messages,
            state: ModalState::default(),
            title: "Chats".to_string(),
            markdown_editor: EasyMarkEditor::default(),
            task_id: Some(task_id)
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) -> Option<String>{
        let mut new_msg: Option<String> = None;

        let mut shadow = Shadow::default();
        shadow.blur = 10.0;
        shadow.spread = 5.0;
        shadow.color = Color32::from_rgb(40,36,40);

        let mut b_panel_marg = Margin::default();
        let mut c_panel_marg = Margin::default();
        c_panel_marg.top = 10.0;
        b_panel_marg.bottom = 10.0;
        let color = Color32::from_rgb(6,6,10);

        let central_panel_frame = Frame::none().fill(color)
            .shadow(shadow).stroke(ui.style().visuals.widgets.inactive.bg_stroke).outer_margin(b_panel_marg)
            .inner_margin(Margin::same(6.0)).rounding(Rounding::same(10.0));

        let bottom_panel_frame = Frame::none().fill(color)
            .shadow(shadow).stroke(ui.style().visuals.widgets.inactive.bg_stroke).outer_margin(c_panel_marg)
            .inner_margin(Margin::same(6.0)).rounding(Rounding::same(10.0));

        TopBottomPanel::bottom("ChatPageBottomPanel")
            .frame(bottom_panel_frame)
            .default_height(300.0)
            .resizable(true)
            .show_inside(ui, |ui| 
        {
            ui.visuals_mut().extreme_bg_color= Color32::BLACK;
            ui.visuals_mut().code_bg_color = Color32::BLACK;
            ui.style_mut().visuals.widgets.inactive.bg_fill = Color32::BLACK;
            let enter_pressed = ui.input_mut(|i| i.consume_shortcut(&SHORTCUT_ENTER));

            if let Some(response) = self.markdown_editor.ui(ui) 
            {
                if response.clicked(){
                    let txt = self.markdown_editor.submit();
                    info!("Txt: {txt}");
                    new_msg = Some(txt);
                }
            }
        });

        CentralPanel::default()
            .frame(central_panel_frame)
            .show_inside(ui, |ui| 
        {
            ScrollArea::vertical()
                .animated(true)
                .max_height(ui.available_height())
                .max_width(f32::INFINITY)
                .auto_shrink(false)
                .stick_to_bottom(true)
                .show(ui, |ui| 
            {

                let max_msg_width = ui.available_width() / 2.5;

                for item in self.messages.iter(){
                    let mut is_message_from_myself = false;
                    if let Some(user) = &self.current_user{
                        is_message_from_myself = if item.from == user.everest_initials{
                            true
                        } else { false };
                    }

                    // Messages from the user are right-aligned.
                    let layout = if is_message_from_myself {
                        Layout::top_down(Align::Max)
                    } else {
                        Layout::top_down(Align::Min)
                    };

                    ui.with_layout(layout, |ui| {
                        ui.set_max_width(max_msg_width);

                        // let mut measure = |text| {
                        //     let label = Label::new(text);
                        //     // We need to calculate the text width here to enable the typical
                        //     // chat bubble layout where the own bubbles are right-aligned and
                        //     // the text within is left-aligned.
                        //     let (_pos, galley, _response) = label
                        //         .layout_in_ui(&mut ui.child_ui(ui.max_rect(), *ui.layout()));
                        //     let rect = galley.rect;
                        //     // Calculate the width of the frame based on the width of
                        //     // the text and add 0.1 to account for floating point errors.
                        //     f32::min(
                        //         rect.width() / 2.5,// + inner_margin * 2.0 + outer_margin * 2.0 + 0.1,
                        //         max_msg_width,
                        //     )
                        // };

                        // let content = RichText::new(&item.note);
                        // let mut msg_width = measure(content.clone());
                        let name = RichText::new(&item.from).strong();
                        // let width = measure(name.clone());
                        // msg_width = f32::max(msg_width, width);
                        let msg_width = ui.available_width() / 2.5;

                        // Set the width of the ui to the width of the message.
                        ui.set_min_width(msg_width);

                        let msg_color = if is_message_from_myself {
                            ui.style().visuals.widgets.inactive.bg_fill
                        } else {
                            ui.style().visuals.widgets.active.weak_bg_fill
                        };

                        let rounding = 8.0;
                        let margin = 8.0;
                        let response = Frame::none()
                            .rounding(Rounding {
                                ne: if is_message_from_myself {
                                    0.0
                                } else {
                                    rounding
                                },
                                nw: if is_message_from_myself {
                                    rounding
                                } else {
                                    0.0
                                },
                                se: rounding,
                                sw: rounding,
                            })
                            .inner_margin(margin)
                            .outer_margin(margin)
                            .fill(msg_color)
                            .show(ui, |ui| {
                                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                                    Label::new(name).ui(ui);

                                    ui.label(&item.note);
                                });
                            })
                            .response;

                        let points = if !is_message_from_myself {
                            let top = response.rect.left_top() + Vec2::splat(margin);
                            let arrow_rect =
                                Rect::from_two_pos(top, top + Vec2::new(-rounding, rounding));

                            vec![
                                arrow_rect.left_top(),
                                arrow_rect.right_top(),
                                arrow_rect.right_bottom(),
                            ]
                        } else {
                            let top = response.rect.right_top() + Vec2::new(-margin, margin);
                            let arrow_rect =
                                Rect::from_two_pos(top, top + Vec2::new(rounding, rounding));

                            vec![
                                arrow_rect.left_top(),
                                arrow_rect.right_top(),
                                arrow_rect.left_bottom(),
                            ]
                        };

                        ui.painter()
                            .add(Shape::convex_polygon(points, msg_color, Stroke::NONE));

                    });
                };
            });
        });

        new_msg
    }
}
