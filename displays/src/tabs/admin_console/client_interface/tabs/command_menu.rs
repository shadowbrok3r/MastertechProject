use crate::{tabs::admin_console::client_interface::WebSocketClient, Cmd};
use eframe::egui::{Button, Ui, Widget};

impl WebSocketClient {
    pub fn command_shell_menu(&mut self, ui: &mut Ui) {
        if Button::new("Tuneup").ui(ui).clicked(){
            log::info!("web_console -> websockets.rs -> Tuneup clicked");
            let _ = self.send_cmd_tx.try_send(Cmd::Tuneup);
            // self.ws_sender.send(WsMessage::Binary(serialize_command(&Cmd::Tuneup)));
            // self.history.push(format!("You\nCommand::Tuneup"));
        }
        
        if Button::new("CPS").ui(ui).clicked(){
            log::info!("web_console -> websockets.rs -> CPS clicked");
            let _ = self.send_cmd_tx.try_send(Cmd::Cps);
            // self.ws_sender.send(WsMessage::Binary(serialize_command(&Cmd::Cps)));
            // self.history.push(format!("You\nCommand::Cps\nChecking current antivirus"));
            self.input = "SELECT * FROM Win32_OperatingSystem".to_string();
        }

        if Button::new("SFC").ui(ui).clicked(){
            log::info!("web_console -> websockets.rs -> SFC clicked");
            let _ = self.send_cmd_tx.try_send(Cmd::SfcScan);
            // self.ws_sender.send(WsMessage::Binary(serialize_command(&Cmd::SfcScan)));
            // self.history.push(format!("You\nCommand::SfcScan"));
            self.input = "sfc /scannow".to_string();
        }

        if Button::new("Dism").ui(ui).clicked(){
            log::info!("web_console -> websockets.rs -> Dism clicked");
            let _ = self.send_cmd_tx.try_send(Cmd::DismScan);
            // self.ws_sender.send(WsMessage::Binary(serialize_command(&Cmd::DismScan)));
            // self.history.push(format!("You\nCommand::DismScan"));
            self.input = "dism /online /cleanup-image /scanhealth\ndism /online /cleanup-image /checkhealth\ndism /online /cleanup-image /restorehealth".to_string();
        }

        if Button::new("Chkdsk").ui(ui).clicked(){
            log::info!("web_console -> websockets.rs -> Chkdsk clicked");
            let _ = self.send_cmd_tx.try_send(Cmd::ChkDsk);
            // self.ws_sender.send(WsMessage::Binary(serialize_command(&Cmd::ChkDsk)));
            // self.history.push(format!("You\nCommand::ChkDsk"));
            self.input = "chkdsk /f /x /r".to_string();
            
        }

        if Button::new("Mbr2Gpt").ui(ui).clicked(){
            log::info!("web_console -> websockets.rs -> Mbr2Gpt clicked");
            let _ = self.send_cmd_tx.try_send(Cmd::Mbr2Gpt);
            // self.ws_sender.send(WsMessage::Binary(serialize_command(&Cmd::Mbr2Gpt)));
            // self.history.push(format!("You\nCommand::Mbr2Gpt"));
            self.input = "mbr2gpt /Convert /AllowFullOS /disk:0".to_string();
        }
    }
}