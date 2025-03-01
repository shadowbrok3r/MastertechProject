use ratatui::{crossterm::event::MouseEvent, layout::{Constraint, Direction, Layout, Rect}, prelude::Backend, style::{Style, Stylize}, text::{Line, Span}, widgets::{Block, BorderType, Borders, List, ListItem, Paragraph, Wrap}, Frame};
use crate::terminal_mode::{styling::{BASE_COLORS, CATPPUCCIN, CYAN, DARKORANGE}, widgets::{ButtonType, HandleWidget, ShrinkArea}};

use super::{checklist::Status, ScriptsTab, ScriptsTabView};

#[derive(Clone, Debug)]
pub struct Report {
    pub reporter: Reporter,
    pub msg: String,
}

#[derive(Clone, Debug)]
pub enum Reporter {
    Tuneup,
    Qc,
    WindowsUpdates,
    GetAntivirus,
    GetInstalledPrograms,
    GetStartupItems,
    GetScheduledTasks,
    GetTaskbarItems,
    RunPrechecks,
    Unknown
}

impl<'a> ScriptsTab<'a> {
    fn draw_antivirus<B: Backend>(&self, f: &mut Frame, area: Rect) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(3), Constraint::Min(0)])
            .split(area);

        f.render_widget(&self.get_antivirus_btn, layout[0]);

        let antivirus_data: Vec<Line> = self.antivirus_products.iter().map(|av| {
            Line::from(vec![
                Span::styled("Name: ", Style::default().fg(CYAN.text)),
                Span::raw(&av.display_name),
                Span::styled(" | State: ", Style::default().fg(DARKORANGE.text)),
                Span::raw(av.product_state.to_string()),
            ])
        }).collect();

        let paragraph = Paragraph::new(antivirus_data)
            .block(Block::default().borders(Borders::ALL).title("Antivirus Info").border_type(BorderType::Rounded))
            .wrap(Wrap { trim: true });

        f.render_widget(paragraph, layout[1]);
    }

    fn draw_installed_programs<B: Backend>(&self, f: &mut Frame, area: Rect) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(3), Constraint::Min(0)])
            .split(area);

        f.render_widget(&self.get_installed_programs_btn, layout[0]);

        let programs_data: Vec<Line> = self.installed_programs.iter().map(|prog| {
            Line::from(vec![
                Span::styled("Name: ", Style::default().fg(CYAN.text)),
                Span::raw(prog.display_name.clone().unwrap_or_default()),
                Span::styled(" | Version: ", Style::default().fg(DARKORANGE.text)),
                Span::raw(prog.display_version.clone().unwrap_or_default()),
            ])
        }).collect();

        let paragraph = Paragraph::new(programs_data)
            .block(Block::default().borders(Borders::ALL).title("Installed Programs").border_type(BorderType::Rounded))
            .wrap(Wrap { trim: true });

        f.render_widget(paragraph, layout[1]);
    }

    fn draw_startup_items<B: Backend>(&self, f: &mut Frame, area: Rect) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(3), Constraint::Min(0)])
            .split(area);

        f.render_widget(&self.get_startup_items_btn, layout[0]);

        let startup_data: Vec<Line> = self.startup_programs.iter().map(|item| {
            Line::from(vec![
                Span::styled("Key: ", Style::default().fg(CYAN.text)),
                Span::raw(&item.key_name),
                Span::styled(" | State: ", Style::default().fg(DARKORANGE.text)),
                Span::raw(format!("{:?}", item.decoded_state)),
            ])
        }).collect();

        let paragraph = Paragraph::new(startup_data)
            .block(Block::default().borders(Borders::ALL).title("Startup Items").border_type(BorderType::Rounded))
            .wrap(Wrap { trim: true });

        f.render_widget(paragraph, layout[1]);
    }

    fn draw_scheduled_tasks<B: Backend>(&self, f: &mut Frame, area: Rect) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(3), Constraint::Min(0)])
            .split(area);

        f.render_widget(&self.get_scheduled_tasks_btn, layout[0]);

        let task_data: Vec<Line> = self.scheduled_tasks.iter().map(|task| {
            Line::from(vec![
                Span::styled("Task Name: ", Style::default().fg(CYAN.text)),
                Span::raw(task.task_name.clone().unwrap_or_default()),
                Span::styled(" | State: ", Style::default().fg(DARKORANGE.text)),
                Span::raw(format!("{:?}", task.state)),
            ])
        }).collect();

        let paragraph = Paragraph::new(task_data)
            .block(Block::default().borders(Borders::ALL).title("Scheduled Tasks").border_type(BorderType::Rounded))
            .wrap(Wrap { trim: true });

        f.render_widget(paragraph, layout[1]);
    }

    fn draw_taskbar_items<B: Backend>(&self, f: &mut Frame, area: Rect) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(3), Constraint::Min(0)])
            .split(area);

        f.render_widget(&self.get_taskbar_items_btn, layout[0]);

        let taskbar_data: Vec<Line> = self.taskbar_items.iter().map(|item| {
            Line::from(vec![
                Span::styled("Name: ", Style::default().fg(CYAN.text)),
                Span::raw(&item.name),
                Span::styled(" | Path: ", Style::default().fg(DARKORANGE.text)),
                Span::raw(&item.path),
            ])
        }).collect();

        let paragraph = Paragraph::new(taskbar_data)
            .block(Block::default().borders(Borders::ALL).title("Taskbar Items").border_type(BorderType::Rounded))
            .wrap(Wrap { trim: true });

        f.render_widget(paragraph, layout[1]);
    }

    fn draw_log_section<B: Backend>(&self, f: &mut Frame, area: Rect) {
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),  // Checklist 
                Constraint::Percentage(75),  // Log Messages
            ])
            .split(area);

        // 📝 Render checklists
        let checklist_items: Vec<ListItem> = self.checklists.values().flat_map(|list| {
            let header = ListItem::new(Line::styled(format!("📌 {}", list.name), Style::default().fg(CATPPUCCIN.sapphire).bold()));
            let tasks = list.items.iter().map(|item| {
                let symbol = match item.status {
                    Status::Completed => "✓",
                    Status::Todo => "☐",
                };
                let style = match item.status {
                    Status::Completed => Style::default().fg(CATPPUCCIN.teal),
                    Status::Todo => Style::default().fg(CATPPUCCIN.pink),
                };
                ListItem::new(Line::styled(format!("{} {}", symbol, item.text), style))
            });
    
            std::iter::once(header).chain(tasks).collect::<Vec<_>>()
        }).collect();
    
        let checklist = List::new(checklist_items)
            .block(Block::default().borders(Borders::ALL).title("Checklist").border_type(BorderType::Rounded).border_type(BorderType::Rounded));
    
        f.render_widget(checklist, layout[0]);
    
        // 📝 Render logs
        let log_text: Vec<Line> = self.reports.borrow().iter().enumerate().map(|(index, r)| {
            let color = BASE_COLORS[index % BASE_COLORS.len()];
            Line::from(vec![
                Span::styled(format!("{:?} => {}", r.reporter, r.msg), Style::default().fg(color))
            ])
        }).collect();
    
        let log_widget = Paragraph::new(log_text)
            .block(Block::default().borders(Borders::ALL).title("Run Report").border_type(BorderType::Rounded))
            .wrap(Wrap { trim: false });
    
        f.render_widget(log_widget, layout[1]);
    }
    
    
}


impl<'a> HandleWidget<'_> for ScriptsTab<'_> {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        self.receive();
        self.update_selected_tab();
    
        // Define layout sections
        let main_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Tab row
                Constraint::Percentage(98), // Content
            ])
            .split(area);
    
        let tab_row = main_layout[0]; // Tab buttons
        let content_area = main_layout[1]; // Dynamic content based on selected tab
    
        // Layout for tab buttons
        let tab_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Ratio(1, self.tab_buttons.len() as u32); self.tab_buttons.len()])
            .split(tab_row);
    
        // Render tab buttons in order
        for (i, (_, button)) in self.tab_buttons.iter().enumerate() {
            f.render_widget(button, tab_layout[i]);
        }
    
        // Display content based on selected tab
        match *self.current_tab.borrow() {
            ScriptsTabView::Main => {
                // Split area into buttons (left) and logs (right)
                let main_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Percentage(30), // Left: Buttons
                        Constraint::Percentage(70), // Right: Logs
                    ])
                    .split(content_area);
    
                let left_half = main_chunks[0];
                let right_half = main_chunks[1];
    
                // Create grid layout for buttons
                let button_grid = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints(vec![Constraint::Ratio(1, 8); 8])
                    .split(left_half);
    
                let button_row1 = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(button_grid[0]);
    
                let button_row2 = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(button_grid[1]);
    
                // Render main buttons
                f.render_widget(&self.tuneup_btn, button_row1[0].shrink(5, 1));
                f.render_widget(&self.qc_btn, button_row1[1].shrink(5, 1));
                f.render_widget(&self.updates_btn, button_row2[0].shrink(5, 1));
                f.render_widget(&self.prechecks_btn, button_row2[1].shrink(5, 1));
                // Render log section
                self.draw_log_section::<B>(f, right_half);
            }
            ScriptsTabView::Antivirus => self.draw_antivirus::<B>(f, content_area),
            ScriptsTabView::StartupItems => self.draw_startup_items::<B>(f, content_area),
            ScriptsTabView::InstalledPrograms => self.draw_installed_programs::<B>(f, content_area),
            ScriptsTabView::ScheduledTasks => self.draw_scheduled_tasks::<B>(f, content_area),
            ScriptsTabView::TaskbarItems => self.draw_taskbar_items::<B>(f, content_area),
        }
    }
    

    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        self.tuneup_btn.handle_mouse_event(&mouse_event);
        self.qc_btn.handle_mouse_event(&mouse_event);
        self.prechecks_btn.handle_mouse_event(&mouse_event);
        self.updates_btn.handle_mouse_event(&mouse_event);
        self.get_antivirus_btn.handle_mouse_event(&mouse_event);
        self.get_installed_programs_btn.handle_mouse_event(&mouse_event);
        self.get_startup_items_btn.handle_mouse_event(&mouse_event);
        self.get_scheduled_tasks_btn.handle_mouse_event(&mouse_event);
        self.get_taskbar_items_btn.handle_mouse_event(&mouse_event);
        for (_id, button) in self.tab_buttons.iter() {
            button.handle_mouse_event(&mouse_event);
        }
    }
}


// impl<'a> HandleWidget<'_> for ScriptsTab<'_> {
//     /// Draw the ScriptsTab with buttons on the left and a log area on the right
//     fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
//         self.handle_log_events();
        
//         // Split the total area into two halves: left (buttons) and right (logs)
//         let main_chunks = Layout::default()
//             .direction(Direction::Horizontal)
//             .constraints([
//                 Constraint::Percentage(50), // Left: Buttons
//                 Constraint::Percentage(50), // Right: Logs
//             ])
//             .split(area);

//         let left_half = main_chunks[0]; // Area for buttons
//         let right_half = main_chunks[1]; // Area for log output



//             // Collect logs into a formatted paragraph
//         let log_text: Vec<Line> = self.reports.borrow().iter().map(|r| {
//             let color = match r.reporter {
//                 Reporter::Tuneup => CATPPUCCIN.teal,
//                 Reporter::Qc => DEEPPINK.text,
//                 Reporter::WindowsUpdates => DARKORANGE.text,
//                 Reporter::Unknown => CATPPUCCIN.red,
//             };
//             Line::from(vec![Span::styled(format!("{:?} => {}", r.reporter, r.msg), Style::default().fg(color))])
//         }).collect();

//         let log_widget = Paragraph::new(log_text)
//             .block(Block::default()
//                 .borders(Borders::ALL)
//                 .border_set(SHORTCUT_SET)
//                 .title("Run Report"))
//             .style(Style::default().fg(CATPPUCCIN.peach))
//             .wrap(Wrap { trim: false });

//         f.render_widget(log_widget, right_half);
//     }

//     /// Handle a mouse event, see if it hits our buttons
//     fn handle_mouse_event(&self, mouse_event: &MouseEvent) {

//     }
// }