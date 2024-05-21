use std::collections::HashSet;
use egui::{Ui, WidgetText};
use egui_dock::{DockState, Node, NodeIndex, SurfaceIndex, TabViewer};
use ratatui::Terminal;
use ratframe::RataguiBackend;
use web_time::{Duration, Instant};

use crate::tabs::terminal::chart::App;

pub struct MtechServer {
    pub context: MtechServerContext,
    pub tree: DockState<String>,
}


pub struct MtechServerContext{
    pub open_tabs: HashSet<String>,
    pub style: Option<egui_dock::Style>,
    pub show_close_buttons: bool,
    pub show_add_buttons: bool,
    pub draggable_tabs: bool,
    pub show_tab_name_on_hover: bool,
    pub terminal: Terminal<RataguiBackend>,
    pub chart_app: App,
    pub tick_rate: Duration,
    pub last_tick: Instant
}


impl TabViewer for MtechServerContext {
    type Tab = String;

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {

        match tab.as_str() {
            "Lil menu" => self.simple_demo_menu(ui),
            "Terminal" => self.terminal(ui),
            _ => { } 
        }
    }

    fn context_menu(&mut self, ui: &mut Ui, tab: &mut Self::Tab, _surface_index: SurfaceIndex, _node_index: NodeIndex) {
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
    
    fn on_add(&mut self, _surface_index: SurfaceIndex, _node_index: NodeIndex) {
        
        // for node in tree[SurfaceIndex::main()].iter() {
        //     if let Node::Leaf { tabs, .. } = node {
        //         for tab in tabs {
        //             open_tabs.insert(tab.clone());
        //         }
        //     }
        // }
        // self.open_tabs.insert(surface_index.);
    }

}

impl Default for MtechServer{
    fn default() -> Self {
        let mut tree = DockState::new(
            vec![
                "Lil menu".to_owned(),
            ]
        );

        tree.translations.tab_context_menu.eject_button = "Undock".to_owned();

        
        let [_a, _b] = tree
            .main_surface_mut()
            .split_left(
                NodeIndex::root(),
                0.30, 
                vec![
                    "File Browser 📂".to_owned(),
        ]);

        let [_a, b] = tree
            .main_surface_mut()
            .split_below(
                NodeIndex::root(),
                0.65, 
                vec![
                    "Terminal".to_owned(),
            ]
        );

        let [_, _] = tree
            .main_surface_mut()
            .split_left(
            b,
            0.45,
            vec!["System Information".to_owned()],
        );

        let [_, _] = tree
            .main_surface_mut()
            .split_left(
            b,
            0.20,
            vec!["Scripts".to_owned()],
        );


        let mut open_tabs = HashSet::new();

        for node in tree[SurfaceIndex::main()].iter() {
            if let Node::Leaf { tabs, .. } = node {
                for tab in tabs {
                    open_tabs.insert(tab.clone());
                }
            }
        }
        
        
        let backend = RataguiBackend::new_with_fonts(
            10,
            10,
            "Regular".into(),
            "Bold".into(),
            "Oblique".into(),
            "BoldOblique".into(),
        );

        
        let terminal = Terminal::new(backend).unwrap();
        let tick_rate = Duration::from_millis(30);
        let chart_app = App::new();
        let mut last_tick = Instant::now();

        let context = MtechServerContext{
            open_tabs,
            style: None,
            show_close_buttons: true,
            show_add_buttons: true,
            draggable_tabs: true,
            show_tab_name_on_hover: false,
            terminal,
            chart_app,
            tick_rate,
            last_tick
        };
        
        Self {
            context,
            tree,
        }
    }
}

