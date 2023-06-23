#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release
use std::collections::HashSet;
use eframe::egui;
use egui::{{*, epaint::Shadow}, color_picker::{color_edit_button_srgba, Alpha}};
use egui_dock::{DockArea, Node, NodeIndex, Style, TabViewer, Tree};
//use egui::{color_picker::{color_edit_button_srgba, Alpha}, CentralPanel, ComboBox, Frame, Slider, TopBottomPanel, Ui, WidgetText,};

//use egui_dock::*; //{DockArea, Node, NodeIndex, Style, TabViewer, Tree};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        initial_window_size: Some(egui::vec2(600.0, 600.0)),
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

    //////////////////////////////////////////
    /*          Widgets and UI elements     */
    //////////////////////////////////////////
    widget_size: f32,
    active_tab: usize,
    default_margins: Margin,
    default_frame: Frame,
    open_tabs: HashSet<String>,
    show_close_buttons: bool,
    show_add_buttons: bool,
    draggable_tabs: bool,
    show_tab_name_on_hover: bool,

    //////////////////////////////////////////
    /*          UI Colors                   */
    //////////////////////////////////////////
    style: Option<egui_dock::Style>,
    purple_color: Color32,
    purple_stroke: Stroke,
    strip_bg_color: Color32,
    text_color: Color32,
    window_fill_color: Color32,
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
            "Style Editor" => self.style_editor(ui),
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
        let mut tree = Tree::new(vec!["TUR Sheet".to_owned(), "Style Editor".to_owned()]);
        let [a, b] = tree.split_left(NodeIndex::root(), 0.3, vec!["Inspector".to_owned()]);
        let [_, _] = tree.split_below(
            a,
            0.7,
            vec!["Scripts".to_owned(), "Style Editor".to_owned()],
        );
        let [_, _] = tree.split_below(b, 0.5, vec!["Hierarchy".to_owned()]);

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

            //////////////////////////////////////////
            /*          Widgets and UI elements     */
            //////////////////////////////////////////
            widget_size: 130.0,
            active_tab: 0,
            default_margins: Margin::same(10.0),
            default_frame: Frame{
                inner_margin: Margin::same(10.0), outer_margin: Margin::same(10.0),
                rounding: Rounding::same(10.0), shadow: Shadow::big_light(),
                fill: Color32::BLACK, stroke: Stroke { width: 1.0, color: Color32::LIGHT_GREEN },
            },
            open_tabs,
            show_close_buttons: true,
            show_add_buttons: true,
            draggable_tabs: true,
            show_tab_name_on_hover: false,

            //////////////////////////////////////////
            /*          UI Colors                   */
            //////////////////////////////////////////
            style: None,
            strip_bg_color: Color32::from_rgb(43, 41, 51),
            purple_color: Color32::from_rgb_additive(145, 29, 122),
            text_color: Color32::from_rgb(24, 186, 135),
            window_fill_color: Color32::from_rgb(38, 44, 56),
            purple_stroke: Stroke::new(1.0, Color32::from_rgb_additive(145, 29, 122))
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

        ui.painter().rect_filled(ui.available_rect_before_wrap(),10.0,self.strip_bg_color);
        ui.painter().rect_stroke(ui.available_rect_before_wrap(),10.0, self.purple_stroke);
        

        ui.vertical(|ui| {ui.add_space(3.0);}); // leave some margin above the textEdits

        
        ui.horizontal(|ui|{
            ui.add_space(15.0);
            ui.add(TextEdit::singleline(&mut self.so_number)
            .hint_text("SO#").char_limit(8).desired_width(self.widget_size));
            
            ui.add(TextEdit::singleline(&mut self.customer_name)
            .hint_text("Customer Name").desired_width(self.widget_size));
        });

        ui.horizontal(|ui| {
            ui.add_space(15.0);

            ui.add(TextEdit::singleline(&mut self.phone1)
            .hint_text("Phone Number 1").desired_width(self.widget_size));
        
            ui.add(TextEdit::singleline(&mut self.phone2)
            .hint_text("Phone Number 2").desired_width(self.widget_size));      
        });

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
        ui.horizontal_top(|ui|{
            ui.add_space(15.0);
            ui.add(TextEdit::multiline(&mut self.checkin_notes)
            .hint_text("Checkin Notes").desired_rows(15).desired_width(self.widget_size * 2.0+8.0));
        });
    }

    fn style_editor(&mut self, ui: &mut Ui) {
        ui.heading("Style Editor");

        ui.collapsing("DockArea Options", |ui| {
            ui.checkbox(&mut self.show_close_buttons, "Show close buttons");
            ui.checkbox(&mut self.show_add_buttons, "Show add buttons");
            ui.checkbox(&mut self.draggable_tabs, "Draggable tabs");
            ui.checkbox(&mut self.show_tab_name_on_hover, "Show tab name on hover");
        });

        let style = self.style.as_mut().unwrap();

        ui.collapsing("Border", |ui| {
            egui::Grid::new("border").show(ui, |ui| {
                ui.label("Width:");
                ui.add(Slider::new(&mut style.border.width, 1.0..=50.0));
                ui.end_row();

                ui.label("Color:");
                color_edit_button_srgba(ui, &mut style.border.color, Alpha::OnlyBlend);
                ui.end_row();
            });
        });

        ui.collapsing("Selection", |ui| {
            egui::Grid::new("selection").show(ui, |ui| {
                ui.label("Color:");
                color_edit_button_srgba(ui, &mut style.selection_color, Alpha::OnlyBlend);
                ui.end_row();
            });
        });

        ui.collapsing("Separator", |ui| {
            egui::Grid::new("separator").show(ui, |ui| {
                ui.label("Width:");
                ui.add(Slider::new(&mut style.separator.width, 1.0..=50.0));
                ui.end_row();

                ui.label("Extra Interact Width:");
                ui.add(Slider::new(
                    &mut style.separator.extra_interact_width,
                    0.0..=50.0,
                ));
                ui.end_row();

                ui.label("Offset limit:");
                ui.add(Slider::new(&mut style.separator.extra, 1.0..=300.0));
                ui.end_row();

                ui.label("Idle color:");
                color_edit_button_srgba(ui, &mut style.separator.color_idle, Alpha::OnlyBlend);
                ui.end_row();

                ui.label("Hovered color:");
                color_edit_button_srgba(ui, &mut style.separator.color_hovered, Alpha::OnlyBlend);
                ui.end_row();

                ui.label("Dragged color:");
                color_edit_button_srgba(ui, &mut style.separator.color_dragged, Alpha::OnlyBlend);
                ui.end_row();
            });
        });

        ui.collapsing("Tabs", |ui| {
            ui.separator();

            ui.checkbox(&mut style.tabs.fill_tab_bar, "Expand tabs");
            ui.checkbox(
                &mut style.tabs.hline_below_active_tab_name,
                "Show a line below the active tab name",
            );

            ui.separator();

            ui.checkbox(
                &mut style.tab_bar.show_scroll_bar_on_overflow,
                "Show scroll bar on tab overflow",
            );
            ui.horizontal(|ui| {
                ui.add(Slider::new(&mut style.tab_bar.height, 20.0..=50.0));
                ui.label("Tab bar height");
            });

            ComboBox::new("add_button_align", "Add button align")
                .selected_text(format!("{:?}", style.buttons.add_tab_align))
                .show_ui(ui, |ui| {
                    for align in [egui_dock::TabAddAlign::Left, egui_dock::TabAddAlign::Right] {
                        ui.selectable_value(
                            &mut style.buttons.add_tab_align,
                            align,
                            format!("{:?}", align),
                        );
                    }
                });

            ui.separator();

            ui.label("Rounding");
            ui.horizontal(|ui| {
                ui.add(Slider::new(&mut style.tabs.rounding.nw, 0.0..=15.0));
                ui.label("North-West");
            });
            ui.horizontal(|ui| {
                ui.add(Slider::new(&mut style.tabs.rounding.ne, 0.0..=15.0));
                ui.label("North-East");
            });
            ui.horizontal(|ui| {
                ui.add(Slider::new(&mut style.tabs.rounding.sw, 0.0..=15.0));
                ui.label("South-West");
            });
            ui.horizontal(|ui| {
                ui.add(Slider::new(&mut style.tabs.rounding.se, 0.0..=15.0));
                ui.label("South-East");
            });

            ui.separator();

            egui::Grid::new("tabs_colors").show(ui, |ui| {
                ui.label("Title text color, inactive and unfocused:");
                color_edit_button_srgba(ui, &mut style.tabs.text_color_unfocused, Alpha::OnlyBlend);
                ui.end_row();

                ui.label("Title text color, inactive and focused:");
                color_edit_button_srgba(ui, &mut style.tabs.text_color_focused, Alpha::OnlyBlend);
                ui.end_row();

                ui.label("Title text color, active and unfocused:");
                color_edit_button_srgba(
                    ui,
                    &mut style.tabs.text_color_active_unfocused,
                    Alpha::OnlyBlend,
                );
                ui.end_row();

                ui.label("Title text color, active and focused:");
                color_edit_button_srgba(
                    ui,
                    &mut style.tabs.text_color_active_focused,
                    Alpha::OnlyBlend,
                );
                ui.end_row();

                ui.label("Close button color unfocused:");
                color_edit_button_srgba(ui, &mut style.buttons.close_tab_color, Alpha::OnlyBlend);
                ui.end_row();

                ui.label("Close button color focused:");
                color_edit_button_srgba(
                    ui,
                    &mut style.buttons.close_tab_active_color,
                    Alpha::OnlyBlend,
                );
                ui.end_row();

                ui.label("Close button background color:");
                color_edit_button_srgba(ui, &mut style.buttons.close_tab_bg_fill, Alpha::OnlyBlend);
                ui.end_row();

                ui.label("Bar background color:");
                color_edit_button_srgba(ui, &mut style.tab_bar.bg_fill, Alpha::OnlyBlend);
                ui.end_row();

                ui.label("Outline color:")
                    .on_hover_text("The outline around the active tab name.");
                color_edit_button_srgba(ui, &mut style.tabs.outline_color, Alpha::OnlyBlend);
                ui.end_row();

                ui.label("Horizontal line color:").on_hover_text(
                    "The line separating the tab name area from the tab content area",
                );
                color_edit_button_srgba(ui, &mut style.tab_bar.hline_color, Alpha::OnlyBlend);
                ui.end_row();

                ui.label("Background color:");
                color_edit_button_srgba(ui, &mut style.tabs.bg_fill, Alpha::OnlyBlend);
                ui.end_row();
            });
        });
    }
}



impl eframe::App for MasterTechApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        TopBottomPanel::top("egui_dock::MenuBar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("View", |ui| {
                    // allow certain tabs to be toggled
                    for tab in &["Tur Sheet", "Scripts", "File Browser"] {
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
                let style = self.context.style.get_or_insert(Style::from_egui(ui.style())).clone();

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