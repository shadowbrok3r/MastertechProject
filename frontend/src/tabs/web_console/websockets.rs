
use std::{borrow::BorrowMut, collections::{HashMap, VecDeque}, fmt::Display};
use crossbeam::channel::Sender;
use database::schema::ConnectedClient;
use egui::{epaint::Shadow, Align, Button, CentralPanel, CollapsingHeader, Color32, Direction, Frame, Key, Label, Layout, Margin, Rect, RichText, Rounding, ScrollArea, Shape, Stroke, TextEdit, TopBottomPanel, Ui, Vec2, Vec2b, Widget};
use egui_extras::{Size, Strip, StripBuilder};
use ewebsock::{WsEvent, WsMessage, WsReceiver, WsSender};
use serde::{Deserialize, Serialize};
use log::info;

use crate::utilities::ColumnLayout;

use super::charts::LinePlot;

pub struct ClientDisplay{
    pub clients: HashMap<String, ConnectedClient>,
    pub client_names: Vec<String>,
    pub connected_client: Option<ConnectedClient>,
    pub client_connected: HashMap<String, bool>,
    pub websocket_client: Option<WebSocketClient>,
}

pub struct WebSocketClient {
    pub ws_sender: WsSender,
    pub ws_receiver: WsReceiver,
    pub events: Vec<WsEvent>,
    pub input: String,
    pub messages: Vec<String>,

    pub cpu_clock: VecDeque<f32>,
    pub temps: VecDeque<HashMap<String, f32>>,
    pub cpu_percentage: VecDeque<f32>,
    pub ram_usage: VecDeque<f32>,

    pub sysinfo: Option<SystemInformation>,
    pub history: Vec<String>
}

impl WebSocketClient{
    pub fn new(ws_sender: WsSender, ws_receiver: WsReceiver) -> Self {
        Self{
            ws_sender,
            ws_receiver,
            events: Default::default(),
            input: String::new(),
            messages: Vec::new(),

            sysinfo: None,
            cpu_clock: VecDeque::new(),
            cpu_percentage: VecDeque::new(),
            ram_usage: VecDeque::new(),
            history: Vec::new(),
            temps: VecDeque::new(),
        }
    }
    
    pub fn handle_events(&mut self) {

        while let Some(event) = self.ws_receiver.try_recv() {
            self.events.push(event);
        }

        for event in &self.events {
            match event{
                WsEvent::Message(msg) => {
                    match msg{
                        WsMessage::Binary(bin) => {
                            let sys = deserialize_system_info(bin);
                            self.sysinfo = Some(deserialize_system_info(bin));

                            let normalized_cpu_percentage = normalize(sys.cpu_percentage, 0.0, 100.0);
                            let normalized_cpu_clock = normalize(sys.cpu_clock, 0.0, 5000.0); // Example range for CPU clock
                            let normalized_temps: Vec<f32> = sys.component_temps.values().map(|&temp| normalize(temp, 0.0, 100.0)).collect();
                            let normalized_ram_usage = normalize(sys.used_memory, 0.0, 16000.0); // Example range for RAM usage
    
                            if self.cpu_percentage.len() < 30
                                // || self.component_temps.len() < 30
                                || self.cpu_clock.len() < 30
                                || self.ram_usage.len() < 30 {
                                self.cpu_percentage.push_back(normalized_cpu_percentage);
                                // self.component_temps.push_back(normalized_temps.iter().sum::<f32>() / normalized_temps.len() as f32); // Average temperature
                                self.cpu_clock.push_back(normalized_cpu_clock);
                                self.ram_usage.push_back(normalized_ram_usage);
                            } else {
                                self.cpu_percentage.pop_front();
                                self.cpu_percentage.push_back(normalized_cpu_percentage);
                                // self.component_temps.pop_front();
                                // self.component_temps.push_back(normalized_temps.iter().sum::<f32>() / normalized_temps.len() as f32); // Average temperature
                                self.cpu_clock.pop_front();
                                self.cpu_clock.push_back(normalized_cpu_clock);
                                self.ram_usage.pop_front();
                                self.ram_usage.push_back(normalized_ram_usage);
                            }
                        },
                        WsMessage::Text(txt) => {
                            info!("Text data: {txt:#?}");
                            self.history.push(txt.clone());
                        },
                        WsMessage::Unknown(unknown) => {
                            info!("unknown data: {unknown:#?}");
                        },
                        _ => {}
                    }
                },
                _ => {}
            }
        }
        self.events.clear();
    }
    
    pub fn show(&mut self, mut strip: Strip, name: String) {
        self.handle_events();

        strip.strip(|strip| 
        {
            strip.sizes(Size::remainder(), 4)
                .horizontal(|mut s| 
            {
                s.cell(|ui|{
                    let _tuneup = Button::new("Tuneup").ui(ui);
                });
                s.cell(|ui|{
                    let _cps = Button::new("CPS").ui(ui);
                });
                s.cell(|ui|{
                    let _qc = Button::new("QC").ui(ui);
                });
                s.cell(|ui|{
                    let live_data = Button::new("Live Data").ui(ui);
                    if live_data.clicked(){
                        self.ws_sender.send(WsMessage::Text("live_data".to_string()));
                    }
                });
            });
        });

        strip.cell(|ui | 
        {
            let client_id = ui.make_persistent_id(format!("client_id {:?}", name.clone()));
            let client_header = CollapsingHeader::new("Charts").id_source(client_id);
            client_header.show_background(true).show_unindented(ui, |ui| 
            {
                ui.add_space(10.0);
                ui.vertical_centered(|ui| {
                    if let Some(sysinfo) = &self.sysinfo {
                        let percentages = self.cpu_percentage.make_contiguous().to_owned();
                        let clocks = self.cpu_clock.make_contiguous().to_owned();
                        // let temps = self.component_temps.make_contiguous().to_owned();
                        let ram = self.ram_usage.make_contiguous().to_owned();
                
                        let mut cpu_usage_plot = LinePlot::new(&[0.0], &percentages.as_slice());
                        let mut cpu_clock_plot = LinePlot::new(&[0.0], &clocks.as_slice());
                        // let temps_plot = LinePlot::new(&[0.0], &temps.as_slice());
                        let mut ram_usage_plot = LinePlot::new(&[0.0], &ram.as_slice());
                
                        cpu_usage_plot.ui(ui, "CPU Usage", cpu_usage_plot.line("CPU Usage (%)", Color32::from_rgb(170, 10, 150)));
                        cpu_clock_plot.ui(ui, "CPU Clock", cpu_clock_plot.line("CPU Clock (MHz)", Color32::from_rgb(21, 232, 165)));
                        // temps_plot.ui(ui, "System Temps", temps_plot.line("System Temps (°C)", Color32::from_rgb(255, 69, 0)));
                        ram_usage_plot.ui(ui, "RAM Usage", ram_usage_plot.line("RAM Usage (MB)", Color32::from_rgb(0, 191, 255)));
                    }
                });
                ui.add_space(10.0);
            });

            let client_id = ui.make_persistent_id(format!("history {:?}", name.clone()));
            let scroll = CollapsingHeader::new("Shell").id_source(client_id);

            scroll.show_background(true).show_unindented(ui, |ui| 
            {
                ScrollArea::vertical()
                    .auto_shrink(false)
                    .stick_to_bottom(true)
                    .show(ui, |ui | 
                {
                    ui.set_width(ui.available_width());
                    let max_msg_width = ui.available_width() / 2.5;

                    for item in self.history.iter(){
                        let is_message_from_myself = if item.contains("You"){
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

                            let content = RichText::new(item);
                            let mut msg_width = measure(content.clone());

                            let width = measure(content.clone());
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
                                .stroke(Stroke::new(1.0, Color32::from_additive_luminance(100)))
                                .show(ui, |ui| {
                                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                                        Label::new(item).selectable(true).ui(ui);
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
                ui.vertical_centered_justified(|ui| {
                    let text_edit = TextEdit::singleline(&mut self.input).hint_text("Send command").ui(ui);
                    let key_press = ui.input(|i| i.key_pressed(Key::Enter));
                    if text_edit.lost_focus() && key_press {
                        text_edit.request_focus();
                        self.history.push(format!("You\n{}", self.input.clone()));
                        self.ws_sender.send(WsMessage::Text(std::mem::take(&mut self.input)));
                    }
                });
            });
        });
        // strip.empty();
    }
}

pub enum ClientConnection{
    ClientUrl(String)
}

impl ClientDisplay{
    pub fn new(clients: HashMap<String, ConnectedClient>) -> Self { 
        let mut client_names = Vec::new();
        for (name, _) in clients.iter(){
            client_names.push(name.clone());
        }

        Self {
            clients,
            client_names,
            connected_client: None,
            client_connected: HashMap::new(),
            websocket_client: None
        }
    }

    pub fn new_client(clients: HashMap<String, ConnectedClient>, websocket_client: WebSocketClient) -> Self { 
        let mut client_names = Vec::new();
        for (name, _) in clients.iter(){
            client_names.push(name.clone());
        }

        Self {
            clients,
            client_names,
            connected_client: None,
            client_connected: HashMap::new(),
            websocket_client: Some(websocket_client)
        }
    }

    pub fn layout_cols(
        &mut self,
        ui: &mut egui::Ui,
        tx: Sender<ClientConnection>
    ){
        let mut shadow = Shadow::default();
        shadow.blur = 10.0;
        shadow.spread = 2.0;
        shadow.color = Color32::from_rgb_additive(20, 1, 20);
        let mut outer_margin = Margin::default();
        outer_margin.left = 8.0;
        let mut inner_margin = Margin::default();
        inner_margin.top = 2.0;
        inner_margin.left = 2.0;
        inner_margin.right = 2.0;

        let panel_frame = Frame::default()
            .fill(Color32::from_rgb(8, 7, 10))
            .inner_margin(Margin::same(5.0))
            .rounding(Rounding::same(5.0))
            .shadow(shadow)
            .stroke(Stroke::new(1.0, Color32::from_rgb_additive(20, 1, 20)));

        ui.style_mut().visuals.window_rounding = Rounding::same(10.0);
        let column_width = Size::exact(450.0);
        
        CentralPanel::default().frame(panel_frame)
            .show_inside(ui, |ui| 
        {
            ScrollArea::horizontal()
                .show_viewport(ui, |ui, _|
            {
                let x: f32 = ui.available_height() - 40.0;
                StripBuilder::new(ui)
                    .cell_layout(Layout::top_down_justified(egui::Align::Center))
                    .size(Size::exact(30.0))
                    .size(Size::exact(5.0))
                    .size(Size::exact(x))
                    .vertical(|mut strip| 
                {
                    strip
                        .strip(|strip| 
                    {
                        strip
                            .sizes(column_width, self.client_names.len())
                            .horizontal( |strip| self.headers(strip, tx.clone()));
                    });
                    strip.empty();
                    strip
                        .strip(|strip| 
                    {
                        strip
                            .sizes(column_width, self.client_names.len())
                            .horizontal( |mut strip| 
                        {
                            self.columns(
                                strip.borrow_mut(),
                            );
                        });
                    });
                });
            });
        });
    }

    pub fn columns(&mut self, strip: &mut egui_extras::Strip) {
        for (name, _) in self.clients.iter(){
            let color = if *self.client_connected.get(name).unwrap_or(&false){
                Color32::GREEN
            }else{
                Color32::RED
            };
            let column_frame = Frame::default().fill(Color32::from_rgb(12, 12, 18))
                .inner_margin(Margin::same(4.0)).rounding(Rounding::same(10.0))
                .stroke(Stroke::new(1.0, Color32::from_additive_luminance(150)));

            strip.strip(|s | 
            {
                s
                    .size(Size::remainder())
                    .vertical(|mut s| 
                {
                    s.cell(|ui| 
                    {
                        column_frame.show(ui, |ui| {
                            ui.vertical_centered_justified(|ui| {
                                ScrollArea::vertical()
                                    .auto_shrink(false)
                                    .show_viewport(ui, |ui, _| 
                                {
                                    let height = ui.available_height();
                                    StripBuilder::new(ui)
                                        .size(Size::exact(25.0))
                                        .size(Size::remainder().at_most(height - 50.0))
                                        // .size(Size::initial(height))
                                        // .size(Size::exact(25.0))// 
                                        .vertical(| strip| 
                                    {
                                        if let Some(ws_client) = &mut self.websocket_client{
                                            ws_client.show(strip, name.clone());
                                        }
                                    });
                                });
                            });
                        });
                    });
                });
            });
        }
    }

    pub fn headers(&mut self, mut s: egui_extras::Strip, tx: Sender<ClientConnection>) {
        let header_frame = Frame::default()
            .fill(Color32::from_rgb(12, 12, 18))
            .inner_margin(Margin::same(4.0))
            .outer_margin(Margin::symmetric(0.0, 0.0))
            .rounding(Rounding::same(5.0))
            .stroke(Stroke::new(1.0, Color32::from_additive_luminance(50)));

        for (name, client) in self.clients.iter(){
            s.cell(|ui|
            {
                header_frame.show(ui, |ui|
                {
                    ui.horizontal_top(|ui| 
                    {
                        ui.with_layout(Layout::left_to_right(Align::Min), 
                        |ui| ui.add_space(ui.available_width() / 4.0));

                        ui.with_layout(Layout::left_to_right(Align::Center), 
                        |ui| ui.colored_label(Color32::WHITE, RichText::new(name.to_owned()).heading()));
                        
                        ui.with_layout(Layout::right_to_left(Align::Max), |ui| 
                        {
                            let button = Button::new(
                                RichText::new("⮫")
                                    .raised()
                                    .color(Color32::LIGHT_RED)
                                )
                                .fill(Color32::TRANSPARENT)
                                .min_size(Vec2::new(30.0, 20.0))
                                .ui(ui);

                            ui.add_space(30.0);

                            if button.clicked(){ // CONNECT
                                let url = format!("ws://127.0.0.1:8081/websocket?role=master&room_id={}", name.clone());
                                self.connected_client = Some(client.clone());
                                self.client_connected.clear();
                                self.client_connected.insert(name.clone(), true);
                                tx.send(ClientConnection::ClientUrl(url)).unwrap();
                            }
                        });
                    });
                });
            });
        }
    }
}


pub fn serialize_system_info(system_info: &SystemInformation) -> Vec<u8> {
    bincode::serialize(system_info).expect("Failed to serialize SystemInformation")
}

pub fn deserialize_system_info(bytes: &[u8]) -> SystemInformation {
    bincode::deserialize(bytes).expect("Failed to deserialize SystemInformation")
}

fn normalize(value: f32, min: f32, max: f32) -> f32 {
    (value - min) / (max - min)
}


#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SystemInformation {
    /// Live CPU usage as a percentaget
    pub cpu_percentage: f32,
    /// Live CPU clock speed
    pub cpu_clock: f32,
    /// Live system temps
    pub component_temps: HashMap<String, f32>,
    /// Live RAM usage in Mb
    pub used_memory: f32,
    /// Total RAM
    pub total_memory: f32,
    /// Disk usage
    pub disks: String,
    /// Name of machine
    pub name: String,
    /// Kernel version
    pub kernel_version: String,
    /// OS version
    pub os_version: String,
    /// Hostname based on DNS
    pub hostname: String,
    /// Number of Physical CPU's
    pub number_of_cpus: String,

    pub network_interfaces: HashMap<String, String>,
}

impl Display for SystemInformation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "==> cpu_percentage: {} \n==> comps: {:?} \n==> used_memory: {} \n==> total_memory: {} \n==> disks: {} \n==> name: {} \n==> kernel_version: {} \n==> os_version: {} \n==> hostname: {} \n==> number_of_cpus: {} \n==> network_interfaces: {:#?} \n", 
            self.cpu_percentage,
            self.component_temps,
            self.used_memory,
            self.total_memory,
            self.disks,
            self.name,
            self.kernel_version,
            self.os_version,
            self.hostname,
            self.number_of_cpus,
            self.network_interfaces,
        )
    }
}