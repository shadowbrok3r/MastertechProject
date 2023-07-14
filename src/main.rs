#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] 
// hide output_console window on Windows in release
use serde_json::Value;
use std::{path::PathBuf, sync::{Arc, Mutex}, collections::HashSet, env}; //, os::windows::thread};
use sysinfo::*; 
use eframe::{egui, glow::PROGRAM_BINARY_LENGTH};
use egui::{*, collapsing_header::CollapsingState};
use egui_dock::{DockArea, Node, NodeIndex, Style, TabViewer, Tree};
use scaffold_builder::PulledKeys;
use tokio::{runtime::Handle, sync::mpsc::{UnboundedReceiver, UnboundedSender}};
use serde::{Deserialize, Serialize};
use egui_extras::*;
use catppuccin_egui::MOCHA;

mod system_info;
mod request;
mod file_browser;
mod scaffold_builder;

use file_browser::{file_browsing, Command, Response, Directory};
use request::SendScaffoldRequest;
use system_info::RetrieveSystemInfo;

use crate::file_browser::CommandControl;

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
struct MastertechContext {
    //////////////////////////////////////////
    /*          Mastertech Vars             */
    //////////////////////////////////////////
    so_number: String,
    customer_name: String,
    phone1: String,
    phone2: String,
    checkin_notes: String,
    recommendations: String,
    checkin_rep: String,
    last_invoice_num: String,
    last_invoice_amnt: String,
    jurisdiction: String,

    webroot_key: String,
    superanti_key: String,

    salesman_cbox: scaffold_builder::Salesman,
    techs_cbox: scaffold_builder::Techs,
    ram_test_cbox: scaffold_builder::HardwareTest,
    hdd_test_cbox: scaffold_builder::HardwareTest,
    ssd_test_cbox: scaffold_builder::HardwareTest,

    output_text: String,

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
    read_hidden_files: bool,
    read_dirs_only: bool,
    dragged_directory: Option<PathBuf>,

    //////////////////////////////////////////
    /*          File Browsing               */
    //////////////////////////////////////////
    command_control: CommandControl,
    current_dir: String,
    selected_path: Option<PathBuf>,
    copied_path: Option<PathBuf>,
    destination_path: Option<PathBuf>,
    entries: Vec<Directory>,
    selected_directory: Option<Directory>,
    directory_contents: Vec<PathBuf>,
    directory_changed: bool,

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
    sysinfo_request: system_info::RetrieveSystemInfo,
    scaffold_request: SendScaffoldRequest,
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
        let mut tree = Tree::new(vec!["TUR Sheet".to_owned(), "System Information".to_owned()]);
        let [a, b] = tree.split_left(NodeIndex::root(), 0.3, vec!["File Browser".to_owned(), "Empty".to_owned()]);
        let [_, _] = tree.split_below(
            a,
            0.7,
            vec!["Console".to_owned()],
        );

            println!("does this only run once? ");
        let [_, _] = tree.split_below(b, 0.5, vec!["Scripts".to_owned()]);

        let mut open_tabs = HashSet::new();

        for node in tree.iter() {
            if let Node::Leaf { tabs, .. } = node {
                for tab in tabs {
                    open_tabs.insert(tab.clone());
                }
            }
        }
        

        // Create a watch channel with a default value
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let tx_scaffold = tx.clone();
        let tx_sysinfo = tx.clone();

        let sysinfo_request = system_info::RetrieveSystemInfo{
            tx: tx_sysinfo,
        };

        let scaffold_request = SendScaffoldRequest{
            tx: tx_scaffold
        };

        let command_control = CommandControl::new();

        let context = MastertechContext {
            //////////////////////////////////////////
            /*          Mastertech Vars             */
            //////////////////////////////////////////
            so_number: "".to_string(),
            customer_name: "".to_string(),
            phone1: "".to_string(),
            phone2: "".to_string(),
            recommendations: "".to_string(),
            checkin_notes: "".to_string(),
            send_specs: false,
            checkin_rep: "Checkin Rep: ".to_string(),
            last_invoice_num: "".to_string(),
            last_invoice_amnt: "".to_string(),
            jurisdiction: "".to_string(),

            webroot_key: "Webroot Key".to_string(),
            superanti_key: "SuperAnti Key".to_string(),

            salesman_cbox: scaffold_builder::Salesman::Jake,
            techs_cbox: scaffold_builder::Techs::Logan,
            ram_test_cbox: scaffold_builder::HardwareTest::RamNotTested,
            hdd_test_cbox: scaffold_builder::HardwareTest::HddNotTested,
            ssd_test_cbox: scaffold_builder::HardwareTest::SsdNotTested,

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
            date: None,
            animate_progress_bar: false,
            reader_bytes: 0,
            get_ticket_button_pressed: false,
            get_cps_button_pressed: false,
            get_seb_button_pressed: false,
            first_run: true,
            get_specs: false,
            spinner: false,
            read_hidden_files: false,
            read_dirs_only: false,
            dragged_directory: None,

            //////////////////////////////////////////
            /*          File Browsing               */
            //////////////////////////////////////////
            command_control: command_control,
            current_dir: env::current_dir().unwrap().to_str().unwrap().to_string(),
            selected_path: None,
            copied_path: None,
            destination_path: None,
            entries: vec![],
            selected_directory: None,
            directory_contents: vec![],
            directory_changed: false,

            //////////////////////////////////////////
            /*          UI Colors                   */
            //////////////////////////////////////////
            style: None,
            text_color: Color32::from_rgb(255, 204, 230),//(200,200,200),
            bg_color: Color32::from_rgb(28,30,36),
            border_stroke_color: Stroke::new(1.0, Color32::from_rgb_additive(150, 62, 124))
        };

        Self { context, tree, sysinfo_request, scaffold_request }
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
                        let gpu = RetrieveSystemInfo::get_gpu();
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

    fn file_browse(&mut self, ui: &mut Ui) {
        let command_sender = self.command_control.get_sender();
        let response_receiver = self.command_control.get_receiver();

        ui.horizontal_top(|ui|{
            ui.checkbox(&mut self.read_dirs_only, "Show Directories ONLY");
            ui.checkbox(&mut self.read_hidden_files ,"Show hidden files");
        });


        // Create a TextEdit for directory input.
        let dir_input = TextEdit::singleline(&mut self.current_dir)
            .id_source("dir_input");
        ui.add(dir_input);
    
        // Send a ListDir command for the current directory.
        let path = if self.current_dir.is_empty() {
            env::current_dir().unwrap()
        } else {
            PathBuf::from(self.current_dir.clone())
        };
        let depth = 3; // Default to listing one level of subdirectories.
        if let Err(e) = command_sender.send(Command::ListDir(path.clone(), depth)) {
            eprintln!("Error sending command: {:?}", e);
        }
        self.directory_changed = false;  // Reset the flag for the next frame.

        let scroll_area = ScrollArea::new([true, true]).id_source("file_browser_scroll");
        scroll_area.show(ui, |ui| {
            for directory in &self.entries {
                if let Some(double_clicked_dir) = Self::directory_ui(ui, directory, 0, &mut self.selected_path, command_sender.clone()) {
                    self.current_dir = double_clicked_dir.to_str().unwrap().to_owned();
                    let path = PathBuf::from(self.current_dir.clone());
                    if let Err(e) = command_sender.send(Command::ListDir(path, depth)) {
                        eprintln!("Error sending command: {:?}", e);
                    }
                }
            }
        });
        

        // Handle responses from the file_browsing function to update the file/directory listing.
        while let Ok(response) = response_receiver.try_recv() {
            match response {
                Response::DirectoryListing(directory) => {
                    self.entries = vec![directory];
                },
                Response::Success(message) => {
                    // Display the message to the user in some way.
                    // This will depend on how your GUI is structured.
                    println!("{}", message);
                },
                Response::Error(error) => {
                    // Display the error to the user in some way.
                    // This will depend on how your GUI is structured.
                    eprintln!("Error: {:?}", error);
                },
            }
        }

        
    
        // Handling for copy, cut, and delete operations.
        if ui.input(|i| i.key_pressed(Key::C) && i.modifiers.ctrl) {
            // Assume self.selected_directory is the path of the selected file or directory
            if let Some(selected_directory) = &self.selected_directory {
                self.copied_path = Some(selected_directory.path.clone());
            }
        }
        if ui.input(|i| i.key_pressed(Key::V) && i.modifiers.ctrl) {
            if let Some(copied_path) = &self.copied_path {
                let destination_path = PathBuf::from(self.current_dir.clone());
                if let Err(e) = command_sender.send(Command::Copy(copied_path.clone(), destination_path)) {
                    eprintln!("Error sending command: {:?}", e);
                }
            }
        }
        if ui.input(|i| i.key_pressed(Key::X) && i.modifiers.ctrl) {
             // Assume self.selected_directory is the path of the selected file or directory
             if let Some(selected_directory) = &self.selected_directory {
                let destination_path = PathBuf::from(self.current_dir.clone());
                if let Err(e) = command_sender.send(Command::Move(selected_directory.path.clone(), destination_path)) {
                    eprintln!("Error sending command: {:?}", e);
                }
            }
        }
        if ui.input(|i| i.key_pressed(Key::Delete)) {
           // Assume self.selected_directory is the path of the selected file or directory
           if let Some(selected_directory) = &self.selected_directory {
            if let Err(e) = command_sender.send(Command::Delete(selected_directory.path.clone())) {
                eprintln!("Error sending command: {:?}", e);
            }
        }
        }
        // Listen for changes in the directory input.
        if ui.input(|i| i.key_released(Key::Enter)) {
            let path = if self.current_dir.is_empty() {
                env::current_dir().unwrap()
            } else {
                PathBuf::from(self.current_dir.clone())
            };
            
            if let Err(e) = command_sender.send(Command::ListDir(path, depth)) {
                eprintln!("Error sending command: {:?}", e);
            }
        }   

        // Always send a ListDir command for the current directory at the end of the function.
        let path = if self.current_dir.is_empty() {
            env::current_dir().unwrap()
        } else {
            PathBuf::from(self.current_dir.clone())
        };
        if let Err(e) = command_sender.send(Command::ListDir(path, depth)) {
            eprintln!("Error sending command: {:?}", e);
        }
    }

    fn directory_ui(ui: &mut Ui, directory: &Directory, depth: usize, selected_directory: &mut Option<PathBuf>, 
        command_sender: UnboundedSender<Command>)  -> Option<PathBuf>{
        let mut double_clicked_dir = None;
        let id = ui.make_persistent_id(directory.path.to_str().unwrap());
        let is_selected = match selected_directory {
            Some(selected_path) => *selected_path == directory.path,
            None => false,
        };

        CollapsingState::load_with_default_open(ui.ctx(), id, true)
            .show_header(ui, |ui| {
                let interaction = ui.selectable_label(is_selected, directory.path.file_name().unwrap().to_string_lossy());
                if interaction.clicked() {
                    *selected_directory = Some(directory.path.clone());
                }
                
                if interaction.double_clicked() {
                    // For testing, print a message whenever a double click event is detected.
                    println!("Double clicked: {}", directory.path.display());
            
                    // Send a command to list this directory
                    if let Err(e) = command_sender.send(Command::ListDir(directory.path.clone(), depth + 1)) {
                        eprintln!("Error sending command: {:?}", e);
                    }
            
                    double_clicked_dir = Some(directory.path.clone());
                }
                if interaction.secondary_clicked() {
                    // Rename the directory
                    let new_name = Self::rename_directory(ui, directory);
                    if let Some(new_name) = new_name {
                        if let Err(e) = command_sender.send(Command::Rename(directory.path.clone(), directory.path.with_file_name(new_name))) {
                            eprintln!("Error sending command: {:?}", e);
                        }
                    }
                }
            })
            .body(|ui| {
                for sub_directory in &directory.children {
                    Self::directory_ui(ui, sub_directory, depth + 1, selected_directory, command_sender.clone());
                }
            });
        double_clicked_dir
    }
    
    fn rename_directory(ui: &mut Ui, directory: &Directory) -> Option<String> {
        // Replace this function body with your UI code to get the new directory name from the user.
        Some(format!("new_{}", directory.path.file_name().unwrap().to_string_lossy()))
    }
    
    
    
    

    
    fn scripts(&mut self, ui: &mut Ui){ }
}

impl eframe::App for MasterTechApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        catppuccin_egui::set_theme(ctx, catppuccin_egui::MOCHA);
        
        self.context.ctx = ctx.clone();
        let ticket_sender = self.scaffold_request.tx.clone();
        let cps_sender = self.scaffold_request.tx.clone();
        let specs_sender = self.sysinfo_request.tx.clone();

        if self.context.get_ticket_button_pressed == true {
            self.context.get_ticket_button_pressed = false;
            let service_num = self.context.so_number.clone();
            self.context.spinner = true;
            SendScaffoldRequest::get_ticket(service_num, ticket_sender); 
        }

        if self.context.get_cps_button_pressed == true {
            self.context.get_cps_button_pressed = false;
            let service_num = self.context.so_number.clone();
            self.context.spinner = true;
            SendScaffoldRequest::get_cps(service_num, cps_sender);
        }   

        if self.context.get_specs == true{
            self.context.get_specs = false;
            self.context.spinner = true;
            RetrieveSystemInfo::get_system_specs(specs_sender);
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
            else if let Ok(info) = serde_json::from_str::<system_info::SystemInformation>(&message) {
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