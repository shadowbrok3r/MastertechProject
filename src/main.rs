#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide output_console window on Windows in release
use std::{collections::HashSet, borrow::BorrowMut};
use eframe::egui;
use egui::*;
use egui_dock::{DockArea, Node, NodeIndex, Style, TabViewer, Tree, ButtonsStyle, SeparatorStyle, TabBarStyle, TabStyle};
use egui_extras::Column;
mod submit_tur;

fn main() -> eframe::Result<()> {
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
    checkin_notes: String,
    output: String,

    //////////////////////////////////////////
    /*          Widgets and UI elements     */
    //////////////////////////////////////////
    widget_size: f32,
    open_tabs: HashSet<String>,
    show_close_buttons: bool,
    show_add_buttons: bool,
    draggable_tabs: bool,
    show_tab_name_on_hover: bool,

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
    tree: Tree<String>,
}

impl TabViewer for MastertechContext {
    type Tab = String;

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        match tab.as_str() {
            "TUR Sheet" => self.tur_sheet(ui),
            "Console" => self.output_console(ui),
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
            checkin_notes: "".to_string(),
            output: "".to_string(),

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

            //////////////////////////////////////////
            /*          UI Colors                   */
            //////////////////////////////////////////
            style: None,
            text_color: Color32::from_rgb(200,200,200),
            bg_color: Color32::from_rgb(28,30,36),
            border_stroke_color: Stroke::new(1.0, Color32::from_rgb_additive(67,251,162))
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
        ui.painter().rect_filled(ui.available_rect_before_wrap(),10.0,self.bg_color);
        ui.painter().rect_stroke(ui.available_rect_before_wrap(),10.0, self.border_stroke_color);
        ui.vertical(|ui| {ui.add_space(3.0);}); // leave some margin above the textEdits
        
        //Grid::new("tur_sheet").num_columns(2).min_col_width(16.0).spacing([16.0, 8.0])
        //.show(ui, |ui| {
        ui.columns(2,|column|{

            column[0].vertical(|ui|{
                //ui.painter().text(Pos2::default(),Align2::CENTER_CENTER, "text", FontId::monospace(12.0), Color32::RED);
                ui.vertical_centered(|ui|{ui.heading("Ticket Information");});
                
                ui.horizontal(|ui|{
                    ui.add_space(15.0);
                    ui.add(TextEdit::singleline(&mut self.so_number)
                    .hint_text("SO#").char_limit(8).desired_width(self.widget_size));
                    
                    ui.add(TextEdit::singleline(&mut self.customer_name)
                    .hint_text("Customer Name").desired_width(self.widget_size));
                });
                ui.end_row();
                ui.horizontal(|ui| {
                    ui.add_space(15.0);
        
                    ui.add(TextEdit::singleline(&mut self.phone1)
                    .hint_text("Phone Number 1").desired_width(self.widget_size));
                
                    ui.add(TextEdit::singleline(&mut self.phone2)
                    .hint_text("Phone Number 2").desired_width(self.widget_size));      
                });
                ui.end_row();
                // Salesman and Tech ComboBoxes
                ui.horizontal(|ui|{
                    ui.add_space(15.0);
        
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
                });
                ui.end_row();
                ui.horizontal(|ui|{
                    ui.add_space(15.0);
                    ui.add(TextEdit::multiline(&mut self.checkin_notes)
                    .hint_text("Checkin Notes").desired_rows(15).desired_width(self.widget_size * 2.0+8.0));
                });
                ui.end_row();
                ui.horizontal(|ui|{
                    ui.add_space(15.0);
                    ui.button("Submit");
                    if ui.button("Submit").clicked(){
                        //return Action
                    }
                });
                ui.end_row();
            });
            column[1].vertical(|ui|{
                ui.horizontal(|ui|{
                    ui.add_space(15.0);
                    ui.add(TextEdit::multiline(&mut self.checkin_notes)
                    .hint_text("stuffs").desired_rows(15).desired_width(self.widget_size * 2.0+8.0));
                });
                ui.end_row();
            });
        });
        //});


        

    }

    fn output_console(&mut self, ui: &mut Ui) { 
        ui.add_sized(ui.available_size(),
            TextEdit::multiline(&mut self.output).hint_text("Output")
            
        );

        //ui.;

    }
}



impl eframe::App for MasterTechApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        TopBottomPanel::top("egui_dock::MenuBar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("View", |ui| {

                    // allow certain tabs to be toggled
                    for tab in &["Tur Sheet", "Scripts", "Console", "System Information"] {
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

        CentralPanel::default()
            // When displaying a DockArea in another UI, it looks better
            // to set inner margins to 0.
            .frame(Frame::central_panel(&ctx.style()).inner_margin(0.))
            .show(ctx, |ui| {
                let mut style = self.context.style.get_or_insert(Style::from_egui(ui.style())).clone();
                style.tabs.bg_fill = Color32::from_rgb(29,28,30);
                style.selection_color = Color32::from_rgb(92,0,87);
                style.separator.extra_interact_width = 20.0;
                style.separator.color_hovered = Color32::from_rgba_premultiplied(50,93,80,77);
                style.separator.color_idle = Color32::from_rgba_premultiplied(17,17,33,5);
                style.separator.color_dragged = Color32::from_rgba_premultiplied(189,189,189,130);
                style.buttons.add_tab_align = egui_dock::TabAddAlign::Left;
                style.tabs.rounding.nw = 15.0;
                style.tabs.rounding.ne = 15.0;
                style.tabs.text_color_active_focused = Color32::from_rgba_premultiplied(0, 254, 158, 255);
                style.buttons.close_tab_color = Color32::from_rgba_premultiplied(118, 0, 129, 58);
                
                //style.tabs.outline_color
                //style.tabs.bg_fill
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