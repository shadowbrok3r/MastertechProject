use std::{cell::Cell, collections::HashSet, rc::Rc};
use crossbeam::channel::{self, Receiver, Sender};
use egui::{Ui, WidgetText};
use egui_dock::{DockState, Node, NodeIndex, SurfaceIndex, TabViewer};
use gloo_worker::Spawnable;
use ratatui::Terminal;
use ratframe::{NewCC, RataguiBackend};
use wasm_bindgen_futures::spawn_local;
use web_time::{Duration, Instant};
use crate::{database::{schema::TaskPayload, Database}, tabs::terminal::chart::App};
use mtechserver_two::webworker::WebWorker;

pub struct MtechServer {
    pub context: MtechServerContext,
    pub tree: DockState<String>,
}


pub struct MtechServerContext{
    /// collection of all open tabs in ui
    pub open_tabs: HashSet<String>,
    /// egui dock styling
    pub style: Option<egui_dock::Style>,
    // pub client: reqwest::Client,

    /// Terminal setup for console tab
    pub terminal: Terminal<RataguiBackend>,
    /// example chart for console tab
    pub chart_app: App,
    /// update period for chart
    pub tick_rate: Duration,
    /// last tick of example chart
    pub last_tick: Instant,

    ///Gets data from the first run of the main loop
    pub first_run: bool,
    
    /// Database connection
    pub database: Option<Database>,
    /// All contained task data from database
    pub task_data: Option<Vec<TaskPayload>>,
    /// Receives task data over crossbeam channel
    pub db_data_rx: Receiver<Vec<TaskPayload>>,
    /// Sends task data over crossbeam channel
    pub db_data_tx: Sender<Vec<TaskPayload>>,
    /// Receives Database connection over crossbeam channel
    pub db_rx: Receiver<Database>,
    /// Sends Database connection over crossbeam channel
    pub db_tx: Sender<Database>,

    pub bridge: Option<gloo_worker::WorkerBridge<WebWorker>>,
    
    pub data_update: Option<Rc<Cell<Option<u32>>>>,
}


impl TabViewer for MtechServerContext {
    type Tab = String;

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {

        match tab.as_str() {
            "Lil menu" => self.simple_demo_menu(ui),
            "Terminal" => self.terminal(ui),
            "Tasks" => self.tasks(ui),
            "My Tasks" => self.my_tasks(ui),
            "Web Console" => self.web_console(ui),
            _ => { } 
        }
    }

    fn context_menu(&mut self, ui: &mut Ui, tab: &mut Self::Tab, _surface_index: SurfaceIndex, _node_index: NodeIndex) {
        match tab.as_str() {
            "My Tasks" => self.simple_demo_menu(ui),
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

// impl MtechServer{
//     fn new(cc: &eframe::CreationContext<'_>) -> Self {}
// }


impl NewCC for MtechServer {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {

        let mut tree = DockState::new(
            vec![
                "My Tasks".to_owned(),
            ]
        );

        tree.translations.tab_context_menu.eject_button = "Undock".to_owned();

        
        let [_a, _b] = tree
            .main_surface_mut()
            .split_left(
                NodeIndex::root(),
                0.10, 
                vec![
                    "Tasks".to_owned(),
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
            vec!["Web Console".to_owned()],
        );

        // let [_, _] = tree
        //     .main_surface_mut()
        //     .split_left(
        //     b,
        //     0.20,
        //     vec!["Scripts".to_owned()],
        // );


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


        let (db_tx, db_rx) = channel::unbounded();
        let (db_data_tx, db_data_rx) = channel::unbounded::<Vec<TaskPayload>>();

        let ctx = cc.egui_ctx.clone();
        let data_update = Rc::new(std::cell::Cell::new(None));
        let sender = data_update.clone();
        let bridge = <WebWorker as Spawnable>::spawner()
            .callback(move |response| {
                sender.set(Some(response.0));
                ctx.request_repaint();
            })
            .spawn("./dummy_worker.js");

        setup_custom_fonts(&cc.egui_ctx);

        let context = MtechServerContext{
            open_tabs,
            style: None,
            
            terminal,
            chart_app,
            tick_rate,
            last_tick,

            first_run: true,
            database: None,
            task_data: None,
            db_tx, 
            db_rx,
            db_data_tx, 
            db_data_rx,

            bridge: Some(bridge),
            data_update: Some(data_update),
        };
        
        Self {
            context,
            tree,
        }
    }
        // Load app state. Note that you must enable the `persistence` feature for this to work.
        // if let Some(storage) = cc.storage {return eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();}
        
        // MtechServer::new(cc)
    // }
    
    fn canvas_id() -> String { "mtech_canvas".into() }
}

fn setup_custom_fonts(ctx: &egui::Context) {
    // Start with the default fonts (we will be adding to them rather than replacing them).
    let mut fonts = egui::FontDefinitions::default();

    // Install my own font (maybe supporting non-latin characters).
    // .ttf and .otf files supported.
    fonts.font_data.insert(
        "Regular".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/Iosevka-Regular.ttf")),
    );
    fonts.families.insert(
        egui::FontFamily::Name("Regular".into()),
        vec!["Regular".to_owned()],
    );
    fonts.font_data.insert(
        "Bold".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/Iosevka-Bold.ttf")),
    );
    fonts.families.insert(
        egui::FontFamily::Name("Bold".into()),
        vec!["Bold".to_owned()],
    );

    fonts.font_data.insert(
        "Oblique".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/Iosevka-Oblique.ttf")),
    );
    fonts.families.insert(
        egui::FontFamily::Name("Oblique".into()),
        vec!["Oblique".to_owned()],
    );

    fonts.font_data.insert(
        "BoldOblique".to_owned(),
        egui::FontData::from_static(include_bytes!(
            "../assets/fonts/Iosevka-BoldOblique.ttf"
        )),
    );
    fonts.families.insert(
        egui::FontFamily::Name("BoldOblique".into()),
        vec!["BoldOblique".to_owned()],
    );

    // Tell egui to use these fonts:
    ctx.set_fonts(fonts);
}