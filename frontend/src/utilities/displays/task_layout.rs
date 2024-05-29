use database::Database;
use eframe::egui::Ui;
use egui::{Color32, Stroke};
use database::schema::{Priority, TaskPayload, User};
use serde::Serialize;

use super::create_task::CreateTaskModal;
use super::task_modal::TaskModal;
use super::{ColumnLayout, Filters, Sortable};

use crate::utilities::modal::Modal;


#[derive(Serialize)]
pub struct TaskLayout{
    pub task_opts: Option<TaskLayoutOpts>,
    pub create_task_modal: bool
    // ui: &mut Ui,
}

#[derive(Serialize)]
pub struct TaskLayoutOpts{
    pub tasks: Vec<TaskPayload>,
    pub style_options: TaskStyles,
    pub filters: Vec<Filters>,
    pub column_names: Vec<String>,
    #[serde(skip)]
    pub database: Database,
    
    pub task_modal: TaskModal,
    pub create_task_modal: CreateTaskModal,
    pub modal: Modal,
}

impl TaskLayoutOpts{
    pub fn new(
        tasks: Vec<TaskPayload>, 
        filters: Vec<Filters>,
        column_names: Vec<String>,
        database: Database,
        // ui: &mut Ui,
    ) -> Self {    
        Self { 
            tasks,
            style_options: TaskStyles::default(),
            filters,
            column_names,
            database,
            task_modal: TaskModal::default(),
            create_task_modal: CreateTaskModal::new(),
            modal: Modal::new("Test"),
        }
    }
}

impl Default for TaskLayout{
    fn default() -> Self {
        Self {
            create_task_modal: false,
            task_opts: None
        }
    }
}

impl TaskLayout{
    pub fn new(
        tasks: Vec<TaskPayload>, 
        filters: Vec<Filters>,
        column_names: Vec<String>,
        database: Database,
        create_task_modal: bool,
        // ui: &mut Ui,
    ) -> Self {
        let task_opts = TaskLayoutOpts{
            tasks,
            style_options: TaskStyles::default(),
            filters,
            column_names,
            database,
            task_modal: TaskModal::default(),
            create_task_modal: CreateTaskModal::new(),
            modal: Modal::new("Test"),
        };
        
        Self { 
            task_opts: Some(task_opts),
            create_task_modal,
            // ui,
        }
    }

    pub fn display(
        &mut self,
        ui: &mut Ui,
        store_users: &Option<Vec<User>>,
        status: bool,
        priority: &Option<Priority>,
        complete: &Option<bool>,
        current_user: &Option<User> 
    ){
        if let Some(task_opts) = &mut self.task_opts{
            let col_names = task_opts.column_names.clone();
            let db = task_opts.database.clone();
            let filters = &task_opts.filters.clone();
            
            task_opts.tasks.sort_task_payloads();
            
            self.setup_display(
                ui, 
                col_names, 
                db, 
                filters, 
                &store_users,
                status,
                &priority,
                &complete,
                &current_user
            );
        }
    }

    pub fn set_styles(&mut self, ui: &mut Ui){
        if let Some(task_opts) = &self.task_opts{

            ui.style_mut().visuals.selection.stroke.color = task_opts.style_options.selection_stroke_color;
            ui.style_mut().visuals.selection.bg_fill = task_opts.style_options.selection_bg_fill;
            
            ui.style_mut().visuals.widgets.inactive.bg_fill = task_opts.style_options.widgets_inactive_bg_fill;
            ui.style_mut().visuals.widgets.inactive.fg_stroke = task_opts.style_options.widgets_inactive_fg_stroke;
            ui.style_mut().visuals.widgets.inactive.weak_bg_fill = task_opts.style_options.widgets_inactive_weak_bg_fill;
            ui.style_mut().visuals.widgets.inactive.bg_stroke = task_opts.style_options.widgets_inactive_bg_stroke;
            
            ui.style_mut().visuals.widgets.open.bg_fill = task_opts.style_options.widgets_open_bg_fill;
            ui.style_mut().visuals.widgets.open.weak_bg_fill = task_opts.style_options.widgets_open_weak_bg_fill;
            
            ui.style_mut().visuals.widgets.active.weak_bg_fill = task_opts.style_options.widgets_active_weak_bg_fill;
            
            ui.style_mut().visuals.widgets.hovered.weak_bg_fill = task_opts.style_options.widgets_hovered_weak_bg_fill;
            ui.style_mut().visuals.widgets.hovered.bg_fill = task_opts.style_options.widgets_hovered_bg_fill;
            ui.style_mut().visuals.widgets.hovered.bg_stroke = task_opts.style_options.widgets_hovered_bg_stroke;
            
            ui.style_mut().visuals.widgets.hovered.expansion = 2.0;
        }
    }
}

#[derive(Serialize)]
pub struct TaskStyles{
    selection_stroke_color:  Color32, //  = Color32::BLACK,
    selection_bg_fill: Color32, //  Color32::from_rgb(120, 10, 120),
    widgets_inactive_bg_fill:  Color32, //  = Color32::GOLD,
    widgets_inactive_fg_stroke:  Stroke, //  = Stroke::new(1.0, Color32::WHITE),
    widgets_inactive_weak_bg_fill:  Color32, //  = Color32::from_rgb(20, 20, 25),
    widgets_inactive_bg_stroke:  Stroke, //  = Stroke::new(1.0, Color32::from_rgb(80, 80, 80)),
    widgets_open_bg_fill:  Color32, //  = Color32::from_black_alpha(50),
    widgets_open_weak_bg_fill:  Color32, //  = Color32::from_black_alpha(50),
    widgets_active_weak_bg_fill:  Color32, //  = Color32::from_rgb(30,30,30),
    widgets_hovered_weak_bg_fill:  Color32, //  = Color32::TRANSPARENT,
    widgets_hovered_bg_fill:  Color32, //  = Color32::from_rgb(12, 12, 12),
    widgets_hovered_bg_stroke:  Stroke, //  = Stroke::new(1.0, Color32::from_rgb(200, 20, 200)),
}

impl Default for TaskStyles{
    fn default() -> Self {
        Self { 
            selection_stroke_color:  Color32::BLACK,
            selection_bg_fill: Color32::from_rgb(120, 10, 120),
            widgets_inactive_bg_fill:  Color32::GOLD,
            widgets_inactive_fg_stroke:  Stroke::new(1.0, Color32::WHITE),
            widgets_inactive_weak_bg_fill:  Color32::from_rgb(20, 20, 25),
            widgets_inactive_bg_stroke:  Stroke::new(1.0, Color32::from_rgb(80, 80, 80)),
            widgets_open_bg_fill:  Color32::from_black_alpha(50),
            widgets_open_weak_bg_fill:  Color32::from_black_alpha(50),
            widgets_active_weak_bg_fill:  Color32::from_rgb(30,30,30),
            widgets_hovered_weak_bg_fill:  Color32::TRANSPARENT,
            widgets_hovered_bg_fill:  Color32::from_rgb(12, 12, 12),
            widgets_hovered_bg_stroke:  Stroke::new(1.0, Color32::from_rgb(200, 20, 200)),
        }
    }
}




// impl ColumnLayout for TaskLayout {
//     fn setup_display(
//         &mut self,
//         ui: &mut egui::Ui, 
//         column_names: Vec<String>, 
//         database: Database,
//         filters: &Vec<Filters>, 
//         assignees: &Option<Vec<User>>,
//         status: bool,
//         priority: &Option<Priority>,
//         complete: &Option<bool>,
//         current_user: &Option<User>
//     ) {
//         ui.style_mut().visuals.window_rounding = Rounding::same(5.0);
//         let header_frame = Frame::default()
//             .fill(Color32::from_rgb(20, 20, 25))
//             .inner_margin(Margin::same(4.0))
//             .outer_margin(Margin::symmetric(4.0, 1.0))
//             .rounding(Rounding::same(5.0))
//             .stroke(Stroke::new(1.0, Color32::from_additive_luminance(50)));
    
//         let column_frame = Frame::default()
//             .fill(Color32::from_rgb(15, 15, 19))
//             .inner_margin(Margin::same(8.0))
//             .rounding(Rounding::same(10.0))
//             .stroke(Stroke::new(1.0, Color32::from_additive_luminance(50)));
    
//         let column_width = Size::exact(450.0);
    
//         ScrollArea::horizontal()
//             .hscroll(true)
//             .show_viewport(ui, |ui, _|
//         {
//             StripBuilder::new(ui)
//                 .cell_layout(Layout::top_down_justified(egui::Align::Center))
//                 .size(Size::relative(0.01))
//                 .size(Size::relative(0.07))
//                 .size(Size::relative(0.92))
//                 .vertical(|mut strip| 
//             {
//                 strip
//                     .strip(|strip| 
//                 {
//                     strip
//                         .sizes(column_width, column_names.len())
//                         .horizontal( |s| 
//                     {
//                         let create_task = task_headers(s, column_names.clone(), header_frame);

//                         if let Some(response) = create_task{
//                             if response.clicked(){
//                                 info!("Creating task!");
//                                 self.open_modal = true;
//                             }
//                         }
//                     });
//                 });
//                 strip.empty();
//                 strip
//                     .strip(|strip| 
//                 {
//                     strip
//                         .sizes(column_width, column_names.len())
//                         .horizontal( |s| 
//                     {
//                         self.task_columns(     
//                             s,           
//                             filters,
//                             &assignees,
//                             status,
//                             &priority,
//                             &complete,
//                             current_user,
//                             database,
//                             column_frame
//                         );
//                     });
//                 });
//             });
//         });
//     }
//     fn task_columns(
//         &mut self,
//         mut s: Strip, 
//         filters: &Vec<Filters>, 
//         assignees: &Option<Vec<User>>,
//         status: bool,
//         priority: &Option<Priority>,
//         complete: &Option<bool>,
//         current_user: &Option<User>,
//         database: Database,
//         column_frame: Frame,
//     ){
//         if let Some(_) = current_user {
//             if status{
//                 for status in Status::VALUES{
//                     let mut filtered = self.filter_items(
//                         &filters,&None,&Some(status),&priority,&complete
//                     );
    
                    
//                     s.cell(|ui| {
//                         column_frame.show(ui, |ui| {
//                             ui.vertical_centered_justified(|ui| {
//                                 ScrollArea::vertical()
//                                 .auto_shrink(false)
//                                 .show_viewport(ui, |ui, _| {
//                                     for task in filtered.iter_mut() {
//                                         if let Some(store_users) = &assignees{
//                                             task.display_task_cards(ui, database.clone(), &store_users.as_ref()).unwrap();
//                                         }
                                        
//                                     }
//                                 });
//                             });
//                         });
//                     });
//                 }
//             }else{
//                 let mut filtered = self.filter_items(
//                     &filters,&None,&None,&priority,&complete
//                 );
                
//                 s.cell(|ui| {
//                     column_frame.show(ui, |ui| {
//                         ui.vertical_centered_justified(|ui| {
//                             ScrollArea::vertical()
//                             .auto_shrink(false)
//                             .show_viewport(ui, |ui, _| {
//                                 for task in filtered.iter_mut() {
//                                     task.display_task_cards(ui, database.clone(), &assignees.as_ref().unwrap()).unwrap();
//                                 }
//                             });
//                         });
//                     });
//                 });
//             }
//         } else if let Some(users) = assignees {
//             // Multiple users, iterate over each user
//             for user in users.iter() {
//                 let mut user_filters = filters.clone();
//                 user_filters.push(Filters::FilterAssignee);
    
//                 let mut filtered = self.filter_items(
//                     &user_filters,
//                     &Some(user.clone()),
//                     &None,
//                     &priority,
//                     &complete,
//                 );
    
//                 s.cell(|ui| {
//                     column_frame.show(ui, |ui| {
//                         ui.vertical_centered_justified(|ui| {
//                             ScrollArea::vertical()
//                                 .auto_shrink(false)
//                                 .show_viewport(ui, |ui, _| 
//                             {
//                                 for task in filtered.iter_mut() {
//                                     task.display_task_cards(ui, database.clone(), &assignees.as_ref().unwrap()).unwrap();
//                                 }
//                             });
//                         });
//                     });
//                 });
//             }
//         }
//     }
    
//     fn filter_items(
//         &mut self,
//         filters: &Vec<Filters>, 
//         assignee: &Option<User>,
//         status: &Option<Status>,
//         priority: &Option<Priority>,
//         complete: &Option<bool>,
//     ) -> Vec<TaskPayload>{
        
//         filters.into_iter().fold(self.tasks.to_owned(), |acc_tasks, filter| {
//             match filter {
//                 Filters::FilterAssignee => {
//                     if let Some(ref user) = assignee {
//                         acc_tasks.filter_by_assignee(user)
//                     } else {
//                         acc_tasks
//                     }
//                 },
//                 Filters::FilterCompleted => {
//                     if let Some(complete) = complete {
//                         acc_tasks.filter_by_completed(*complete)
//                     } else {
//                         acc_tasks
//                     }
//                 },
//                 Filters::FilterStatus => {
//                     if let Some(status) = status {
//                         acc_tasks.filter_by_status(&status)
//                     } else {
//                         acc_tasks
//                     }
//                 },
//                 Filters::FilterPriority => {
//                     if let Some(ref priority) = priority {
//                         acc_tasks.filter_by_priority(&priority)
//                     } else {
//                         acc_tasks
//                     }
//                 },
//             }
//         })
//     }
// }


// pub fn task_headers(
//     mut s: Strip,
//     column_names: Vec<String>,
//     header_frame: Frame,
// ) -> Option<Response>{
//     let mut response: Option<Response> = None;
//     for name in &column_names{
//         s.cell(|ui|{
//             header_frame.show(ui, |ui|
//             {
//                 ui.horizontal_top(|ui| 
//                 {
//                     ui.with_layout(Layout::left_to_right(egui::Align::Center), |ui| 
//                     {
//                         ui.vertical_centered(|ui|{
//                             ui.colored_label(Color32::WHITE, RichText::new(name.clone()).heading());
//                         });
//                     });

//                     ui.with_layout(Layout::right_to_left(egui::Align::Max), |ui| 
//                     {
//                         response = Some(
//                             Button::new(
//                             RichText::new("✚")
//                                 .raised()
//                                 .color(Color32::LIGHT_RED)
//                             )
//                             .fill(Color32::TRANSPARENT)
//                             .ui(ui)
//                         );
//                         // info!("clicked + button");
//                     });
//                 });
//             });
//         });
//     }
//     match response{
//         Some(res) => Some(res),
//         None => None,
//     }
// }