#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide output_console window on Windows in release
//use sysinfo::{System, SystemExt, RefreshKind};
use std::{collections::HashSet, borrow::BorrowMut}; //, os::windows::thread};
use eframe::egui;
use egui::*;
use egui_dock::{DockArea, Node, NodeIndex, Style, TabViewer, Tree};
use tokio::{sync::watch, task, runtime::Handle};
use egui_extras::*;
use std::sync::{Arc, Mutex};

mod request;
mod data_transfer;

#[tokio::main]
async fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        initial_window_size: Some(egui::vec2(900.0, 700.0)),
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
    ram_pass,
    ram_fail,
    ram_not_tested,
    hdd_pass,
    hdd_fail,
    hdd_not_tested,
    ssd_pass,
    ssd_fail,
    ssd_not_tested,
}
pub struct SendRequest {
}

impl Default for SendRequest { 
    fn default() -> Self {
        Self { }
    }
}

struct TicketInformation{
    cust_code: String,
    user_id: String,
    terms: String, // "TERMS": "CC",
    doc_alias: String, // "DOC_ALIAS": "SERVICE ORDER",
    department: String, // "DEP": "LTN"
    jurisdiction: String, //"JURISCODE": "LTN",
    invoice_amnt: String,


    customer_name: String, // "NAME": "Timber Ridge Fireplace LLC",
    //customer_address: String,
    last_invoice_number: String, //"LI_DOC": "53745333",
    //last_invoice_date: String,  //"LI_AMT": "53.6100", //I COULD USE THIS TO CHECK LAST TUNEUP
    //last_tuneup_date: String, // <-- HERE
    last_checkin_date: String, // "DW_UPDATE_DATE": "2023-06-27 13:38:50.440",
    total_invoice_count: String,


    //pub item_objects: Vec<ItemObjects>,
}


impl SendRequest{
   fn get_ticket(so_number: String, mastertech_app: &mut MasterTechApp){
        let handle = Handle::current();
        let service_num = so_number.clone();
       
        let mut context = &mut mastertech_app.context;
        let mut locked_text = context.lock_text.clone();
        

        let output_text_clone = Arc::clone(&context.output_text);
 
        std::thread::spawn(move||{
            handle.block_on(async{
                let response = request::request_ticket_info(service_num).await;
                


                match response { // Successfully received GetTicketResponse
                    Ok(get_ticket_response) => {
                        // You can now use fields of get_ticket_response
                        //let main_json = &get_ticket_response.main_json;
                        let header = &get_ticket_response.header;
                        let customer = &get_ticket_response.customer;

                        //let transactions = &get_ticket_response.transactions;
                        //let addresses = &get_ticket_response.addresses;
                        

                        //let items = get_ticket_response.items;
                        // for addr_arr in &main_json.addresses{
                        //     println!("info: {:?}", addr_arr);
                        // }
        
                        let ticket_information = TicketInformation{
                            cust_code: header.CUST_CODE.clone(),
                            user_id: header.USER_ID.clone(),
                            terms: header.TERMS.clone(),
                            doc_alias: header.DOC_ALIAS.clone(),
                            department: header.DEP.clone(),
                            jurisdiction: header.JURISCODE.clone(),
                            invoice_amnt: header.INV_AMOUNT.clone(),
                            customer_name: customer.NAME.clone(),
                            //customer_address: customer.CUSTOMER_ADDRESS.clone(),
                            last_invoice_number: customer.LI_DOC.clone(),
                            //last_invoice_date: customer.LAST_INVOICE_DATE.clone(),
                            //last_tuneup_date: customer.LAST_TUNEUP_DATE.clone(),
                            last_checkin_date: customer.LI_AMT.clone(),
                            total_invoice_count: customer.NUM_INV.clone(),
                        };

                        let mut output_text = output_text_clone.lock().unwrap();
                        *output_text = format!("Output: {}", ticket_information.cust_code.as_str());
                        
                        drop(output_text);
                        locked_text = false;
                    },

                    Err(e) => {
                        // There was an error while making the request
                        //response_output = &e.
                        let mut output_text = output_text_clone.lock().unwrap();
                        *output_text = format!("Error: {}",e);
                        drop(output_text);
                        locked_text = false;
                    },
                }
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
    output_text: Arc<Mutex<String>>,

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
    lock_text: bool,

    //////////////////////////////////////////
    /*          UI Colors                   */
    //////////////////////////////////////////
    style: Option<egui_dock::Style>,
    text_color: Color32,
    border_stroke_color: Stroke,
    bg_color: Color32,
}

struct MasterTechApp {
    context: MastertechContext,
    //requestor: SendRequest,
    tree: Tree<String>,

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
                ui.label(tab.as_str());
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
}

impl Default for MasterTechApp {
    fn default() -> Self {
        
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
            ram_test_cbox: HardwareTest::ram_not_tested,
            hdd_test_cbox: HardwareTest::ram_not_tested,
            ssd_test_cbox: HardwareTest::ssd_not_tested,
            checkin_notes: "".to_string(),
            webroot_key: "".to_string(),
            superanti_key: "".to_string(),
            recommendations: "".to_string(),
            send_specs: false,
            output_text: Arc::new(Mutex::new(String::new())),

            //////////////////////////////////////////
            /*          Widgets and UI elements     */
            //////////////////////////////////////////
            widget_size: 130.0,
            //default_margins: Margin::same(10.0),
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
            lock_text: false,

            //////////////////////////////////////////
            /*          UI Colors                   */
            //////////////////////////////////////////
            style: None,
            text_color: Color32::from_rgb(128, 242, 192),//(200,200,200),
            bg_color: Color32::from_rgb(28,30,36),
            border_stroke_color: Stroke::new(1.0, Color32::from_rgb_additive(150, 62, 124))
        };

        Self { context, tree }
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
        ui.style_mut().spacing.button_padding = (5.0, 3.0).into();
        ui.style_mut().spacing.window_margin.left = 15.0;
        ui.style_mut().spacing.window_margin.right = 15.0;
        // ui.style_mut().visuals.button_frame
        ui.painter().rect_filled(ui.available_rect_before_wrap(),10.0,self.bg_color);
        ui.painter().rect_stroke(ui.available_rect_before_wrap(),10.0, self.border_stroke_color);
        
        ui.vertical(|ui| {ui.add_space(3.0);}); // leave some margin above the textEdits

        ui.columns(2,|column|{
        column[0].vertical(|ui|{
            ui.horizontal(|ui|{
                ui.add_space(80.0);
                if ui.add(Button::new("Get Ticket").stroke(self.border_stroke_color)
                .fill(Color32::from_rgb(50, 57, 71)).min_size(vec2(self.widget_size, 5.0)).sense(Sense { click: true, drag: false, focusable: true })).clicked(){ 
                    self.get_ticket_button_pressed = true; // Sets bool to true so the main loop runs the get_ticket function
                }
            });

            Grid::new("tur_sheet_grid1_col1").spacing(vec2(5.0, 5.0)).num_columns(2)
            .show(ui, |ui| {

                /*     ROW 1     */
                ui.add_space(15.0);
                ui.add(TextEdit::singleline(&mut self.so_number)
                .hint_text("SO#").char_limit(8).desired_width(self.widget_size));
                
                ui.add(TextEdit::singleline(&mut self.customer_name)
                .hint_text("Customer Name").desired_width(self.widget_size));
                ui.end_row();
                
                /*     ROW 2     */
                ui.add_space(15.0);
                ui.add(TextEdit::singleline(&mut self.phone1)
                .hint_text("Phone Number 1").desired_width(self.widget_size));
                ui.add(TextEdit::singleline(&mut self.phone2)
                .hint_text("Phone Number 2").desired_width(self.widget_size));      
                ui.end_row();
            
                /*     ROW 3     */
                ui.add_space(15.0);
                ComboBox::from_id_source("salesman_cbox").width(self.widget_size - 2.0)
                .selected_text(format!("{:?}", self.salesman_cbox))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.salesman_cbox, Salesman::Jake, "Jake");
                    ui.selectable_value(&mut self.salesman_cbox, Salesman::Danny, "Danny");
                });
    
                ComboBox::from_id_source("techs_cbox").width(self.widget_size - 2.0)
                .selected_text(format!("{:?}", self.techs_cbox))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.techs_cbox, Techs::Logan, "Logan");
                    ui.selectable_value(&mut self.techs_cbox, Techs::Bread, "Bread");
                    ui.selectable_value(&mut self.techs_cbox, Techs::Taco, "Taco");
                });          
            });

            ui.vertical(|ui| {ui.add_space(3.0);});
            Grid::new("tur_sheet_grid2_col1").spacing(vec2(5.0, 5.0)).num_columns(1)
            .show(ui, |ui| {
                ui.add_space(16.0);
                ui.add(TextEdit::multiline(&mut self.checkin_notes)
                .hint_text("Checkin Notes").desired_rows(15).desired_width(self.widget_size * 2.0 + 3.0));
            });
            ui.vertical(|ui| {ui.add_space(3.0);});
            Grid::new("tur_sheet_grid3_col1").spacing(vec2(5.0, 5.0)).num_columns(2)
            .show(ui, |ui| {
                ui.add_space(15.0);
                if ui.add(Button::new("Get Keys").stroke(self.border_stroke_color)
                .fill(Color32::from_rgb(25, 12, 48)).min_size(vec2(self.widget_size, 5.0)).sense(Sense { click: true, drag: false, focusable: true })).clicked(){ 
                    //get_cps_keys
                }
                
                if ui.add(Button::new("Check SEB").stroke(self.border_stroke_color)
                .fill(Color32::from_rgb(25, 12, 48)).min_size(vec2(self.widget_size, 5.0)).sense(Sense { click: true, drag: false, focusable: true })).clicked(){ 
                    //check_seb_info
                }
                ui.end_row();
            });
            ui.vertical(|ui| {ui.add_space(3.0);});

            Grid::new("tur_sheet_grid4_col1").spacing(vec2(5.0, 5.0)).num_columns(2)
            .show(ui, |ui| {

                /*     ROW 1     */
                ui.add_space(15.0);
                ui.visuals_mut().override_text_color = Some(Color32::from_rgb(0, 224, 90));
                if ui.add(Button::new("Webroot").stroke(Stroke::new(1.5, Color32::from_rgb(0, 224, 90)))
                .fill(Color32::from_rgb(27, 27, 28)).min_size(vec2(self.widget_size, 5.0)).sense(Sense { click: true, drag: false, focusable: true })).clicked(){ 
                    //SABB-TAOG-ECC9-9C8C-CFD2
                    //copy_webroot_key
                }
                ui.add(TextEdit::singleline(&mut self.webroot_key).desired_width(self.widget_size)
                .hint_text("<-- Copy Key").char_limit(24));
                ui.end_row();

                /*     ROW 2     */
                ui.add_space(15.0);
                ui.visuals_mut().override_text_color = Some(Color32::from_rgb(240, 98, 98));
                if ui.add(Button::new("SuperAntiSpyware").stroke(Stroke::new(1.5, Color32::from_rgb(240, 98, 98)))
                .fill(Color32::from_rgb(27, 27, 28)).min_size(vec2(self.widget_size, 5.0)).sense(Sense { click: true, drag: false, focusable: true })).clicked(){ 
                    //1C2J-JTPD-CFG3R
                    //copy_superanti_key
                }
                ui.add(TextEdit::singleline(&mut self.superanti_key).desired_width(self.widget_size)
                .hint_text("<-- Copy Key").char_limit(13));
                ui.end_row();
            });
        });


        column[1].vertical(|ui|{
            Grid::new("tur_sheet_grid1_col2").spacing(vec2(5.0, 5.0)).num_columns(2)
            .show(ui, |ui|{
                /*     ROW 3     */
                ui.add_space(8.0);
                ComboBox::from_id_source("ssd_cbox").width(self.widget_size - 2.0)
                .selected_text(format!("{:?}", self.ssd_test_cbox))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.ssd_test_cbox, HardwareTest::ssd_fail, "SSD Fail");
                    ui.selectable_value(&mut self.ssd_test_cbox, HardwareTest::ssd_pass, "SSD Pass");
                    ui.selectable_value(&mut self.ssd_test_cbox, HardwareTest::ssd_not_tested, "SSD Not Tested");
                });
    
                ComboBox::from_id_source("hdd_cbox").width(self.widget_size - 2.0)
                .selected_text(format!("{:?}", self.hdd_test_cbox))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.hdd_test_cbox, HardwareTest::hdd_fail, "HDD Fail");
                    ui.selectable_value(&mut self.hdd_test_cbox, HardwareTest::hdd_pass, "HDD Pass");
                    ui.selectable_value(&mut self.hdd_test_cbox, HardwareTest::hdd_not_tested, "HDD Not Tested");
                });
                ui.end_row();

            });
            Grid::new("tur_sheet_grid2_col2").spacing(vec2(5.0, 5.0)).num_columns(1)
            .show(ui, |ui|{
                ui.add_space(80.0);
                ComboBox::from_id_source("ram_cbox").width(self.widget_size - 2.0)
                .selected_text(format!("{:?}", self.ram_test_cbox))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.ram_test_cbox, HardwareTest::ram_fail, "RAM Fail");
                    ui.selectable_value(&mut self.ram_test_cbox, HardwareTest::ram_pass, "RAM Pass");
                    ui.selectable_value(&mut self.ram_test_cbox, HardwareTest::ram_not_tested, "RAM Not Tested");
                });
            });

            /*     ROW 1     */
            ui.add_space(15.0);
            ui.add(TextEdit::multiline(&mut self.recommendations)
            .hint_text("Recommendations").desired_rows(15).desired_width(self.widget_size * 2.0+8.0));

            Grid::new("tur_sheet_grid3_col2").spacing(vec2(5.0, 5.0)).num_columns(2)
            .show(ui, |ui| {
                ui.checkbox(&mut self.send_specs, "Send System Info");
                ui.end_row();
            });
            //#[cfg(feature = "chrono")]
            //let date = self.date.get_or_insert_with(|| chrono::offset::Utc::now().date_naive());
            //ui.add(egui_extras::DatePickerButton::new(date));
            ui.end_row();
            
            ui.add_space(15.0);
            ui.visuals_mut().override_text_color = Some(Color32::from_rgb(170, 33, 191));
            if ui.add(Button::new("Submit TUR Sheet").stroke(Stroke::new(2.0, Color32::from_rgb(191, 33, 101)))
            .fill(Color32::from_rgb(38, 38, 38)).min_size(vec2(self.widget_size * 2.0+8.0, 8.0)).sense(Sense { click: true, drag: false, focusable: true })).clicked(){ 
                // TODO
            }

        });
        });
    }

    fn output_console(&mut self, ui: &mut Ui) { 
        ui.painter().rect_stroke(ui.available_rect_before_wrap(),10.0, Stroke::new(1.0, Color32::LIGHT_GREEN));
        match self.lock_text{
            true => {
                let mut txt = "empty".to_string();
                ui.add_sized(ui.available_size(), TextEdit::multiline(&mut txt).hint_text("Output"));   
            }
            false => {
                let text = self.output_text.lock().unwrap(); //actually, i should init the TicketInfoStruct right here
                let mut out = format!("Output: {}", *text);
                ui.add_sized(ui.available_size(), TextEdit::multiline(&mut out).hint_text("Output"));
            }
        }
        

            // if self.lock_text == true{

            // }else{
            //     let mut text = "";
            // }
        }
    fn system_information(&mut self, ui: &mut Ui){
        ui.painter().rect_filled(ui.available_rect_before_wrap(),10.0,self.bg_color);
        ui.painter().rect_stroke(ui.available_rect_before_wrap(),10.0, self.border_stroke_color);
        ui.vertical(|ui| {ui.add_space(3.0);}); // leave some margin above the textEdits

        //let mut sys = System::new_with_specifics(RefreshKind::withd);// Create `System` struct.
        //sys.refresh_all(); // First we update all information of our `System` struct.
        Grid::new("tur_sheet_grid1_col1").spacing(vec2(5.0, 5.0)).num_columns(2)
        .show(ui, |ui| { // TODO

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

        if self.context.get_ticket_button_pressed == true {
            self.context.get_ticket_button_pressed = false;

            let service_num = self.context.so_number.clone();
            
            //let output_txt = self.context.output_text.clone();
            SendRequest::get_ticket(service_num, &mut MasterTechApp::default());
            
            
        }

        



        CentralPanel::default()// When displaying a DockArea in another UI, it looks better
            .frame(Frame::central_panel(&ctx.style()).inner_margin(0.))// to set inner margins to 0.
            .show(ctx, |ui| {

                let mut style = self.context.style.get_or_insert(Style::from_egui(ui.style())).clone();
                style.tabs.bg_fill = Color32::from_rgb(35,35,35);
                style.selection_color = Color32::from_rgb(92,0,87);
                style.separator.extra_interact_width = 20.0;
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
                    .draggable_tabs(self.context.draggable_tabs)
                    .show_tab_name_on_hover(self.context.show_tab_name_on_hover)
                    .show_inside(ui, &mut self.context);
            });
    }
}