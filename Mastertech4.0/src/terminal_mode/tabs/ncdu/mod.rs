// TODO: Catch errors when scanning and display them to the user, then continue
// TODO: Display scanning animation when refreshing too
// TODO: Allow specifying a command to print the size of a file instead of using disk usage
// TODO: Add an argument parser to handle invalid input better
use std::{collections::BTreeMap,ffi::OsString,path::PathBuf,sync::{Arc, Mutex},thread};
use path_info::{get_starting_dir, get_wrapped_contents, PathInfo};
use crate::terminal_mode::context::TerminalContext;
use ratatui::widgets::ListState;

pub mod render;
pub mod path_info;
pub mod input;
mod file_explorer;
mod widget;

pub use file_explorer::{File, FileExplorer};
pub use input::Input;
pub use widget::Theme;

pub struct NcduTab {
    ctx: Arc<Mutex<TerminalContext>>,
    starting_dir: Arc<Mutex<PathBuf>>,
    state: Arc<Mutex<ListState>>,
    contents: Arc<Mutex<PathInfo>>,
    current_dir: Arc<Mutex<Vec<OsString>>>,
}
/*
use std::{
    borrow::Cow,
    fs::read_to_string,
    io::{self, stdout},
};

use crossterm::{
    event::{read, Event, KeyCode},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{prelude::*, widgets::*};

use ratatui_explorer::{File, FileExplorer, Theme};

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;

    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    let layout = Layout::horizontal([Constraint::Ratio(1, 3), Constraint::Ratio(2, 3)]);

    // Create a new file explorer with the default theme and title.
    let theme = get_theme();
    let mut file_explorer = FileExplorer::with_theme(theme)?;

    loop {
        // Get the content of the current selected file (if it's indeed a file).
        let file_content = get_file_content(file_explorer.current());

        let file_content = match file_content {
            Ok(file_content) => file_content,
            _ => "Couldn't load file.".into(),
        };

        // Render the file explorer widget and the file content.
        terminal.draw(|f| {
            let chunks = layout.split(f.area());

            f.render_widget(&file_explorer.widget(), chunks[0]);
            f.render_widget(Clear, chunks[1]);
            f.render_widget(
                Paragraph::new(file_content).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Double),
                ),
                chunks[1],
            );
        })?;

        // Read the next event from the terminal.
        let event = read()?;
        if let Event::Key(key) = event {
            if key.code == KeyCode::Char('q') {
                break;
            }
        }
        // Handle the event in the file explorer.
        file_explorer.handle(&event)?;
    }

    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}

fn get_file_content(file: &File) -> io::Result<Cow<'_, str>> {
    // If the path is a file, read its content.
    if file.is_file() {
        read_to_string(file.path()).map(Into::into)
    } else if file.is_dir() {
        Ok("".into())
    } else {
        Ok("<not a regular file>".into())
    }
}

fn get_theme() -> Theme {
    Theme::default()
        .with_block(Block::default().borders(Borders::ALL))
        .with_dir_style(
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .with_highlight_dir_style(
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
                .bg(Color::DarkGray),
        )
        .with_scroll_padding(1)
}
*/
impl NcduTab {
    pub fn new(ctx: Arc<Mutex<TerminalContext>>) -> Self {
        let starting_dir: Arc<Mutex<PathBuf>> = match get_starting_dir() {
            Ok(dir) => Arc::new(Mutex::new(dir)),
            Err(e) => panic!("{}", e),
        };
    
        let state: Arc<Mutex<ListState>> = Arc::new(Mutex::new(ListState::default()));
        state.lock().unwrap().select(Some(0));
    
        // let (tx, rx) = std::sync::mpsc::channel();
    
        let contents: Arc<Mutex<PathInfo>> = Arc::new(Mutex::new(PathInfo::Folder(0, BTreeMap::new(), 0)));
        let contents_clone: Arc<Mutex<PathInfo>> = Arc::clone(&contents);
        let dir: Vec<OsString> = vec![];
        let current_dir: Arc<Mutex<Vec<OsString>>> = Arc::new(Mutex::new(dir));
        let starting_dir_clone: Arc<Mutex<PathBuf>> = Arc::clone(&starting_dir);
        // thread::spawn(move || {
        //     *contents_clone.lock().unwrap() = get_wrapped_contents(&starting_dir_clone.lock().unwrap());
        //     tx.send(0).unwrap();
        // });
    
        // let mut dot_pos = 0;
        // let mut dot_fwd = true;


        Self { 
            ctx,
            state,
            starting_dir,
            contents,
            current_dir,
        }
    }

    pub fn receive(&mut self) {
        // match self.rx.try_recv() {
        //     Ok(_) => {},
        //     Err(_) => {}
        // }
    }
}