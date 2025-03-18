use crate::app_state::MastertechContext;
use eframe::egui::{Ui, CentralPanel};
use ratatui::{buffer::Buffer, widgets::Paragraph};

impl MastertechContext{
    pub fn egui_terminal(&mut self, ui: &mut Ui) {
        let maybe_buf = &mut Buffer::default();
        if let Some(ws_event) = self.shared_ctx.ws_receiver.try_recv() {
            match ws_event {
                ewebsock::WsEvent::Message(ws_message) => {
                    match ws_message {
                        ewebsock::WsMessage::Text(buf_string) => {
                            let buf = serde_json::from_str::<Buffer>(&buf_string);
                            if let Ok(buffer) = buf {
                                *maybe_buf = buffer;
                            }
                        },
                        _ => {}
                    }
                },
                _ => {}
            }
        }
        self.shared_ctx.terminal.draw(|f| {
            f.buffer_mut().merge(maybe_buf);
        }).unwrap();

        CentralPanel::default().show_inside(ui, |ui| {
            ui.add(self.shared_ctx.terminal.backend_mut());
        });
    }
}