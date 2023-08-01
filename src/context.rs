use serde_json::Value;
use std::{sync::{Arc, Mutex}, collections::HashSet}; //, os::windows::thread};

use eframe::egui;
use egui::*;
use egui_dock::{Node, NodeIndex, Tree, TabViewer};
use scaffold::PulledKeys;
use tokio::sync::mpsc::channel;
use egui_extras::{*, DatePickerButton};

use crate::{
    scaffold::{self, TicketInformation}, 
    file_browser::FileBrowser, 
    system_info, 
    request::SendRequest,
    system_info::RetrieveSystemInfo
};

/** 
TODO, dont make all of these public, maybe we
need a getter/setter, or use some Higher-level Methods: 
Instead of manipulating the fields of your struct directly, 
consider whether you can introduce higher-level methods that 
perform the operations you need.

For example, instead of getting a Vec field and pushing an 
element to it, you might introduce a add_element method:

pub struct MyStruct {
    vec_field: Vec<i32>,
    // other fields...
}

impl MyStruct {
    pub fn add_element(&mut self, element: i32) {
        self.vec_field.push(element);
    }
    // other methods...
}

*/
pub struct MastertechContext { 
    pub so_number: String,
    pub recommendations: String,

    pub ticket_info: TicketInformation,
    pub keys: PulledKeys,
    pub file_browser: Arc<Mutex<FileBrowser>>,
    pub client: reqwest::Client,
    scaffold_request: SendRequest,
    sysinfo_request: system_info::RetrieveSystemInfo,
    
    pub salesman_cbox: scaffold::Salesman,
    pub techs_cbox: scaffold::Techs,
    pub ram_test_cbox: scaffold::HardwareTest,
    pub hdd_test_cbox: scaffold::HardwareTest,
    pub ssd_test_cbox: scaffold::HardwareTest,

    pub output_text: String,
    
    pub cpu_name: String,
    pub total_ram: String,
    pub system_name: String,
    pub disks: Value,
    pub disk_num: usize,

    pub tur_sheet_tab: String,
    pub output_console_tab: String,
    pub system_info_tab: String,
    pub file_browser_tab: String,
    pub scripts_tab: String,

    pub rx: Option<std::sync::mpsc::Receiver<String>>,
    pub ctx: egui::Context,
    pub widget_size: f32,
    pub open_tabs: HashSet<String>,
    pub show_close_buttons: bool,
    pub show_add_buttons: bool,
    pub draggable_tabs: bool,
    pub show_tab_name_on_hover: bool,

    pub date: Option<chrono::NaiveDate>,
    
    
    pub reader_bytes: u32,

    pub animate_progress_bar: bool,
    pub specs_first_run: bool,
    pub file_browse_run: bool,
    pub get_specs: bool,
    pub send_specs: bool,
    pub spinner: bool,
    
    pub style: Option<egui_dock::Style>,
    pub text_color: Color32,
    pub border_stroke_color: Stroke,
    pub bg_color: Color32
}

impl TabViewer for MastertechContext {
    type Tab = String;

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {

        match tab.as_str() {
            "TUR Sheet" => self.tur_sheet(ui),
            "Console" => self.output_console(ui),
            "Scripts" => self.scripts(ui),
            "File Browser 📂" => self.file_browse(ui),
            "System Information" => self.system_information(ui),
            _ => {
                let sysinfo_tab = &self.system_info_tab.to_string();
                if ui.label(tab.as_str()).clicked(){
                    if tab.as_str() == sysinfo_tab{
                        self.specs_first_run = true;
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

pub struct MasterTechApp {
    pub context: MastertechContext,
    pub tree: Tree<String>,
}

impl Default for MasterTechApp {
    fn default() -> Self {
        let mut tree = Tree::new(vec!["TUR Sheet".to_owned(), "System Information".to_owned()]);
        let [a, _] = tree.split_left(NodeIndex::root(), 0.3, vec!["File Browser 📂".to_owned(), "Scripts".to_owned()]);
        let [_, _] = tree.split_below(
            a,
            0.7,
            vec!["Console".to_owned()],
        );//let [_, _] = tree.split_below(b, 0.5, vec!["Scripts".to_owned()]);

        let mut open_tabs = HashSet::new();

        for node in tree.iter() {
            if let Node::Leaf { tabs, .. } = node {
                for tab in tabs {
                    open_tabs.insert(tab.clone());
                }
            }
        }

        let client = reqwest::Client::new();

        // Create watch channel with a default value
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let tx_scaffold = tx.clone();
        let tx_sysinfo = tx.clone();

        let sysinfo_request = system_info::RetrieveSystemInfo{
            tx: tx_sysinfo,
        };

        let scaffold_request = SendRequest{
            tx: tx_scaffold,
        };

        let ticket_information = TicketInformation {
            cust_code: "".to_string(),
            user_id: "".to_string(),
            terms: "".to_string(),
            doc_alias: "".to_string(),
            department: "".to_string(),
            jurisdiction: "".to_string(),
            invoice_amnt: "".to_string(),
            customer_name: "".to_string(),
            customer_phone_1: "".to_string(),
            customer_phone_2: "".to_string(),
            customer_email: "".to_string(),
            last_invoice_number: "".to_string(),
            last_invoice_amount: "".to_string(),
            total_invoice_count: "".to_string(),
            checkin_notes: "".to_string(),
            item_codes: "".to_string(),
        };

        let context = MastertechContext {
            so_number: "".to_string(),
            recommendations: "".to_string(),

            ticket_info: ticket_information,
            keys: PulledKeys { 
                webroot_key: "Webroot Key".to_string(), 
                superanti_key: "SuperAnti Key".to_string() 
            },
            scaffold_request,
            sysinfo_request,
            client,
            file_browser: Arc::new(Mutex::new(FileBrowser::new())),

            // I should just make this section take
            // the whole enum
            salesman_cbox: scaffold::Salesman::Jake, 
            techs_cbox: scaffold::Techs::Logan, 
            ram_test_cbox: scaffold::HardwareTest::RamNotTested,
            hdd_test_cbox: scaffold::HardwareTest::HddNotTested,
            ssd_test_cbox: scaffold::HardwareTest::SsdNotTested,

            output_text: "".to_string(),

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
            file_browser_tab: "File Browser 📂".to_string(),
            date: None,
            animate_progress_bar: false,
            reader_bytes: 0,

            send_specs: false,

            specs_first_run: true,
            file_browse_run: false,
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
                                                let service_num = self.so_number.clone();
                                                self.spinner = true;
                                                SendRequest::get_ticket(service_num, self.scaffold_request.tx.clone(), self.client.clone()); 
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

                                            ui.add(TextEdit::singleline(&mut self.ticket_info.customer_name)
                                            .hint_text("Customer Name  ").desired_width(self.widget_size + 3.0));

                                            ui.end_row();

                                                                /*     ROW 2     */
                                            ui.add(TextEdit::singleline(&mut self.ticket_info.customer_phone_1)
                                            .hint_text("Phone Number 1").desired_width(self.widget_size));
                                            ui.add(TextEdit::singleline(&mut self.ticket_info.customer_phone_2)
                                            .hint_text("Phone Number 2").desired_width(self.widget_size + 3.0));      
                                            
                                            ui.end_row();

                                                                /*     ROW 3     */
                                            ComboBox::from_id_source("salesman_cbox").width(self.widget_size)
                                            .selected_text(format!("{:?}", self.salesman_cbox))
                                            .show_ui(ui, |ui| {
                                                ui.selectable_value(&mut self.salesman_cbox, scaffold::Salesman::Jake, "Jake");
                                                ui.selectable_value(&mut self.salesman_cbox, scaffold::Salesman::Danny, "Danny");
                                            });


                                            ComboBox::from_id_source("techs_cbox").width(self.widget_size)
                                            .selected_text(format!("{:?}", self.techs_cbox))
                                            .show_ui(ui, |ui| {
                                                
                                                ui.selectable_value(&mut self.techs_cbox, scaffold::Techs::Logan, "Logan");
                                                ui.selectable_value(&mut self.techs_cbox, scaffold::Techs::Bread, "Bread");
                                                ui.selectable_value(&mut self.techs_cbox, scaffold::Techs::Taco, "Taco");
                                            });    
                                            
                                            ui.end_row();
                                                                /*     ROW 4     */
                                            if ui.add(Button::new("Get Keys").min_size(vec2(self.widget_size, 5.0)))
                                            .clicked(){ 
                                                let service_num = self.so_number.clone();
                                                self.spinner = true;
                                                SendRequest::get_cps(service_num, self.scaffold_request.tx.clone(), self.client.clone());
                                            }
                                            
                                            if ui.add(Button::new("Check SEB").min_size(vec2(self.widget_size, 5.0)))
                                            .clicked(){ 
                                                
                                                //check_seb_info
                                            }
                
                                            ui.end_row();
                                            
                                                                /*     ROW 5     */
                                            if ui.add(Button::new(RichText::new(format!("{}", self.keys.webroot_key)).size(9.0)
                                            .color(Color32::from_rgb(102, 255, 153))
                                            .strong())
                                            .min_size(vec2(self.widget_size + 2.0, 8.0)))
                                            .on_hover_text("Click To Copy Webroot Key to Clipboard")
                                            .clicked(){ 
                                                let webroot = self.keys.webroot_key.clone();
                                                ui.output_mut(|o| o.copied_text = webroot);
                                            }
                                                
                                            if ui.add(Button::new(RichText::new(format!("{}", self.keys.superanti_key)).size(9.0)
                                            .color(Color32::from_rgb(255, 61, 126))
                                            .strong())
                                            .min_size(vec2(self.widget_size + 2.0, 8.0)))
                                            .on_hover_text("Click To Copy SAS Key to Clipboard")
                                            .clicked(){ 
                                                let sas = self.keys.superanti_key.clone();
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
                                        ui.add(TextEdit::multiline(&mut self.ticket_info.checkin_notes)
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
                                                ui.selectable_value(&mut self.ram_test_cbox, scaffold::HardwareTest::RamFail, "RAM Fail");
                                                ui.selectable_value(&mut self.ram_test_cbox, scaffold::HardwareTest::RamPass, "RAM Pass");
                                                ui.selectable_value(&mut self.ram_test_cbox, scaffold::HardwareTest::RamNotTested, "RAM Not Tested");
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
                                            ui.selectable_value(&mut self.ssd_test_cbox, scaffold::HardwareTest::SsdFail, "SSD Fail");
                                            ui.selectable_value(&mut self.ssd_test_cbox, scaffold::HardwareTest::SsdPass, "SSD Pass");
                                            ui.selectable_value(&mut self.ssd_test_cbox, scaffold::HardwareTest::SsdNotTested, "SSD Not Tested");
                                        }); // Combo Box

                                                            /*     ROW 2     */
                                        ComboBox::from_id_source("hdd_cbox").width(self.widget_size - 5.0)
                                        .selected_text(format!("{}", self.hdd_test_cbox.as_str()))
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut self.hdd_test_cbox, scaffold::HardwareTest::HddFail, "HDD Fail");
                                            ui.selectable_value(&mut self.hdd_test_cbox, scaffold::HardwareTest::HddPass, "HDD Pass");
                                            ui.selectable_value(&mut self.hdd_test_cbox, scaffold::HardwareTest::HddNotTested, "HDD Not Tested");
                                        }); // Combo Box
                                        ui.end_row();
                                    }); // Grid   

                                    
                                    ui.vertical(|ui|{ui.add_space(18.0);});

                                    if self.spinner == true{
                                        ui.add(Spinner::new());
                                    }
                                    

                                    ui.vertical(|ui|{ui.add_space(18.0);});

                                    ui.checkbox(&mut self.send_specs, "Send System Info");

                                    //#[cfg(feature = "chrono")]
                                    
                                    let date = self.date.get_or_insert_with(|| chrono::offset::Utc::now().date_naive());
                                    ui.add(DatePickerButton::new(date));
                                    ui.end_row();
                                    

                                    if ui.add(Button::new(RichText::new("Submit TUR Sheet")
                                    .color(Color32::from_rgb(255, 204, 255))
                                    .strong()
                                    .italics())
                                    .stroke(Stroke::new(2.0, Color32::from_rgb(191, 33, 101))))
                                    .clicked(){ 
                                        self.spinner = true;
                                        let cust = &self.ticket_info.customer_name;
                                        let so_num = &self.so_number;
                                        let salesman = &format!("{:?}", &self.salesman_cbox);
                                        let checkin_rep = &self.ticket_info.user_id;
                                        let technician = &format!("{:?}", &self.techs_cbox);
                                        let system_name = &self.system_name;
                                        let cpu_name = &self.cpu_name;
                                        let total_ram = &self.total_ram;
                                        let ssd_test = &format!("{:?}", &self.ssd_test_cbox);
                                        let hdd_test = &format!("{:?}", &self.hdd_test_cbox);
                                        let ram_test = &format!("{:?}", &self.ram_test_cbox);
                                        let checkin_notes = &self.ticket_info.checkin_notes;
                                        let recommendations = &self.recommendations;

                                        println!("{:?}", self.date);
                                        
                                        let task_name = (cust, so_num);
                                        let html_notes = format!(
                                            "<body><h2><strong><code>Ticket Info</code></strong></h2><ul>\n
                                            <li><strong>Salesman:</strong>              {salesman}</li>\n
                                            <li><strong>Checkin rep:</strong>           {checkin_rep}</li>\n
                                            <li><strong>Technician:</strong>            {technician}</li></ul>\n
                                            <strong><h2><code>      Computer Info       </code></h2></strong>\n
                                            <table>
                                                <tr>
                                                    <td></td>
                                                    <td>Details</td>
                                                </tr>
                                                <tr>
                                                    <td>OS</td>
                                                    <td>{system_name}</td>
                                                </tr>
                                                <tr>
                                                    <td>CPU</td>
                                                    <td>{cpu_name}</td>
                                                </tr>
                                                <tr>
                                                    <td>RAM</td>
                                                    <td>{total_ram}</td>
                                                </tr>
                                                <tr>
                                                    <td>GPU</td>
                                                    <td></td>
                                                </tr>
                                                <tr>
                                                    <td>Antivirus</td>
                                                    <td></td>
                                                </tr>
                                                <tr>
                                                    <td>SEB</td>
                                                    <td></td>
                                                </tr>
                                            </table>
                                            <table>
                                                <tr>
                                                    <td>Drive Letter</td>
                                                    <td>Available Space</td>
                                                    <td>Total Space</td>
                                                    <td>S/N# (may be encoded)</td>
                                                </tr>
                                                <tr>
                                                    <td></td>
                                                    <td></td>
                                                    <td></td>
                                                    <td></td>
                                                </tr>
                                                <tr>
                                                    <td></td>
                                                    <td></td>
                                                    <td></td>
                                                    <td></td>
                                                </tr>
                                                <tr>
                                                    <td></td>
                                                    <td></td>
                                                    <td></td>
                                                    <td></td>
                                                </tr>
                                                <tr>
                                                    <td></td>
                                                    <td></td>
                                                    <td></td>
                                                    <td></td>
                                                </tr>
                                            </table>
                                            <ul>
                                            <li><strong>SSD test:</strong>     {ssd_test}</li>\n
                                            <li><strong>HDD test:</strong>     {hdd_test}</li>\n
                                            <li><strong>RAM test:</strong>     {ram_test}</li></ul>\n
                                
                                            <h2><strong><code>      Notes       </code></strong></h2><ul>\n
                                            <li><strong>        Checkin Notes:      </strong>\n     {checkin_notes}</li>\n
                                            <li><strong>        Recommendations:        </strong>\n     {recommendations}</li></ul></body>",
                                        );
                                        // I think i should probably just pass send_ticket_request the
                                        // whole ticket_info struct

                                        /*
                                            cust_code: "".to_string(),
                                            user_id: "".to_string(),
                                            terms: "".to_string(),
                                            doc_alias: "".to_string(),
                                            department: "".to_string(),
                                            jurisdiction: "".to_string(),
                                            invoice_amnt: "".to_string(),

                                            customer_email: "".to_string(),
                                            last_invoice_number: "".to_string(),
                                            last_invoice_amount: "".to_string(),
                                            total_invoice_count: "".to_string(),

                                            item_codes: "".to_string(),
                                        */

                                        SendRequest::send_ticket_request(
                                            self.scaffold_request.tx.clone(), 
                                            self.client.clone(), 
                                            task_name,
                                            html_notes
                                        );
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

        if self.specs_first_run == true{
            // self.get_specs = true;
            let specs_sender = self.sysinfo_request.tx.clone();
            self.spinner = true;
            
            RetrieveSystemInfo::get_system_specs(specs_sender);
        }
        self.specs_first_run = false;
        
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
                    row.col(|_ui|{
                        //let gpu = RetrieveSystemInfo::get_gpu();
                        //ui.label(format!("{}", gpu));
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
            .body(|body| {
                body.rows(
                20.0,  // Replace with your desired row height
                self.disk_num,
                |disk_index, mut row| 
                {
                    if let Some(disk) = self.disks.get(disk_index){
                        //println!("disks: {:#?}", disk);

                        //let disk_name = format!("{:#?}", disk.get("name"));
                        let disk_letter = format!("{}", disk.get("letter").and_then(Value::as_str).unwrap_or(""));

                        row.col(|ui| {
                            ui.label(disk_index.to_string());  // Show disk index
                        });
                        row.col(|ui| {
                            ui.label(disk_letter);  // Show disk letter
                        });
                        // let disk_used = format!("{}", (disk.get("total space").and_then(Value::as_u64).unwrap_or(0)) - 
                        // (disk.get("available space").and_then(Value::as_u64).unwrap_or(0)));
                        // row.col(|ui| {
                        //     ui.label(disk_used.to_string());  // Show disk space
                        // });
                        row.col(|ui| {
                            let disk_space = format!(
                                "{} Gb / {} Gb",
                                disk.get("available space").and_then(Value::as_str).unwrap_or(""),
                                disk.get("total space").and_then(Value::as_str).unwrap_or("")
                            );
                            ui.label(disk_space);  // Show disk space
                        });
                        self.ctx.request_repaint();
                        self.spinner = false;
                    }   

                });
            });
        });
        self.spinner = false;
    }

    fn file_browse(&mut self, ui: &mut Ui) {
        let (command_tx, command_rx) = channel(4);
        // Lock the Mutex and show the GUI
        let file_browser_clone = Arc::clone(&self.file_browser);
        let mut file_browser = file_browser_clone.lock().unwrap();
        file_browser.show(ui, &self.ctx.clone(), command_tx, command_rx);
    }
    
    fn scripts(&mut self, _ui: &mut Ui){ }
}
