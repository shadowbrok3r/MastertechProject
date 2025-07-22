use eframe::egui::{epaint::Shadow, Align, Button, CentralPanel, Color32, Direction, Frame, Id, Key, KeyboardShortcut, Layout, Margin, Modifiers, Rect, RichText, ScrollArea, Sense, Shape, Stroke, TextEdit, TopBottomPanel, Ui, Vec2, Widget};
use egui_extras::syntax_highlighting::{highlight, CodeTheme};
use crate::tabs::admin_console::WebSocketClient;
use bincode::{config::standard, serde::*};
use ewebsock::WsMessage;
use core::f32;
use crate::Cmd;

#[derive(Default, Clone, serde::Serialize, serde::Deserialize, Debug)]
pub struct History {
    pub from: String,
    pub message: String,
    pub timestamp: String
}


impl WebSocketClient {
    pub fn show_shell(&mut self, ui: &mut Ui) {
        let b_panel_marg = Margin::symmetric(5, 10);

        let id = ui.auto_id_with(format!("Chat {:?}", self.client.client_hash));

        TopBottomPanel::bottom(id)
            .default_height(ui.available_height()/1.2) // .resizable(false)
            .show_inside(ui, |ui| 
        {
            ui.visuals_mut().extreme_bg_color= Color32::BLACK;
            ui.visuals_mut().code_bg_color = Color32::BLACK;
            ui.style_mut().visuals.widgets.inactive.bg_fill = Color32::BLACK;
            
            let style = ui.style_mut();
            let default_rounding = eframe::egui::CornerRadius::same(2);
            style.visuals.widgets.inactive.corner_radius = default_rounding;
            style.visuals.widgets.active.corner_radius = default_rounding;
            style.visuals.widgets.hovered.corner_radius = default_rounding;
            
            let mut layouter = |ui: &Ui, buf: &dyn eframe::egui::TextBuffer, _: f32| {
                let mut layout_job: eframe::egui::text::LayoutJob = highlight(
                    ui.ctx(), 
                    &ui.style(), 
                    &CodeTheme::dark(12.), 
                    buf.as_str(), 
                    "bash".into()
                );
                layout_job.wrap.max_width = ui.available_width()/1.1;
                ui.fonts(|f| f.layout_job(layout_job))
            };

            ui.add_space(3.);

            let text_edit = TextEdit::singleline(&mut self.input)
                .hint_text("Use Wisely..")
                .margin(Margin::symmetric(10, 4))
                .desired_width(ui.available_width())
                .desired_rows(4)
                .layouter(&mut layouter)
                .ui(ui);
            

            
            let key_press = ui.input(|i| i.key_pressed(Key::Enter));
            let up_press = ui.input(|i| i.key_pressed(Key::ArrowUp));
            let down_press = ui.input(|i| i.key_pressed(Key::ArrowDown));
            let copy_key = ui.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::CTRL, Key::C)));

            if copy_key && text_edit.has_focus() {
                self.input.clear();
            }

            if down_press {
                if self.history_idx <= self.my_history.len() {
                    self.history_idx += 1;
                }
                if let Some(history) = self.my_history.get(self.history_idx){
                    self.input = history.message.clone();
                }
            } 
            if up_press {
                if self.history_idx > 0 {
                    self.history_idx -= 1;
                }
                if let Some(history) = self.my_history.get(self.history_idx){
                    self.input = history.message.clone();
                }
            }

            if text_edit.lost_focus() && key_press && !self.interactive{
                self.loading = true;
                text_edit.request_focus();

                self.history.push(History { 
                    from: "You".to_string(), 
                    message: self.input.clone(), 
                    timestamp:  chrono::Local::now().to_rfc3339()
                });

                self.notifications += 1;

                self.my_history.push(History { 
                    from: "You".to_string(), 
                    message: self.input.clone(), 
                    timestamp:  chrono::Local::now().to_rfc3339()
                });

                self.ws_sender.send(WsMessage::Text(std::mem::take(&mut self.input)));

            } else if text_edit.lost_focus() && key_press && self.interactive { 
                text_edit.request_focus();
                self.history.push(History { 
                    from: "You".to_string(), 
                    message: self.input.clone(), 
                    timestamp:  chrono::Local::now().to_rfc3339()
                });
                self.notifications += 1;

                self.my_history.push(History { 
                    from: "You".to_string(), 
                    message: self.input.clone(), 
                    timestamp:  chrono::Local::now().to_rfc3339()
                });

                match encode_to_vec(&Cmd::InteractiveInput(std::mem::take(&mut self.input)), standard()){
                    Ok(bytes) => self.ws_sender.send(WsMessage::Binary(bytes)),
                    Err(e) => self.history.push(History { 
                        from: "Client".to_string(), 
                        message: e.to_string(), 
                        timestamp:  chrono::Local::now().to_rfc3339()
                    }),
                }
            }
        
        });

        let central_panel_frame = Frame::new().fill(ui.style().visuals.widgets.inactive.weak_bg_fill)
            .stroke(ui.style().visuals.widgets.inactive.bg_stroke).outer_margin(b_panel_marg)
            .inner_margin(Margin::same(6));

        // info!("avail_size: {:?}", avail_size);
        CentralPanel::default()
            .frame(central_panel_frame)
            .show_inside(ui, |ui| 
        {
        // ui.allocate_ui(Vec2::new(avail_size.x, avail_size.y), |ui| {
            let id = Id::new(format!("scroll_area-{:?}", self.client.client_hash));
            ScrollArea::vertical()
                .id_salt(id)
                .animated(true)
                .max_width(f32::INFINITY)
                // .max_height(400.)
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| 
            {
                ui.set_width(ui.available_width());
                let max_msg_width = ui.available_width() / 1.2;
                let fixed_height = 50.0;

                // Display history messages
                let mut display_messages = self.history.clone();
                
                // If there's a buffer with ongoing output, show it as a temporary preview
                if !self.buffer.is_empty() {
                    display_messages.push(History {
                        from: "Client".to_string(),
                        message: format!("{}\n[Receiving...]", self.buffer.trim()),
                        timestamp: chrono::Local::now().to_rfc3339(),
                    });
                }

                // Render messages with improved styling
                for item in &display_messages {
                    let is_message_from_myself = if item.from.eq("You"){ true } else { false };
    
                    // Messages from the user are right-aligned.
                    let layout = if is_message_from_myself { 
                        Layout::top_down(Align::Max)
                    } else { 
                        Layout::top_down(Align::Min)
                    };
    
                    let msg_color = if is_message_from_myself {
                        ui.style().visuals.widgets.inactive.bg_fill
                    } else {
                        ui.style().visuals.widgets.active.weak_bg_fill
                    };
    
                    ui.with_layout(layout, |ui| {
                        ui.set_max_width(max_msg_width);
    
                        let rounding = 8.0;
                        let margin = 6.0;
                        
                        let rnding = eframe::egui::CornerRadius {
                            ne: if is_message_from_myself { 2.0 } else { rounding },
                            nw: if is_message_from_myself { rounding } else { 2.0 },
                            se: rounding,
                            sw: rounding,
                        };

                        // Add hover effect
                        let response = Frame::new()
                            .corner_radius(rnding)
                            .inner_margin(margin)
                            .outer_margin(margin)
                            .fill(msg_color)
                            .show(ui, |ui| {
                                ui.set_min_height(fixed_height);
                                ui.set_max_width(max_msg_width);
                                
                                ui.with_layout(Layout::top_down(Align::Min), |ui| {
    
                                    let mut shadow = Shadow::default();
                                    shadow.blur = 3;
                                    shadow.spread = 3;
                                    shadow.color = Color32::from_rgb(40,36,40);
                                    
                                    let mut b_panel_marg = Margin::default();
                                    b_panel_marg.top = 3;
    
                                    let color = Color32::from_rgb(10,10,12);
    
                                    let note_frame = Frame::new().fill(color)
                                        .shadow(shadow).stroke(ui.style().visuals.widgets.inactive.bg_stroke).outer_margin(b_panel_marg)
                                        .inner_margin(Margin::symmetric(6, 10)).corner_radius(rnding);
    
                                    let (from, txt) = if item.from.eq("You"){
                                        (
                                            RichText::new("Command Sent:").strong().monospace().color(Color32::LIGHT_BLUE),
                                            RichText::new(item.message.clone()).monospace()
                                        )
                                    }else {
                                        (
                                            RichText::new("Client Response:").strong().monospace().color(Color32::LIGHT_GREEN),
                                            RichText::new(item.message.clone()).monospace()
                                        )
                                    };
                                    
    
                                    if is_message_from_myself {
                                        ui.with_layout(Layout::from_main_dir_and_cross_align(
                                            Direction::RightToLeft,
                                            Align::Min,
                                        ), |ui| {
                                            Button::new(from)
                                                .fill(Color32::TRANSPARENT)
                                                .min_size(Vec2::new(30.0, 20.0))
                                                .frame(false)
                                                .sense(Sense::hover())
                                                .ui(ui);

                                            ui.add_space(max_msg_width / 1.1);

                                            let copy_btn = Button::new(RichText::new("🗐").weak().color(Color32::LIGHT_RED))
                                                .corner_radius(eframe::egui::CornerRadius::same(255)).small().min_size(Vec2::new(30.0, 14.0)).ui(ui)
                                                .on_hover_text(RichText::new("Copy Command"));

                                            if copy_btn.clicked(){
                                                ui.ctx().copy_text(item.message.to_string());
                                            }
                                        });
                                    } else {
                                        ui.with_layout(Layout::from_main_dir_and_cross_align(
                                            Direction::LeftToRight,
                                            Align::Min,
                                        ), |ui| {
                                            Button::new(from)
                                                .fill(Color32::TRANSPARENT)
                                                .min_size(Vec2::new(30.0, 20.0))
                                                .frame(false)
                                                .sense(Sense::hover())
                                                .ui(ui);

                                            ui.add_space(max_msg_width / 1.1);
                                            let btn = Button::new(RichText::new("🗐").small().weak().color(Color32::LIGHT_RED))
                                                .corner_radius(eframe::egui::CornerRadius::same(255)).small().min_size(Vec2::new(30.0, 14.0)).ui(ui);

                                            if btn.clicked(){
                                                ui.ctx().copy_text(item.message.clone());
                                            }
                                        });
                                    }
                                    note_frame.show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        let style = ui.style_mut();
                                        style.visuals.widgets.inactive.corner_radius = eframe::egui::CornerRadius::same(2);
                                        ui.label(txt);
                                        // egui_extras::syntax_highlighting::code_view_ui(
                                        //     ui, 
                                        //     &CodeTheme::dark(12.), 
                                        //     txt.text(), 
                                        //     "bash"
                                        // );
                                    });
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

                // After rendering, process the buffer
                // Note: Buffer processing is now handled in receive.rs when DONE is detected
            });
        });

    }
}