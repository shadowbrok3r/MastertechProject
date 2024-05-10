use eframe::egui::{CentralPanel, Key, Sense, TextBuffer, Ui, Widget};

#[derive(Debug)]
struct Size {
    cols: u16,
    rows: u16,
}

pub fn setup_terminal(ui: &mut Ui, output: &String) -> anyhow::Result<(), anyhow::Error> {

    // disable_raw_mode()?;
    // // execute!(terminal.backend_mut(), LeaveAlternateScreen,)?;
    // terminal.show_cursor()?;

    Ok(())
}