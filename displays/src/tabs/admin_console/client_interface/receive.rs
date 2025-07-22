use crate::{tabs::admin_console::client_interface::serialize_command, virtual_filesystem::FileSysHelper, Cmd, FileSystemAction};
use database::schema::{Node, SystemInformation};
use ewebsock::{WsEvent, WsMessage};
use eframe::egui::Context;

use super::{deserializer, ui::WsDisplayState, History, WebSocketClient};

impl WebSocketClient {
    pub fn receive(&mut self, ctx: &Context) {
        self.explorer.receive();

        if let Ok(msg) = self.msg_to_client_rx.try_recv() {
            self.ws_sender.send(msg);
        }
        
        if let Some(event) = self.ws_receiver.try_recv() {
            match event{
                WsEvent::Message(msg) => {
                    match msg{
                        WsMessage::Binary(bin) => self.handle_binary_message(bin),
                        WsMessage::Text(text) => self.handle_text_message(text),
                        WsMessage::Pong(_) => {
                            // Update pong time and connection status
                            self.last_pong_time = Some(web_time::Instant::now());
                            self.is_connected = true;
                            self.connection_status = "Connected".to_string();
                        },
                        _ => {}
                    }
                },
                WsEvent::Opened => {
                    self.is_connected = true;
                    self.connection_status = "Connected".to_string();
                    self.history.push(History { 
                        from: "Client".to_string(), 
                        message: "Connection opened".to_string(), 
                        timestamp:  chrono::Local::now().to_rfc3339()
                    });
                    self.notifications += 1;
                },
                WsEvent::Closed => {
                    self.is_connected = false;
                    self.connection_status = "Disconnected".to_string();
                    self.last_pong_time = None;
                    self.history.push(History { 
                        from: "Client".to_string(), 
                        message: "Connection closed".to_string(), 
                        timestamp:  chrono::Local::now().to_rfc3339()
                    });
                    self.notifications += 1;
                },
                WsEvent::Error(err) => {
                    self.is_connected = false;
                    self.connection_status = format!("Error: {}", err);
                    self.history.push(History { 
                        from: "Client".to_string(), 
                        message: format!("Connection error: {}", err), 
                        timestamp:  chrono::Local::now().to_rfc3339()
                    });
                    self.notifications += 1;
                },
            }
        }
        
        if let Ok(state) = self.display_state_channel.1.try_recv() {
            self.state = state;
        }

        // Here we will handle commands we are going to SEND to Mastertech
        if let Ok(command) = self.send_cmd_rx.try_recv() {
            self.handle_command(command);
        }

        // Here we will handle commands we receive from Mastertech
        if let Ok(command) = self.receive_cmd_rx.try_recv() {
            ctx.request_repaint();
            if let Cmd::FileSystemAction(file_system_action) = command {
                self.helper_delegate.handle_filesystem_action(&file_system_action);
            }
        }
    }
    
    fn handle_command(&mut self, command: Cmd) {
        match command {
            Cmd::FileSystemAction(ref action) => {
                match action {
                    FileSystemAction::EnterDirectory(directory) => {
                        log::info!("web_console/websockets.rs -> EnterDirectory -> {directory:?}\nweb_console/websockets.rs -> EnterDirectory -> Root: {:?}", self.explorer.root);
                        log::info!("Prefix before double clicking folder: {}", self.explorer.current_prefix);
                        self.explorer.double_click_folder(&directory);
                        log::info!("After: {}", self.explorer.current_prefix);
                    },
                    FileSystemAction::GetNode(new_node) => {
                        log::info!("web_console/websockets.rs -> GetNode -> Root: {:?}", self.explorer.root); // {new_node:?}
                        if let Node::Folder(prefix, _) = new_node {
                            if &self.explorer.current_prefix == "current" {
                                self.explorer.current_prefix = prefix.clone();
                            }
                            log::info!("web_console/websockets.rs -> Current prefix: {}\nNew prefix: {}", self.explorer.current_prefix, prefix);
                        }
                        let insert_node = self.explorer.insert_node(new_node.clone());
                        log::info!("web_console/websockets.rs -> InsertNode -> {insert_node:?}");
                    },
                    FileSystemAction::RequestNewContents(directory) => {
                        log::info!("web_console/websockets.rs -> RequestNewContents -> {directory}");
                        log::info!("ACTION TO SEND: {command:?}");
                        self.ws_sender.send(WsMessage::Binary(serialize_command(&command)));
                    }
                    FileSystemAction::Execute(label) => { 
                        self.explorer.execute_file = label.clone(); 
                        if !label.is_empty() {
                            self.ws_sender.send(WsMessage::Binary(serialize_command(&command)));
                            self.interactive = true;
                            self.history.push(History { 
                                from: "Client".to_string(), 
                                message: "Switching to interactive mode".to_string(), 
                                timestamp:  chrono::Local::now().to_rfc3339()
                            });
                            self.notifications += 1;
                            let _ = self.display_state_channel.0.try_send(WsDisplayState::Shell);
                        }
                    },
                    FileSystemAction::Select((modifiers, label)) => {
                        if self.explorer.selected_items.borrow().contains(label) {
                            // If the item was already selected, deselect it
                            self.explorer.selected_items.borrow_mut().remove(label);
                        } 
                        if modifiers.ctrl { 
                            self.explorer.selected_items.borrow_mut().insert(label.clone());
                        } else { // If the control key is not down, clear previous selection and select the current item
                            self.explorer.selected_items.borrow_mut().clear();
                            self.explorer.selected_items.borrow_mut().insert(label.clone());
                        }
                        
                        
                        self.ws_sender.send(WsMessage::Binary(serialize_command(&command)));
                    },
                    FileSystemAction::ExpandDirectory(directory) => self.explorer.expand_folder(&directory),
                    FileSystemAction::NavigateHome => {
                        log::info!("web_console/websockets.rs -> NavigateHome");
                        // self.explorer.navigation_stack.clear();
                        // self.explorer.current_prefix.clear();
                    }
                    // FileSystemAction::CopyToClient(_) => todo!(),
                    // FileSystemAction::CopyFromClient(_) => todo!(),
                    // FileSystemAction::Delete(_) => todo!(),
                    FileSystemAction::PreviewedFile(file) => {
                        self.explorer.previewed_file = Some(file.to_string());
                    },
                    _ => {
                        self.ws_sender.send(WsMessage::Binary(serialize_command(&command)));
                    }
                }
            }
            Cmd::Quit => {
                self.interactive = false;
                self.ws_sender.send(WsMessage::Binary(serialize_command(&Cmd::Quit)));
            },
            _ => self.ws_sender.send(WsMessage::Binary(serialize_command(&command)))
        }
    }

    fn handle_binary_message(&mut self, bin: Vec<u8>) {
        match self.state {
            WsDisplayState::LiveStats => {
                // let bin = &self.handle_binary_message(bin);
                if let Some(sysinfo) = deserializer::<SystemInformation>(&bin){
                    log::info!("Got sysinfo from admin console");
                    self.resource_monitor.set_sysinfo(sysinfo);
                }
            },
            WsDisplayState::Terminal => {
                let _ = self.msg_from_client_tx.try_send(WsMessage::Binary(bin));
            },
            _ => {
                if let Some(cmd) = deserializer::<Cmd>(&bin){
                    let _ = self.receive_cmd_tx.try_send(cmd);
                } else if bin.len() > 0 {
                    self.loading = false;
                    let msg = String::from_utf8_lossy(&bin).to_string();

                    // Check if the incoming message ends with "DONE"
                    if msg.trim().ends_with("DONE") {
                        // Remove the DONE marker and add the content to buffer
                        let content = msg.trim_end_matches("DONE").trim();
                        if !content.is_empty() {
                            self.buffer.push_str(content);
                            self.buffer.push('\n');
                        }
                        
                        // Push the buffered content as a new history entry
                        if !self.buffer.is_empty() {
                            self.history.push(History {
                                from: "Client".to_string(),
                                message: self.buffer.trim().to_string(),
                                timestamp: chrono::Local::now().to_rfc3339(),
                            });
                            self.buffer.clear(); // Clear the buffer after processing
                            self.notifications += 1;
                        }
                    } else if msg.is_ascii() {
                        log::info!("Message that is ascii: {msg:?}");
                        // Append the incoming message to the buffer with a newline
                        self.buffer.push_str(&msg);
                        if !msg.ends_with('\n') {
                            self.buffer.push('\n');
                        }
                    } else {
                        // log::error!("Message not handled: {msg:?}");
                    }
                }        
            }
        }
    }

    fn handle_text_message(&mut self, text: String) {
        if text.eq("Closed") {
            self.ws_sender.close();
        }
        
        self.loading = false;
        log::info!("Text data: {text:#?}");
    
        // Append the incoming text to the buffer
        self.buffer.push_str(&text);
    
        // Process the buffer for complete lines
        while let Some(pos) = self.buffer.find('\n') {
            // Extract the complete line up to the newline character
            let line = self.buffer.drain(..=pos).collect::<String>().trim_end().to_string();
    
            // Create a new history entry for the extracted line
            let history = History {
                from: "Client".to_string(),
                message: line,
                timestamp: chrono::Local::now().to_rfc3339(),
            };
    
            // Add to history
            self.history.push(history);
            self.notifications += 1;
        }
    }
}