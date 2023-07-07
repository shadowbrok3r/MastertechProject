#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] use file_browser::FileBrowser;
// hide output_console window on Windows in release
use serde_json::{json, Value, value};
use sysinfo::*;
use std::{collections::HashSet, borrow::BorrowMut}; //, os::windows::thread};
use eframe::{egui, glow::PROGRAM_BINARY_LENGTH};
use egui::*;
use egui_dock::{DockArea, Node, NodeIndex, Style, TabViewer, Tree};
use scaffold_builder::PulledKeys;
use tokio::runtime::Handle;
use serde::{Deserialize, Serialize};
use egui_extras::*;
use catppuccin_egui::MOCHA;

mod request;
mod file_browser;
mod scaffold_builder;

#[tokio::main]
async fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        initial_window_size: Some(egui::vec2(925.0, 710.0)),
        ..Default::default()
    };
    eframe::run_native(
        "Mastertech",
        options,
        Box::new(|_cc| Box::<MasterTechApp>::default()),
    )
}

pub struct SendAsyncReq {
    tx: std::sync::mpsc::Sender<String>,
}

#[derive(Serialize, Deserialize)]
pub struct SystemInformation{
    cpu_name: String,
    total_ram: String,
    system_name: String,
    disks: DiskData, //Option<String>
}

#[derive(Serialize, Deserialize)]
struct DiskData {
    disks: Vec<Value>,
}

impl DiskData {
    fn new() -> Self {
        DiskData {
            disks: Vec::new(),
        }
    }

    fn add_disk(&mut self, disk: Value){
        self.disks.push(disk);
    }
}

impl SendAsyncReq{
    fn get_ticket(so_number: String, tx: std::sync::mpsc::Sender<String>){
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

                        let ticket_info_json = serde_json::to_string(&ticket_information).unwrap();

                        match tx.send(ticket_info_json) {
                            Ok(_) => {
                                drop(tx)
                            },
                            Err(e) => {
                                eprintln!("Error while sending ticket information: {}", e.to_string());
                                drop(tx)
                            }
                        }
                        
                    },
                    Err(e) => { 
                        match tx.send(e.to_string()) {
                            Ok(_) => {
                                drop(tx)
                            },
                            Err(e) => {
                                eprintln!("Error while sending error message: {}", e);
                                drop(tx)
                            }
                        }
                    }
                    
                }
            });
        }); 
    }
    
    fn get_cps(so_number: String, tx: std::sync::mpsc::Sender<String>){
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

                        let cps_keys_json = serde_json::to_string(&cps_keys).unwrap();
                        
                        match tx.send(cps_keys_json) {
                            Ok(_) => {
                                drop(tx)
                            },
                            Err(e) => {
                                eprintln!("Error while sending ticket information: {}", e.to_string());
                                drop(tx)
                            }
                        }
                        
                    },
                    Err(e) => { 
                        match tx.send(e.to_string()) {
                            Ok(_) => {
                                drop(tx)
                            },
                            Err(e) => {
                                eprintln!("Error while sending error message: {}", e);
                                drop(tx)
                            }
                        }
                    }                    
                }
            });
        });
    }

    fn get_system_specs(tx: std::sync::mpsc::Sender<String>){
        let handle = Handle::current();
        
        std::thread::spawn(move||{
            handle.block_on(async{
                let mut sys = System::new_all(); // Create `System` struct.

                let cpu_brand = sys.cpus()[0].brand().to_string();
                let ram = (sys.total_memory() / ( 1024 * 1024 * 1024)).to_string();
                let system = sys.long_os_version().unwrap_or_else(|| "<unknown>".to_owned());
                let disks = sys.disks();
                let disks_clone = disks.clone();


                let mut data = DiskData::new();

                for disk in disks_clone{
                    if !disk.is_removable(){
                        data.add_disk(serde_json::json!({
                            "name": disk.name(),
                            "letter": disk.mount_point().to_str(),
                            "total space": (disk.total_space() / ( 1024 * 1024 * 1024)).to_string(),
                            "available space": (disk.available_space() / ( 1024 * 1024 * 1024)).to_string(),
                        }));
                    }   
                }
                
                // String for each disk: [name] [letter]:\\ [ Available space / Total space ]
                let system_info = SystemInformation{
                    cpu_name: cpu_brand,
                    total_ram: ram,
                    system_name: system,
                    disks: data
                };

                let system_info_json = serde_json::to_string(&system_info).unwrap();

                match tx.send(system_info_json) {
                    Ok(_) => {
                        drop(tx);
                    },
                    Err(e) => {
                        eprintln!("Error while sending ticket information: {}", e.to_string());
                        drop(tx);
                    }
                }
                


            });
        });
    }
    
    #[cfg(target_os = "windows")]
    fn get_gpu(){
        let gpu = std::process::Command::new("cmd").args(["/C", "wmic path win32_VideoController get name"]).output();
        match gpu{
            Ok(_) => {

            }
            Err(e) => {
                println!("Error: {}", e);
            }
        }
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
    salesman_cbox: scaffold_builder::Salesman,
    techs_cbox: scaffold_builder::Techs,
    ram_test_cbox: scaffold_builder::HardwareTest,
    hdd_test_cbox: scaffold_builder::HardwareTest,
    ssd_test_cbox: scaffold_builder::HardwareTest,
    checkin_notes: String,
    webroot_key: String,
    superanti_key: String,
    recommendations: String,
    checkin_rep: String,
    output_text: String,
    last_invoice_num: String,
    last_invoice_amnt: String,
    jurisdiction: String,
    cpu_name: String,
    total_ram: String,
    system_name: String,
    disks: Value,
    disk_num: usize,
    rx: Option<std::sync::mpsc::Receiver<String>>,

    //////////////////////////////////////////
    /*          Widgets and UI elements     */
    //////////////////////////////////////////
    ctx: egui::Context,
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
    reader_bytes: u32,
    get_ticket_button_pressed: bool,
    get_cps_button_pressed: bool,
    get_seb_button_pressed: bool,
    first_run: bool,
    get_specs: bool,
    spinner: bool,

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
            "File Browser" => self.file_browse(ui),
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
        let (tx, rx) = std::sync::mpsc::channel::<String>();

        let mut tree = Tree::new(vec!["TUR Sheet".to_owned(), "System Information".to_owned()]);
        let [a, b] = tree.split_left(NodeIndex::root(), 0.3, vec!["File Browser".to_owned(), "Empty".to_owned()]);
        let [_, _] = tree.split_below(
            a,
            0.7,
            vec!["Console".to_owned()],
        );

        let [_, _] = tree.split_below(b, 0.5, vec!["Scripts".to_owned()]);

        let mut open_tabs = HashSet::new();

        for node in tree.iter() {
            if let Node::Leaf { tabs, .. } = node {
                for tab in tabs {
                    open_tabs.insert(tab.clone());
                }
            }
        }
        
        let send_async_req = SendAsyncReq{
            tx: tx,
        };

        let context = MastertechContext {
            //////////////////////////////////////////
            /*          Mastertech Vars             */
            //////////////////////////////////////////
            so_number: "".to_string(),
            customer_name: "".to_string(),
            phone1: "".to_string(),
            phone2: "".to_string(),
            salesman_cbox: scaffold_builder::Salesman::Jake,
            techs_cbox: scaffold_builder::Techs::Logan,
            ram_test_cbox: scaffold_builder::HardwareTest::RamNotTested,
            hdd_test_cbox: scaffold_builder::HardwareTest::HddNotTested,
            ssd_test_cbox: scaffold_builder::HardwareTest::SsdNotTested,
            checkin_notes: "".to_string(),
            webroot_key: "Webroot Key".to_string(),
            superanti_key: "SuperAnti Key".to_string(),
            recommendations: "".to_string(),
            send_specs: false,
            checkin_rep: "Checkin Rep: ".to_string(),
            output_text: "".to_string(),
            last_invoice_num: "".to_string(),
            last_invoice_amnt: "".to_string(),
            jurisdiction: "".to_string(),
            cpu_name: "".to_string(),
            total_ram: "".to_string(),
            system_name: "".to_string(),
            disks: Value::Array(vec![]),
            disk_num: 0,
            rx: Some(rx),

            //////////////////////////////////////////
            /*          Widgets and UI elements     */
            //////////////////////////////////////////
            ctx: egui::Context::default(),
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
            reader_bytes: 0,
            get_ticket_button_pressed: false,
            get_cps_button_pressed: false,
            get_seb_button_pressed: false,
            first_run: true,
            get_specs: false,
            spinner: false,

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
        ui.style_mut().spacing.button_padding = (4.0, 5.0).into();
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
                                                ui.selectable_value(&mut self.salesman_cbox, scaffold_builder::Salesman::Jake, "Jake");
                                                ui.selectable_value(&mut self.salesman_cbox, scaffold_builder::Salesman::Danny, "Danny");
                                            });


                                            ComboBox::from_id_source("techs_cbox").width(self.widget_size)
                                            .selected_text(format!("{:?}", self.techs_cbox))
                                            .show_ui(ui, |ui| {
                                                
                                                ui.selectable_value(&mut self.techs_cbox, scaffold_builder::Techs::Logan, "Logan");
                                                ui.selectable_value(&mut self.techs_cbox, scaffold_builder::Techs::Bread, "Bread");
                                                ui.selectable_value(&mut self.techs_cbox, scaffold_builder::Techs::Taco, "Taco");
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
                                            if ui.add(Button::new(RichText::new(format!("{}", self.webroot_key)).size(9.0)
                                            .color(Color32::from_rgb(102, 255, 153))
                                            .strong())
                                            .min_size(vec2(self.widget_size + 2.0, 8.0)))
                                            .on_hover_text("Click To Copy Webroot Key to Clipboard")
                                            .clicked(){ 
                                                let webroot = self.webroot_key.clone();
                                                ui.output_mut(|o| o.copied_text = webroot);
                                            }
                                                
                                            if ui.add(Button::new(RichText::new(format!("{}", self.superanti_key)).size(9.0)
                                            .color(Color32::from_rgb(255, 61, 126))
                                            .strong())
                                            .min_size(vec2(self.widget_size + 2.0, 8.0)))
                                            .on_hover_text("Click To Copy SAS Key to Clipboard")
                                            .clicked(){ 
                                                let sas = self.superanti_key.clone();
                                                ui.output_mut(|o| o.copied_text = sas);

                                            }

                                            ui.end_row();
                                        });
                                    });
                                });
                                
                                strip.cell(|ui|{              
                                    ScrollArea::new([false, true])
                                    .max_height(235.0)
                                    .id_source("checkin_notes_scroll")
                                    .show(ui, |ui|{
                                        ui.add(TextEdit::multiline(&mut self.checkin_notes)
                                        .hint_text(RichText::new("Checkin Notes").weak())
                                        .desired_rows(16));
                                    }) ;
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
                                                ui.selectable_value(&mut self.ram_test_cbox, scaffold_builder::HardwareTest::RamFail, "RAM Fail");
                                                ui.selectable_value(&mut self.ram_test_cbox, scaffold_builder::HardwareTest::RamPass, "RAM Pass");
                                                ui.selectable_value(&mut self.ram_test_cbox, scaffold_builder::HardwareTest::RamNotTested, "RAM Not Tested");
                                            }); // Combo Box
                                        });
                                        
                                    }); // Vertical Centered
        
                                    Grid::new("drive_tests")
                                    .spacing(vec2(4.0, 3.0))
                                    .min_col_width(self.widget_size)
                                    .num_columns(2)
                                    .show(ui, |ui| {
                                                            /*     ROW 1     */
                                        ComboBox::from_id_source("ssd_cbox").width(self.widget_size - 5.0)
                                        .selected_text(format!("{}", self.ssd_test_cbox.as_str()))
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut self.ssd_test_cbox, scaffold_builder::HardwareTest::SsdFail, "SSD Fail");
                                            ui.selectable_value(&mut self.ssd_test_cbox, scaffold_builder::HardwareTest::SsdPass, "SSD Pass");
                                            ui.selectable_value(&mut self.ssd_test_cbox, scaffold_builder::HardwareTest::SsdNotTested, "SSD Not Tested");
                                        }); // Combo Box

                                                            /*     ROW 2     */
                                        ComboBox::from_id_source("hdd_cbox").width(self.widget_size - 5.0)
                                        .selected_text(format!("{}", self.hdd_test_cbox.as_str()))
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut self.hdd_test_cbox, scaffold_builder::HardwareTest::HddFail, "HDD Fail");
                                            ui.selectable_value(&mut self.hdd_test_cbox, scaffold_builder::HardwareTest::HddPass, "HDD Pass");
                                            ui.selectable_value(&mut self.hdd_test_cbox, scaffold_builder::HardwareTest::HddNotTested, "HDD Not Tested");
                                        }); // Combo Box
                                        ui.end_row();
                                    }); // Grid   

                                    
                                    ui.vertical(|ui|{ui.add_space(18.0);});

                                    
                                    let progress_bar = egui::ProgressBar::new(self.reader_bytes as f32)
                                        .show_percentage()
                                        .animate(true);

                                    if self.spinner == true{
                                        ui.add(Spinner::new());
                                    }
                                    

                                    ui.vertical(|ui|{ui.add_space(18.0);});

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
                                ScrollArea::new([false, true])
                                .id_source("reccomendations_scroll")
                                .max_height(235.0)
                                .show(ui, |ui|{
                                    ui.add(TextEdit::multiline(&mut self.recommendations)
                                    .hint_text(RichText::new("Recommendations")
                                    .weak())
                                    .desired_rows(16));
                                });
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

        if self.first_run == true{
            self.get_specs = true;
        }
        self.first_run = false;
        
        if self.spinner == true{
            ui.vertical_centered(|ui|{
                ui.add(Spinner::new());
            });
            
        }

        ui.indent("indented_sysinfo_table", |ui|{
            let table = TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::initial(100.0).range(50.0..=300.0).clip(true))
                .column(Column::remainder())
                .min_scrolled_height(0.0);
            table
            .header(20.0, |mut header|{
                header.col(|ui| {
                    ui.strong("Hardware Name");
                });
                header.col(|ui| {
                    ui.strong("Info");
                });
            })
            .body(|mut body| {
                body.row(20.0, |mut row| {
                    row.col(|ui|{
                        ui.label("System Name");
                    });
                    row.col(|ui|{
                        ui.label(&self.system_name);
                    });
                });
                body.row(20.0, |mut row| {
                    row.col(|ui|{
                        ui.label("CPU Name");
                    });
                    row.col(|ui|{
                        ui.label(&self.cpu_name);
                    });
                });
                body.row(20.0, |mut row| {
                    row.col(|ui|{
                        ui.label("Total RAM");
                    });
                    row.col(|ui|{
                        ui.label(format!("{} Gb", &self.total_ram));
                    });
                });
                #[cfg(target_os = "windows")]
                body.row(20.0, |mut row| {
                    row.col(|ui|{
                        ui.label("GPU");
                    });
                    row.col(|ui|{
                        let gpu = SendAsyncReq::get_gpu();
                        ui.label(format!("{}", gpu));
                    });
                });
                
            });

        });
        ui.vertical(|ui|{ui.add_space(20.0)});
        ui.indent("indented_disks",|ui|{
            let disks_table = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(Layout::left_to_right(Align::Center))
            .column(Column::exact(15.0))
            .column(Column::exact(150.0))
            .columns(Column::remainder(), 2);
            
            disks_table
            .header(20.0, |mut header|{
                header.col(|ui|{
                    ui.label("#");
                });
                header.col(|ui|{
                    ui.label("Drive Letter");
                });
                // header.col(|ui|{
                //     ui.label("Space Used");
                // });
                header.col(|ui|{
                    ui.label("Avail / Total Space");
                });

            })
            .body(|mut body| {
                body.rows(
                20.0,  // Replace with your desired row height
                self.disk_num,
                |disk_index, mut row| 
                {
                    if let Some(disk) = self.disks.get(disk_index){
                        //println!("disks: {:#?}", disk);

                        //let disk_name = format!("{:#?}", disk.get("name"));
                        let disk_letter = format!("{}", disk.get("letter").and_then(Value::as_str).unwrap_or(""));
                        let disk_space = format!(
                            "{} Gb / {} Gb",
                            disk.get("available space").and_then(Value::as_str).unwrap_or(""),
                            disk.get("total space").and_then(Value::as_str).unwrap_or("")
                        );
                        let disk_used = format!("{}", (disk.get("total space").and_then(Value::as_u64).unwrap_or(0)) - 
                        (disk.get("available space").and_then(Value::as_u64).unwrap_or(0)));

                        let disk_space = format!(
                            "{} Gb / {} Gb",
                            disk.get("available space").and_then(Value::as_str).unwrap_or(""),
                            disk.get("total space").and_then(Value::as_str).unwrap_or("")
                        );
                        
                    
                        row.col(|ui| {
                            ui.label(disk_index.to_string());  // Show disk index
                        });
                        row.col(|ui| {
                            ui.label(disk_letter);  // Show disk letter
                        });
                        // row.col(|ui| {
                        //     ui.label(disk_used.to_string());  // Show disk space
                        // });
                        row.col(|ui| {
                            ui.label(disk_space);  // Show disk space
                        });
                        self.ctx.request_repaint();
                        self.spinner = false;
                    }   

                });
            });
        });
    }

    fn file_browse(&mut self, ui: &mut Ui){ 
        //file_browser::file_browser();
        let ctx = self.ctx.clone();
        file_browser::FileBrowser::file_browsing_test(&ctx);
        //file_browser::FileBrowser::file_dialog(&mut x, &ctx);

    }

    fn scripts(&mut self, ui: &mut Ui){ }
}

impl eframe::App for MasterTechApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        catppuccin_egui::set_theme(ctx, catppuccin_egui::MOCHA);
        
        self.context.ctx = ctx.clone();
        let ticket_sender = self.send_async_req.tx.clone();
        let cps_sender = self.send_async_req.tx.clone();
        let specs_sender = self.send_async_req.tx.clone();

        if self.context.get_ticket_button_pressed == true {
            self.context.get_ticket_button_pressed = false;
            let service_num = self.context.so_number.clone();
            self.context.spinner = true;
            SendAsyncReq::get_ticket(service_num, ticket_sender); 
        }

        if self.context.get_cps_button_pressed == true {
            self.context.get_cps_button_pressed = false;
            let service_num = self.context.so_number.clone();
            self.context.spinner = true;
            SendAsyncReq::get_cps(service_num, cps_sender);
        }   

        if self.context.get_specs == true{
            self.context.get_specs = false;
            self.context.spinner = true;
            SendAsyncReq::get_system_specs(specs_sender);
        }

        let receiver = self.context.rx.as_ref().unwrap();

        // On the receiving end:
        while let Ok(message) = receiver.try_recv() {
            // Try to parse the JSON string into a TicketInformation
            if let Ok(info) = serde_json::from_str::<scaffold_builder::TicketInformation>(&message) {
                self.context.output_text.clear();
                let checkin_rep = info.user_id;
                self.context.checkin_rep = checkin_rep.clone();
                if checkin_rep == "DMK"{self.context.salesman_cbox = scaffold_builder::Salesman::Danny;}
                else if checkin_rep == "JDH2"{self.context.salesman_cbox = scaffold_builder::Salesman::Jake}

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
                self.context.spinner = false;

            }             
            else if let Ok(info) = serde_json::from_str::<scaffold_builder::PulledKeys>(&message) {
                // Handle PulledKeys
                self.context.webroot_key = info.webroot_key;
                self.context.superanti_key = info.superanti_key;
                self.context.spinner = false;
            }
            // If neither parse was successful, consider it an error
            else if let Ok(info) = serde_json::from_str::<SystemInformation>(&message) {
                self.context.system_name = info.system_name;
                self.context.cpu_name = info.cpu_name;
                self.context.total_ram = info.total_ram;
                for disk in info.disks.disks{
                    
                    self.context.disk_num += 1;

                    if let Some(disks_arr) = self.context.disks.as_array_mut() {
                        // Convert `disk` to a serde_json::Value
                        let disk_json = serde_json::to_value(&disk).unwrap();
                
                        disks_arr.push(disk_json);
                    } else {
                        eprintln!("Expected self.context.disks to be an Array");
                    }
                    
                }
                
            }
            else{
                // Handle error
                self.context.output_text = format!("Error parsing JSON: {}", message);
                self.context.spinner = false;
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