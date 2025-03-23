
use ratatui::{crossterm::event::KeyCode, layout::{Alignment, Constraint, Direction, Layout}, prelude::*, style::{Color, Modifier, Style}, text::Span, widgets::{Block, Borders, List, ListItem, ListState, Paragraph}, Terminal};
use crate::terminal_mode::widgets::HandleWidget;
use std::{ffi::OsString, sync::Arc};
use super::{path_info::{get_wrapped_contents, join_path_to_vec}, NcduTab, PathInfo};

impl<'a> HandleWidget <'a> for NcduTab {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                [
                    Constraint::Length(3),
                    Constraint::Length((f.size().height - 6) / 2),
                    Constraint::Length(3),
                ]
                .as_ref(),
            )
            .split(f.size());
        let display_dir_string = String::from(starting_dir_copy.to_string_lossy());
        let block = Paragraph::new(display_dir_string)
            .block(Block::default().title(" rsdu ").borders(Borders::ALL));
        f.render_widget(block, chunks[0]);
        let blank1 = Block::default();
        f.render_widget(blank1, chunks[1]);
        let msg = Paragraph::new(
            "Scanning".to_string()
                + &" ".repeat(dot_pos)
                + "..."
                + &" ".repeat(6 - dot_pos),
        )
        .alignment(Alignment::Center)
        .block(Block::default());
        f.render_widget(msg, chunks[2]);
            //////////////// DO THIS OUTSIDE THE DRAW 
        if dot_pos == 6 {
            dot_fwd = false;
        } else if dot_pos == 0 {
            dot_fwd = true;
        }
        if dot_fwd {
            dot_pos += 1;
        } else {
            dot_pos -= 1;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)].as_ref())
            .split(f.size());
        let mut items: Vec<ListItem> = vec![];
        let mut contents_access = contents_clone.lock().unwrap();
        let joined_contents = contents_access
            .join(&*current_dir_clone.lock().unwrap())
            .unwrap();
        let display_dir = join_path_to_vec(
            &*starting_dir_clone.lock().unwrap(),
            (current_dir_clone.lock().unwrap()).clone(),
        )
        .canonicalize()
        .unwrap();
        let display_dir_string = String::from(display_dir.to_string_lossy());
        let block = Paragraph::new(display_dir_string)
            .block(Block::default().title(" rsdu ").borders(Borders::ALL));
        f.render_widget(block, chunks[0]);

        for (path, info) in joined_contents.sorted().unwrap() {
            items.push(ListItem::new(Spans::from(Span::raw(
                String::from(pad_and_prettify_bytes(&info.size()))
                    + &size_bar(&info.size(), &joined_contents.size())
                    + &path.as_os_str().to_string_lossy()
                    + match info {
                        PathInfo::Folder(..) => "/",
                        PathInfo::File(..) => "",
                    },
            ))));
        }
        let paths = List::new(items)
            .block(Block::default().borders(Borders::ALL))
            .highlight_style(
                Style::default()
                    .fg(Color::Blue)
                    .add_modifier(Modifier::BOLD),
            );
        f.render_stateful_widget(paths, chunks[1], &mut state_clone.lock().unwrap());
    }

    fn handle_key_event(&mut self, key_event: ratatui::crossterm::event::KeyEvent) -> bool {
        let contents_clone = Arc::clone(&self.contents);
        let current_dir_clone = Arc::clone(&self.current_dir);
        let starting_dir_clone = Arc::clone(&self.starting_dir);
        let state_clone = Arc::clone(&self.state);

        match key_event.code {
            // TODO: implement deletion with confirmation
            // TODO: implement trashing with the give `trash` command found on the shell's path
            // TODO: implement selection and application of deletion and trashing commands
            // to all selected files
            KeyCode::Char('q') => {},
            KeyCode::Char('j') | KeyCode::Down => {
                let dir_len = (&contents_clone)
                    .lock()
                    .unwrap()
                    .join(&*current_dir_clone.lock().unwrap())
                    .unwrap()
                    .contents()
                    .unwrap()
                    .len();
                if dir_len != 0 {
                    let new_state =
                        (((state_clone.lock().unwrap().selected().unwrap() as isize) + 1)
                            .max(0) as usize)
                            .min(dir_len - 1);
                    state_clone.lock().unwrap().select(Some(new_state));
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let dir_len = (&contents_clone)
                    .lock()
                    .unwrap()
                    .join(&*current_dir_clone.lock().unwrap())
                    .unwrap()
                    .contents()
                    .unwrap()
                    .len();
                if dir_len != 0 {
                    let new_state =
                        (((state_clone.lock().unwrap().selected().unwrap() as isize) - 1)
                            .max(0) as usize)
                            .min(dir_len - 1);
                    state_clone.lock().unwrap().select(Some(new_state));
                }
            }
            KeyCode::Char('l') | KeyCode::Right => {
                let mut drawn_dir_access = current_dir_clone.lock().unwrap();
                let mut contents_access = (&contents_clone).lock().unwrap();
                let mut joined = contents_access.join(&*drawn_dir_access).unwrap();
                match joined {
                    PathInfo::Folder(.., ref mut s) => {
                        *s = state_clone.lock().unwrap().selected().unwrap();
                    }
                    PathInfo::File(..) => panic!(),
                }
                let sorted = joined.sorted().unwrap();
                let (target_os_string, info) = sorted
                    .iter()
                    .nth(state_clone.lock().unwrap().selected().unwrap())
                    .unwrap();
                match info {
                    PathInfo::Folder(..) => {
                        (*drawn_dir_access).push(OsString::from(target_os_string));
                        joined = contents_access.join(&*drawn_dir_access).unwrap();
                        state_clone.lock().unwrap().select(match joined {
                            PathInfo::Folder(_, _, s) => {
                                let new_state = *s as usize;
                                Some(new_state)
                            }
                            PathInfo::File(..) => panic!(),
                        });
                    }
                    PathInfo::File(..) => {}
                }
            }
            KeyCode::Char('h') | KeyCode::Left => {
                let mut drawn_dir_access = current_dir_clone.lock().unwrap();
                let mut contents_access = (&contents_clone).lock().unwrap();
                let mut joined = contents_access.join(&*drawn_dir_access).unwrap();
                match joined {
                    PathInfo::Folder(.., ref mut s) => {
                        *s = state_clone.lock().unwrap().selected().unwrap();
                    }
                    PathInfo::File(..) => panic!(),
                }
                (*drawn_dir_access).pop();
                joined = contents_access.join(&*drawn_dir_access).unwrap();
                state_clone.lock().unwrap().select(match joined {
                    PathInfo::Folder(.., s) => Some(*s as usize),
                    PathInfo::File(..) => panic!(),
                });
            }
            KeyCode::Char('r') => {
                let drawn_dir_clone = current_dir_clone.lock().unwrap().clone();
                let mut contents_access = (&contents_clone).lock().unwrap();
                let joined = contents_access.join(&drawn_dir_clone).unwrap();
                *joined = get_wrapped_contents(&mut join_path_to_vec(
                    &starting_dir_clone.lock().unwrap(),
                    drawn_dir_clone,
                ));
            }
            KeyCode::Char('g') => state_clone.lock().unwrap().select(Some(0)),
            KeyCode::Char('G') => {
                let dir_len = (&contents_clone)
                    .lock()
                    .unwrap()
                    .join(&*current_dir_clone.lock().unwrap())
                    .unwrap()
                    .contents()
                    .unwrap()
                    .len();
                state_clone.lock().unwrap().select(Some(dir_len - 1));
            }
            KeyCode::Ctrl('d') | KeyCode::Ctrl('f') => {
                let dir_len = (&contents_clone)
                    .lock()
                    .unwrap()
                    .join(&*current_dir_clone.lock().unwrap())
                    .unwrap()
                    .contents()
                    .unwrap()
                    .len();
                if dir_len != 0 {
                    let new_state = (((state_clone.lock().unwrap().selected().unwrap()
                        as isize)
                        + (termion::terminal_size().unwrap().1 as isize / 4))
                        .max(0) as usize)
                        .min(dir_len);
                    state_clone.lock().unwrap().select(Some(new_state));
                }
            }
            KeyCode::Ctrl('u') | KeyCode::Ctrl('b') => {
                let dir_len = (&contents_clone)
                    .lock()
                    .unwrap()
                    .join(&*current_dir_clone.lock().unwrap())
                    .unwrap()
                    .contents()
                    .unwrap()
                    .len();
                if dir_len != 0 {
                    let new_state = (((state_clone.lock().unwrap().selected().unwrap()
                        as isize)
                        - (termion::terminal_size().unwrap().1 as isize / 4))
                        .max(0) as usize)
                        .min(dir_len);
                    state_clone.lock().unwrap().select(Some(new_state));
                }
            }
            _ => (),
        } 
    
        return true;
    }
}