use database::schema::{TaskNotePayload, User};
use eframe::emath::Vec2;
use egui::{
    Align, Color32, Frame, Label, Layout, Rect, RichText, Rounding, ScrollArea, Shape, Stroke, TextEdit, Ui, Widget
};
use super::modals::ModalState;

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
    pub current_user: User
}

impl ChatView {
    pub fn new(messages: Vec<TaskNotePayload>, current_user: User) -> Self {
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
            current_user,
            messages,
            state: ModalState::default(),
            title: "Chats".to_string()
        }
    }

    pub fn ui(&self, ui: &mut Ui) {
        ui.style_mut().visuals.widgets.inactive.bg_fill =  Color32::from_rgb(120, 10, 120);
        ui.style_mut().visuals.selection.stroke.color =  Color32::BLACK;
        ui.style_mut().visuals.selection.bg_fill = Color32::from_rgb(120, 10, 120);
        ui.style_mut().visuals.widgets.inactive.fg_stroke =  Stroke::new(1.0, Color32::WHITE);
        ui.style_mut().visuals.widgets.inactive.weak_bg_fill =  Color32::from_rgb(20, 20, 25);
        ui.style_mut().visuals.widgets.inactive.bg_stroke =  Stroke::new(1.0, Color32::from_rgb(80, 80, 80));
        ui.style_mut().visuals.widgets.open.bg_fill =  Color32::from_black_alpha(50);
        ui.style_mut().visuals.widgets.open.weak_bg_fill =  Color32::from_black_alpha(50);
        ui.style_mut().visuals.widgets.active.weak_bg_fill =  Color32::from_rgb(30,30,30);
        ui.style_mut().visuals.widgets.hovered.bg_fill =  Color32::from_rgb(12, 12, 12);
        ui.style_mut().visuals.widgets.hovered.weak_bg_fill =  Color32::TRANSPARENT;
        ui.style_mut().visuals.widgets.hovered.bg_stroke =  Stroke::new(1.0, Color32::from_rgb(200, 20, 200));
        

        ScrollArea::vertical()
            .animated(false)
            .max_height(600.0)
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                
                ui.set_width(550.0);

                let max_msg_width = ui.available_width() / 2.5;

                for item in self.messages.iter(){
                    let is_message_from_myself = if item.from == self.current_user.everest_initials{
                        true
                    }else{
                        false
                    };

                    // Messages from the user are right-aligned.
                    let layout = if is_message_from_myself {
                        Layout::top_down(Align::Max)
                    } else {
                        Layout::top_down(Align::Min)
                    };

                    ui.with_layout(layout, |ui| {
                        ui.set_max_width(max_msg_width);

                        let mut measure = |text| {
                            let label = Label::new(text);
                            // We need to calculate the text width here to enable the typical
                            // chat bubble layout where the own bubbles are right-aligned and
                            // the text within is left-aligned.
                            let (_pos, galley, _response) = label
                                .layout_in_ui(&mut ui.child_ui(ui.max_rect(), *ui.layout()));
                            let rect = galley.rect;
                            // Calculate the width of the frame based on the width of
                            // the text and add 0.1 to account for floating point errors.
                            f32::min(
                                rect.width() / 2.5,// + inner_margin * 2.0 + outer_margin * 2.0 + 0.1,
                                max_msg_width,
                            )
                        };

                        let content = RichText::new(&item.note);
                        let mut msg_width = measure(content.clone());
                        let name = RichText::new(&item.from).strong();
                        let width = measure(name.clone());
                        msg_width = f32::max(msg_width, width);

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

                // ui.all
            });
        
        ui.visuals_mut().extreme_bg_color= Color32::BLACK;
        ui.visuals_mut().code_bg_color = Color32::BLACK;
        ui.visuals_mut().extreme_bg_color= Color32::BLACK;
        ui.visuals_mut().code_bg_color = Color32::BLACK;
        ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::new(2.0, Color32::from_additive_luminance(80));
        ui.style_mut().visuals.widgets.inactive.bg_fill = Color32::BLACK;
        ui.style_mut().visuals.widgets.inactive.weak_bg_fill = Color32::BLACK;
        let _text_edit = TextEdit::singleline(&mut "Enter a message")
            .desired_rows(4)
            .desired_width(ui.available_width())
            .code_editor()
            .horizontal_align(egui::Align::Center)
            .show(ui);

        ui.shrink_width_to_current();
    }
}
