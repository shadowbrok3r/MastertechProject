#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide output_console window on Windows in release
use sysinfo::*;
use std::{collections::HashSet, borrow::BorrowMut}; //, os::windows::thread};
use eframe::egui;
use egui::*;
use egui_dock::{DockArea, Node, NodeIndex, Style, TabViewer, Tree};
use scaffold_builder::PulledKeys;
use tokio::runtime::Handle;
use egui_extras::*;
use catppuccin_egui::MOCHA;

mod request;
mod data_transfer;
mod scaffold_builder;

#[tokio::main]
async fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        initial_window_size: Some(egui::vec2(925.0, 700.0)),
        ..Default::default()
    };
    eframe::run_native(
        "Mastertech",
        options,
        Box::new(|_cc| Box::<MasterTechApp>::default()),
    )
}

#[derive(Debug, PartialEq)]
enum Salesman {
    Jake,
    Danny
}
#[derive(Debug, PartialEq)]
enum Techs{
    Logan,
    Bread,
    Taco
}
#[derive(Debug, PartialEq)]
enum HardwareTest{
    RamPass,
    RamFail,
    RamNotTested,
    HddPass,
    HddFail,
    HddNotTested,
    SsdPass,
    SsdFail,
    SsdNotTested,
}

impl HardwareTest{
    fn as_str(&self) -> &'static str {
        match *self {
            HardwareTest::RamPass => "RAM Pass",
            HardwareTest::RamFail => "RAM Fail",
            HardwareTest::HddPass => "HDD Pass",
            HardwareTest::HddFail => "HDD Fail",
            HardwareTest::SsdPass => "SSD Pass",
            HardwareTest::SsdFail => "SSD Fail",
            HardwareTest::RamNotTested => "RAM not tested",
            HardwareTest::HddNotTested => "HDD not tested",
            HardwareTest::SsdNotTested => "SSD not tested",
        }
    }
}

enum SendReceiveMessage{
    TicketInfo(scaffold_builder::TicketInformation),
    Cpskeys(PulledKeys),
    Error(String)
}

enum SendReceiveSystemInfo{
    RetrieveSystemInfo(SystemInformation),
    Error(String)
}
pub struct SystemInformation{
    cpu_name: String,
    total_ram: String,
    system_name: String,
    disks: Option<String>
}
pub struct SendAsyncReq {
    tx: Option<std::sync::mpsc::Sender<SendReceiveMessage>>,
    system_info_tx: Option<std::sync::mpsc::Sender<SendReceiveSystemInfo>>,
}

impl SendAsyncReq{
    fn get_ticket(so_number: String, tx: Option<std::sync::mpsc::Sender<SendReceiveMessage>>){
        let handle = Handle::current();
        
        std::thread::spawn(move||{
            handle.block_on(async{
                let args = vec![
                    serde_json::json!(so_number),
                    serde_json::json!("false"),
                ];
            
                let scaffold_builder = scaffold_builder::ScaffoldRequestBuilder{
                    app: scaffold_builder::ScaffoldApps::Everest,
                    action: scaffold_builder::ScaffoldActions::EverestCall, 
                    call: Some(scaffold_builder::ScaffoldCalls::GetOrder), 
                    arguments: Some(args.clone())
            
                };

                let response = request::request_ticket_info(scaffold_builder).await;
                //println!("response: {:?}", response);
                match response { // Successfully received GetTicketResponse
                    Ok(get_ticket_response) => {
                        // You can now use fields of get_ticket_response
                        let header = &get_ticket_response.header;
                        let customer = &get_ticket_response.customer;
                        let addresses = &get_ticket_response.addresses.address_object;
                        let items_objects = get_ticket_response.items;
                        //let transactions = &get_ticket_response.transactions;

                        let mut checkin_note = "".to_string();
                        let mut itemcodes = "".to_string();

                        // DW_UPDATE_DATE is the exact time that the line item (AKA 'items') was added.
                        // iterates through the array of objects, gets note if not null and not empty, parses, assigns to checkin_note
                        for object in items_objects{

                            // If i want to....
                            // "COST": "7.100000", this is our cost
                            // ITEM_PR_FEX is what we charge the customer, although AMOUNT is the same value
                            object.get("NOTE")
                            .and_then(|v| v.as_str())
                            .map(|note| {
                                if note != "null" && !note.is_empty() {
                                    let parts: Vec<&str> = note.split("Symptoms (Details):").collect();
                                    if parts.len() > 1{
                                        let note = &parts[1].to_string();
                                        checkin_note = note.to_string();
                                    }
                                }
                            });

                            object.get("ITEM_CODE")
                            .and_then(|v| v.as_str())
                            .map(|item_code| {
                                itemcodes += item_code;
                            });
                        }

                        let ticket_information = scaffold_builder::TicketInformation{
                            cust_code: header.CUST_CODE.clone(),
                            user_id: header.USER_ID.clone(),
                            customer_phone_1: addresses.TEL1.clone(),
                            customer_phone_2: addresses.TEL2.clone(),
                            customer_email: addresses.EMAIL.clone(),
                            last_invoice_amount: customer.LI_AMT.clone(),
                            terms: header.TERMS.clone(),
                            doc_alias: header.DOC_ALIAS.clone(),
                            department: header.DEP.clone(),
                            jurisdiction: header.JURISCODE.clone(),
                            invoice_amnt: header.INV_AMOUNT.clone(),
                            customer_name: customer.NAME.clone(),
                            checkin_notes: checkin_note.clone(),
                            last_invoice_number: customer.LI_DOC.clone(),
                            item_codes: itemcodes.clone(),
                            //last_tuneup_date: customer.LAST_TUNEUP_DATE.clone(),
                            //last_checkin_date: customer.LI_AMT.clone(),
                            total_invoice_count: customer.NUM_INV.clone(),
                        };

                        match tx{
                            Some(tx) => {
                                if let Err(e) = tx.send(SendReceiveMessage::TicketInfo(ticket_information)) {
                                    tx.send(SendReceiveMessage::Error(e.to_string()));
                                }
                            }
                            None => {eprintln!("Tried to send an update, but the sender is None");}
                        }
                    },
                    Err(e) => {// There was an error while making the request
                        match tx{
                            Some(tx) => {
                                if let Err(e) = tx.send(SendReceiveMessage::Error(e.to_string())) {
                                    println!("ERROR: \n\n\n{:?}", e);
                                }
                            }
                            None => { eprintln!("Tried to send an update, but the sender is None");}
                        }
                    },
                }
            });
        }); 
    }
    fn get_cps(so_number: String, tx: Option<std::sync::mpsc::Sender<SendReceiveMessage>>){
        let handle = Handle::current();
        
        std::thread::spawn(move||{
            handle.block_on(async{
                let args = vec![
                    serde_json::json!(so_number),
                ];
            
                let scaffold_builder = scaffold_builder::ScaffoldRequestBuilder{
                    app: scaffold_builder::ScaffoldApps::SoftwareLicenseFetch,
                    action: scaffold_builder::ScaffoldActions::FetchKeys, 
                    call: Some(scaffold_builder::ScaffoldCalls::None),
                    arguments: Some(args.clone())
                };
                
                let response = request::request_keys(scaffold_builder).await;

                match response { // Successfully received GetTicketResponse
                    Ok(get_keys_response) => {

                        let webroot_key = &get_keys_response.webroot_key;
                        let superanti_key = &get_keys_response.superanti_key;

                

                        let cps_keys = PulledKeys{
                            webroot_key: webroot_key.to_string(),
                            superanti_key: superanti_key.to_string()
                        };



                        match tx{
                            Some(tx) => {
                                if let Err(e) = tx.send(SendReceiveMessage::Cpskeys(cps_keys)) {
                                    tx.send(SendReceiveMessage::Error(e.to_string()));
                                }
                            }
                            None => {eprintln!("Tried to send an update, but the sender is None");}
                        }
                    },
                    Err(e) => {// There was an error while making the request
                        match tx{
                            Some(tx) => {
                                if let Err(e) = tx.send(SendReceiveMessage::Error(e.to_string())) {
                                    println!("ERROR: \n\n\n{:?}", e);
                                }
                            }
                            None => { eprintln!("Tried to send an update, but the sender is None");}
                        }
                        
                        //let mut output_text = output_text_clone.lock().unwrap();
                        //*output_text = format!("Error: {}",e);

                    },
                }
            });
        });
    }

    fn get_system_specs(tx: Option<std::sync::mpsc::Sender<SendReceiveSystemInfo>>){
        let handle = Handle::current();
        
        std::thread::spawn(move||{
            handle.block_on(async{
                let mut sys = System::new_all(); // Create `System` struct.

                let cpu_brand = sys.cpus()[0].brand().to_string();
                let ram = (sys.total_memory() / ( 1024 * 1024 * 1024)).to_string();
                let system = sys.long_os_version().unwrap_or_else(|| "<unknown>".to_owned());
                let disks = sys.disks();
                let disks_clone = disks.clone();

                //let mount_point = Option<"">;
                let available_disk_space = "";
                let total_disk_space = "";

                for disk in disks_clone{
                    if !disk.is_removable(){
                        let mount_point = disk.mount_point().to_str();
                        mount_point.map(|string|{
                            println!("Strings: {:?}", string);
                        });
                    }
                    
                }

                // let system_info = SystemInformation{
                //     cpu_name: cpu_brand,
                //     total_ram: ram,
                //     system_name: system,
                //     disks:
                // };

                println!("CPU: {:#?}", cpu_brand);
                println!("ram: {:#?}", ram);
                println!("system: {:#?}", system);
                println!("disks: {:#?}", disks);
            });
        });
    }
}

struct MastertechContext {
    //////////////////////////////////////////
    /*          Mastertech Vars             */
    //////////////////////////////////////////
    so_number: String,
    customer_name: String,
    phone1: String,
    phone2: String,
    salesman_cbox: Salesman,
    techs_cbox: Techs,
    ram_test_cbox: HardwareTest,
    hdd_test_cbox: HardwareTest,
    ssd_test_cbox: HardwareTest,
    checkin_notes: String,
    webroot_key: String,
    superanti_key: String,
    recommendations: String,
    checkin_rep: String,
    output_text: String,
    rx: Option<std::sync::mpsc::Receiver<SendReceiveMessage>>,
    system_info_rx: Option<std::sync::mpsc::Receiver<SendReceiveSystemInfo>>,

    //////////////////////////////////////////
    /*          Widgets and UI elements     */
    //////////////////////////////////////////
    widget_size: f32,
    open_tabs: HashSet<String>,
    show_close_buttons: bool,
    show_add_buttons: bool,
    draggable_tabs: bool,
    show_tab_name_on_hover: bool,
    tur_sheet_tab: String,
    output_console_tab: String,
    system_info_tab: String,
    scripts_tab: String,
    date: Option<chrono::NaiveDate>,
    send_specs: bool,
    animate_progress_bar: bool,
    get_ticket_button_pressed: bool,
    get_cps_button_pressed: bool,
    copy_webroot_button_pressed: bool,
    copy_sas_button_pressed: bool,
    get_seb_button_pressed: bool,
    first_run: bool,
    get_specs: bool,

    //////////////////////////////////////////
    /*          UI Colors                   */
    //////////////////////////////////////////
    style: Option<egui_dock::Style>,
    text_color: Color32,
    border_stroke_color: Stroke,
    bg_color: Color32
}

struct MasterTechApp {
    context: MastertechContext,
    tree: Tree<String>,
    send_async_req: SendAsyncReq,
}

impl TabViewer for MastertechContext {
    type Tab = String;

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {

        match tab.as_str() {
            "TUR Sheet" => self.tur_sheet(ui),
            "Console" => self.output_console(ui),
            "Scripts" => self.scripts(ui),
            "System Information" => self.system_information(ui),
            _ => {
                let sysinfo_tab = &self.system_info_tab.to_string();
                if ui.label(tab.as_str()).clicked(){
                    if tab.as_str() == sysinfo_tab{
                        self.first_run = true;
                    }
                };
            }
        }
    }

    fn context_menu(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        match tab.as_str() {
            "TUR Sheet" => self.simple_demo_menu(ui),
            _ => {
                ui.label(tab.to_string());
                ui.label("This is a context menu");
            }
        }
    }
    fn title(&mut self, tab: &mut Self::Tab) -> WidgetText {
        tab.as_str().into()
    }
    fn on_close(&mut self, tab: &mut Self::Tab) -> bool {
        self.open_tabs.remove(tab);
        true
    }
    fn on_add(&mut self, _node: NodeIndex) {
        //self.open_tabs.add(tab)
    }
}

impl Default for MasterTechApp {
    fn default() -> Self {
        // Create a watch channel with a default value
        let (tx, rx) = std::sync::mpsc::channel::<SendReceiveMessage>();
        let (tx_system, rx_system) = std::sync::mpsc::channel::<SendReceiveSystemInfo>();
        let mut tree = Tree::new(vec!["TUR Sheet".to_owned(), "Empty".to_owned()]);
        let [a, b] = tree.split_left(NodeIndex::root(), 0.3, vec!["Scripts".to_owned(), "System Information".to_owned()]);
        let [_, _] = tree.split_below(
            a,
            0.7,
            vec!["Console".to_owned()],
        );

        let [_, _] = tree.split_below(b, 0.5, vec!["Empty1".to_owned()]);

        let mut open_tabs = HashSet::new();

        for node in tree.iter() {
            if let Node::Leaf { tabs, .. } = node {
                for tab in tabs {
                    open_tabs.insert(tab.clone());
                }
            }
        }
        
        let send_async_req = SendAsyncReq{
            tx: Some(tx),
            system_info_tx: Some(tx_system),
        };


        let context = MastertechContext {
            //////////////////////////////////////////
            /*          Mastertech Vars             */
            //////////////////////////////////////////
            so_number: "".to_string(),
            customer_name: "".to_string(),
            phone1: "".to_string(),
            phone2: "".to_string(),
            salesman_cbox: Salesman::Jake,
            techs_cbox: Techs::Logan,
            ram_test_cbox: HardwareTest::RamNotTested,
            hdd_test_cbox: HardwareTest::HddNotTested,
            ssd_test_cbox: HardwareTest::SsdNotTested,
            checkin_notes: "".to_string(),
            webroot_key: "".to_string(),
            superanti_key: "".to_string(),
            recommendations: "".to_string(),
            send_specs: false,
            checkin_rep: "Checkin Rep: ".to_string(),
            output_text: "".to_string(),
            rx: Some(rx),
            system_info_rx: Some(rx_system),

            //////////////////////////////////////////
            /*          Widgets and UI elements     */
            //////////////////////////////////////////
            widget_size: 135.0,
            open_tabs,
            show_close_buttons: true,
            show_add_buttons: true,
            draggable_tabs: true,
            show_tab_name_on_hover: false,
            tur_sheet_tab: "TUR Sheet".to_string(),
            output_console_tab: "Console".to_string(),
            system_info_tab: "System Information".to_string(),
            scripts_tab: "Scripts".to_string(),
            date: None,
            animate_progress_bar: false,
            get_ticket_button_pressed: false,
            get_cps_button_pressed: false,
            copy_webroot_button_pressed: false,
            copy_sas_button_pressed: false,
            get_seb_button_pressed: false,
            first_run: true,
            get_specs: false,

            //////////////////////////////////////////
            /*          UI Colors                   */
            //////////////////////////////////////////
            style: None,
            text_color: Color32::from_rgb(255, 204, 230),//(200,200,200),
            bg_color: Color32::from_rgb(28,30,36),
            border_stroke_color: Stroke::new(1.0, Color32::from_rgb_additive(150, 62, 124))
        };

        Self { context, tree, send_async_req }
    }
}

impl MastertechContext {
    fn simple_demo_menu(&mut self, ui: &mut Ui) {
        ui.label("Egui widget example");
        ui.menu_button("Sub menu", |ui| {
            ui.label("hello :)");
        });
    }

    fn tur_sheet(&mut self, ui: &mut Ui) {
        ui.visuals_mut().override_text_color = Some(self.text_color);
        ui.style_mut().spacing.button_padding = (6.0, 3.0).into();
        ui.set_min_width(600.0);
        ui.set_max_height(600.0);
        ui.shrink_width_to_current();
        ui.shrink_height_to_current();
        ui.painter().rect_filled(ui.available_rect_before_wrap(),10.0,self.bg_color);
        ui.painter().rect_stroke(ui.available_rect_before_wrap(),10.0, self.border_stroke_color);
        
        ui.indent("indented", |ui|{
            ui.with_layout(Layout::top_down_justified(Align::Center),|ui|{
                ui.columns(2, |columns|{
                    columns[0].vertical_centered(|ui|{

                        ui.vertical(|ui| {ui.add_space(8.0);});

                            ui.set_min_width(self.widget_size*2.0+5.0);
                            ui.set_max_width(self.widget_size*2.0+6.0);

                            StripBuilder::new(ui)
                            .cell_layout(Layout::top_down_justified(Align::Center))
                            .size(Size::remainder()) //for the initial textedits
                            .size(Size::relative(0.57)) // for the checkin notes
                            .vertical(|mut strip|{
                                strip.cell(|ui|{
                                    ui.set_max_width(self.widget_size * 2.0 + 7.0);
                                    ui.group(|ui|{
                                        ui.vertical_centered(|ui|{
                                            if ui.add(Button::new(RichText::new("Get Ticket")
                                            .color(Color32::from_rgb(255, 204, 255))
                                            .strong()
                                            .italics())
                                            .stroke(Stroke::new(2.0, Color32::from_rgb(191, 33, 101)))
                                            .min_size(vec2(self.widget_size * 2.0 + 7.0, 7.0)))
                                            .clicked(){ 
                                                self.get_ticket_button_pressed = true; // Sets bool to true so the main loop runs the get_ticket function
                                            }
                                        }); 
                                        
                                        ui.vertical(|ui| {ui.add_space(3.0);});
            
                                        Grid::new("ticket_information")
                                        .spacing(vec2(6.0, 8.0))
                                        .min_col_width(self.widget_size)
                                        .max_col_width(self.widget_size + 5.0)
                                        .num_columns(2)
                                        .show(ui, |ui| {
                                            
                                                                /*     ROW 1     */
                                            ui.add(TextEdit::singleline(&mut self.so_number)
                                            .hint_text("Service #  ").char_limit(8).desired_width(self.widget_size));

                                            let x = ui.add(TextEdit::singleline(&mut self.customer_name)
                                            .hint_text("Customer Name  ").desired_width(self.widget_size + 3.0));

                                            ui.end_row();

                                                                /*     ROW 2     */
                                            ui.add(TextEdit::singleline(&mut self.phone1)
                                            .hint_text("Phone Number 1").desired_width(self.widget_size));
                                            ui.add(TextEdit::singleline(&mut self.phone2)
                                            .hint_text("Phone Number 2").desired_width(self.widget_size + 3.0));      
                                            
                                            ui.end_row();

                                                                /*     ROW 3     */
                                            ComboBox::from_id_source("salesman_cbox").width(self.widget_size)
                                            .selected_text(format!("{:?}", self.salesman_cbox))
                                            .show_ui(ui, |ui| {
                                                ui.selectable_value(&mut self.salesman_cbox, Salesman::Jake, "Jake");
                                                ui.selectable_value(&mut self.salesman_cbox, Salesman::Danny, "Danny");
                                            });


                                            ComboBox::from_id_source("techs_cbox").width(self.widget_size)
                                            .selected_text(format!("{:?}", self.techs_cbox))
                                            .show_ui(ui, |ui| {
                                                
                                                ui.selectable_value(&mut self.techs_cbox, Techs::Logan, "Logan");
                                                ui.selectable_value(&mut self.techs_cbox, Techs::Bread, "Bread");
                                                ui.selectable_value(&mut self.techs_cbox, Techs::Taco, "Taco");
                                            });    
                                            
                                            ui.end_row();
                                                                /*     ROW 4     */
                                            if ui.add(Button::new("Get Keys").min_size(vec2(self.widget_size, 5.0)))
                                            .clicked(){ 
                                                self.get_cps_button_pressed = true;
                                            }
                                            
                                            if ui.add(Button::new("Check SEB").min_size(vec2(self.widget_size, 5.0)))
                                            .clicked(){ 
                                                self.get_seb_button_pressed = true;
                                                //check_seb_info
                                            }
                
                                            ui.end_row();
                                            
                                                                /*     ROW 5     */
                                            
                                            
                                            let webroot_key = "Webroot Key"; // Color32::from_rgb(102, 255, 153)
                                            if ui.add(Button::new(RichText::new(format!("{}", webroot_key))
                                            .color(Color32::from_rgb(102, 255, 153))
                                            .strong())
                                            .min_size(vec2(self.widget_size, 5.0)))
                                            .on_hover_text("Click To Copy Webroot Key to Clipboard")
                                            .clicked(){ 
                                                self.copy_webroot_button_pressed = true;
                                            }
                                                
                                            let sas_key = "SuperAnti Key";
                                            if ui.add(Button::new(RichText::new(format!("{}", sas_key))
                                            .color(Color32::from_rgb(255, 61, 126)))
                                            .min_size(vec2(self.widget_size, 5.0)))
                                            .on_hover_text("Click To Copy SAS Key to Clipboard")
                                            .clicked(){ 
                                                self.copy_sas_button_pressed = true;
                                            }

                                            ui.end_row();
                                        });
                                    });
                                });
                                
                                strip.cell(|ui|{                
                                    ui.add(TextEdit::multiline(&mut self.checkin_notes)
                                    .hint_text(RichText::new("Checkin Notes").weak())
                                    .desired_rows(14));

                                    ui.vertical(|ui|{
                                        ui.add_space(5.0);
                                    });

                                    ui.vertical_centered(|ui| {
                                        ui.label(format!("{}", self.checkin_rep));
                                    });  
                                    ui.shrink_height_to_current(); 
                                });
                            });
                            
                    });
                    columns[1].centered_and_justified(|ui|{
                        ui.vertical(|ui| {ui.add_space(8.0);});

                        ui.set_min_width(self.widget_size*2.0+4.0);
                        ui.set_max_width(self.widget_size*2.0+6.0);

                        StripBuilder::new(ui)
                        .cell_layout(Layout::top_down_justified(Align::Center))
                        .size(Size::remainder()) //for the initial textedits
                        .size(Size::relative(0.6)) // for the checkin notes
                        .vertical(|mut strip|{
                            strip.cell(|ui|{
                                ui.set_max_width(self.widget_size * 2.0+3.0);
                                ui.group(|ui|{
                                    ui.vertical_centered(|ui|{
                                        ui.horizontal(|ui|{

                                            ui.add_space(self.widget_size/1.8);

                                            ComboBox::from_id_source("ram_cbox").width(self.widget_size - 5.0)
                                            .selected_text(format!("{}", self.ram_test_cbox.as_str()))
                                            .show_ui(ui, |ui| {
                                                ui.selectable_value(&mut self.ram_test_cbox, HardwareTest::RamFail, "RAM Fail");
                                                ui.selectable_value(&mut self.ram_test_cbox, HardwareTest::RamPass, "RAM Pass");
                                                ui.selectable_value(&mut self.ram_test_cbox, HardwareTest::RamNotTested, "RAM Not Tested");
                                            }); // Combo Box
                                        });
                                        
                                    }); // Vertical Centered
        
                                    Grid::new("drive_tests")
                                    .spacing(vec2(4.0, 5.0))
                                    .min_col_width(self.widget_size)
                                    .num_columns(2)
                                    .show(ui, |ui| {
                                                            /*     ROW 1     */
                                        ComboBox::from_id_source("ssd_cbox").width(self.widget_size - 5.0)
                                        .selected_text(format!("{}", self.ssd_test_cbox.as_str()))
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut self.ssd_test_cbox, HardwareTest::SsdFail, "SSD Fail");
                                            ui.selectable_value(&mut self.ssd_test_cbox, HardwareTest::SsdPass, "SSD Pass");
                                            ui.selectable_value(&mut self.ssd_test_cbox, HardwareTest::SsdNotTested, "SSD Not Tested");
                                        }); // Combo Box

                                                            /*     ROW 2     */
                                        ComboBox::from_id_source("hdd_cbox").width(self.widget_size - 5.0)
                                        .selected_text(format!("{}", self.hdd_test_cbox.as_str()))
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut self.hdd_test_cbox, HardwareTest::HddFail, "HDD Fail");
                                            ui.selectable_value(&mut self.hdd_test_cbox, HardwareTest::HddPass, "HDD Pass");
                                            ui.selectable_value(&mut self.hdd_test_cbox, HardwareTest::HddNotTested, "HDD Not Tested");
                                        }); // Combo Box
                                        ui.end_row();
                                    }); // Grid   

                                    
                                    ui.vertical(|ui|{ui.add_space(65.0);});

                                    ui.checkbox(&mut self.send_specs, "Send System Info");

                                        #[cfg(feature = "chrono")]
                                        let date = self.date.get_or_insert_with(|| chrono::offset::Utc::now().date_naive());
                                        //ui.add(egui_extras::DatePickerButton::new(date));

                                    if ui.add(Button::new(RichText::new("Submit TUR Sheet")
                                    .color(Color32::from_rgb(255, 204, 255))
                                    .strong()
                                    .italics())
                                    .stroke(Stroke::new(2.0, Color32::from_rgb(191, 33, 101))))//.min_size(vec2(self.widget_size * 2.0+8.0, 8.0)))
                                    .clicked(){ 
                                        // TODO
                                    }

                                }); // Group
                            }); // Strip cell
                            
                            strip.cell(|ui|{
                                ui.vertical(|ui| {ui.add_space(8.0);});   
                                                           
                                ui.add(TextEdit::multiline(&mut self.recommendations)
                                .hint_text(RichText::new("Recommendations")
                                .weak())
                                .desired_rows(14));
                                ui.shrink_height_to_current(); 

                            }); //Strip Cell
                        }); //Strip Builder
                    }); // Column 1
                }); // Columns
            }); // UI layout
        }); // indent
    }

    fn output_console(&mut self, ui: &mut Ui) { 
        ui.add_sized(ui.available_size(), TextEdit::multiline(&mut self.output_text.to_string()).hint_text("Output"));
        }
    
    fn system_information(&mut self, ui: &mut Ui){
        ui.painter().rect_filled(ui.available_rect_before_wrap(),10.0,self.bg_color);
        ui.painter().rect_stroke(ui.available_rect_before_wrap(),10.0, self.border_stroke_color);
        ui.vertical(|ui| {ui.add_space(3.0);}); // leave some margin above the textEdits

        Grid::new("sysinfo_grid").spacing(vec2(5.0, 5.0)).num_columns(2)
        .show(ui, |ui| { // TODO

            if self.first_run == true{
                self.get_specs = true;
            }
            self.first_run = false;
            
            let mut table = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::initial(100.0).range(40.0..=300.0).clip(true))
            .column(Column::remainder())
            .min_scrolled_height(0.0);

            while let Ok(message) = self.system_info_rx.as_ref().unwrap().try_recv() {
                match message {
                    SendReceiveSystemInfo::RetrieveSystemInfo(system_information) => {
                        
                    },
                    SendReceiveSystemInfo::Error(e) => {

                    }
                }
            }

            //ui.label(format!("{}", serde_json::to_string(&sys).unwrap()));
            /*     ROW 1     
            ui.label("=> Disks:");
            for disk in sys.disks() { // We display all disks' information:
                ui.add_space(15.0);
                ui.label(format!("{:#?}", disk));
                ui.end_row();
            }

            ui.label("=> system:");
            // RAM and swap information:
            ui.label(format!("total memory: {} bytes", sys.total_memory()));
            ui.end_row();

            // Display system information:
            ui.label(format!("System name:             {:?}", sys.name()));
            ui.label(format!("System OS version:       {:?}", sys.os_version()));
            ui.end_row();
            ui.label(format!("System host name:        {:?}", sys.host_name()));

            // Number of CPUs:
            ui.label(format!("NB CPUs: {}", sys.cpus().len()));
            ui.end_row();
            */

        });

    }

    fn scripts(&mut self, ui: &mut Ui){ }
}



impl eframe::App for MasterTechApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        catppuccin_egui::set_theme(ctx, catppuccin_egui::MOCHA);

        if self.context.get_ticket_button_pressed == true {
            self.context.get_ticket_button_pressed = false;
            let service_num = self.context.so_number.clone();
            SendAsyncReq::get_ticket(service_num, self.send_async_req.tx.clone()); 
        }

        if self.context.get_cps_button_pressed == true {
            self.context.get_cps_button_pressed = false;
            let service_num = self.context.so_number.clone();
            SendAsyncReq::get_cps(service_num, self.send_async_req.tx.clone());
        }   

        if self.context.get_specs == true{
            self.context.get_specs == false;
            SendAsyncReq::get_system_specs(self.send_async_req.system_info_tx.clone());
        }
        // On the receiving end:
        while let Ok(message) = self.context.rx.as_ref().unwrap().try_recv() {
            match message {
                SendReceiveMessage::TicketInfo(info) => {
                    // Handle TicketInformation
                    self.context.customer_name = info.customer_name;
                    self.context.phone1 = info.customer_phone_1;
                    self.context.phone2 = info.customer_phone_2;
                    self.context.checkin_notes = info.checkin_notes;
                    self.context.output_text += format!(
                    "Customer Code: {:?}
Customer Email: {:?}
Last Invoice Number: {:?}
Last Invoice Amount: {:?}
Department: {:?}
Jurisdiction: {:?}
Type of Order: {:?}
Item Codes: {:?}\n",
                    &info.cust_code, &info.customer_email, &info.last_invoice_number, &info.last_invoice_amount,
                    &info.department, &info.jurisdiction, &info.doc_alias, &info.item_codes).as_str();

                }
                SendReceiveMessage::Cpskeys(info) => {
                    self.context.webroot_key = info.webroot_key;
                    self.context.superanti_key = info.superanti_key;
                }
                SendReceiveMessage::Error(err) => {
                    // Handle error
                    self.context.output_text = err.to_string();
                }
            }
        }
        
    
        TopBottomPanel::top("egui_dock::MenuBar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("View", |ui| {
                    // allow certain tabs to be toggled
                    for tab in &[&self.context.tur_sheet_tab, &self.context.scripts_tab, &self.context.output_console_tab, &self.context.system_info_tab] {
                        if ui
                            .selectable_label(self.context.open_tabs.contains(*tab), *tab)
                            .clicked()
                        {
                            if let Some(index) = self.tree.find_tab(&tab.to_string()) {
                                self.tree.remove_tab(index);
                                self.context.open_tabs.remove(*tab);
                            } else {
                                self.tree.push_to_focused_leaf(tab.to_string());
                            }
                            ui.close_menu();
                        }
                    }
                });
            })
        });
        CentralPanel::default()// When displaying a DockArea in another UI, it looks better
            .frame(Frame::central_panel(&ctx.style()).inner_margin(4.))// to set inner margins to 0.
            .show(ctx, |ui| {
                let mut style = self.context.style.get_or_insert(Style::from_egui(ui.style())).clone();
                style.selection_color = Color32::from_rgb(92,0,87);
                style.separator.color_hovered = Color32::from_rgba_premultiplied(50,93,80,77);
                style.separator.color_idle = Color32::from_rgba_premultiplied(17,17,33,5);
                style.separator.color_dragged = Color32::from_rgba_premultiplied(189,189,189,130);
                style.buttons.add_tab_align = egui_dock::TabAddAlign::Left;
                style.tabs.rounding.nw = 15.0;
                style.tabs.rounding.ne = 15.0;
                style.tabs.text_color_active_focused = Color32::from_rgba_premultiplied(0, 254, 158, 255);
                style.tabs.text_color_active_unfocused = Color32::from_rgba_premultiplied(0, 255, 255, 255);
                style.tabs.text_color_unfocused = Color32::from_rgba_premultiplied(230, 230, 230, 100);
                style.buttons.close_tab_color = Color32::from_rgba_premultiplied(118, 0, 129, 58);

                DockArea::new(&mut self.tree)
                    .style(style)
                    .show_close_buttons(self.context.show_close_buttons)
                    .show_add_buttons(self.context.show_add_buttons)
                    .show_add_popup(true)
                    .draggable_tabs(self.context.draggable_tabs)
                    .show_tab_name_on_hover(self.context.show_tab_name_on_hover)
                    .show_inside(ui, &mut self.context);
            });
    }
}