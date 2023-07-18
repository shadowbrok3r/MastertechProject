#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release
use tokio::fs::*;
use tokio::io::*;
use std::path::Path;
use egui_file::{FileDialog, DialogType, Filter};
use std::sync::mpsc;
use std::thread::JoinHandle;
use env_logger;
use eframe::*;
use egui::Context;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> core::result::Result<(), eframe::Error>{
    env_logger::init(); // Log to stderr (if you run with `RUST_LOG=debug`).
    let options = eframe::NativeOptions {
        initial_window_size: Some(egui::vec2(1024.0, 768.0)),
        ..Default::default()
    };
    eframe::run_native(
        "My parallel egui App",
        options,
        Box::new(|_cc| Box::new(MyApp::new())),
    )
}

/// State per thread.
struct ThreadState {
    opened_file: Option<PathBuf>,
    open_file_dialog: Option<FileDialog>,
    open: bool,
}

impl ThreadState {
    fn new(thread_nr: usize, open: bool) -> Self {
        let title = format!("Background thread {thread_nr}");
        Self {
            opened_file: None,
            open_file_dialog: None,
            open: open,
        }
    }

    fn show(&mut self, ctx: &egui::Context) {
        let pos = egui::pos2(16.0, 128.0 * (self.thread_nr as f32 + 1.0));
        egui::Window::new(&self.title)
            .default_pos(pos)
            .show(ctx, |ui| {
                if self.open == true{
                    let mut dialog = FileDialog::open_file(self.opened_file.clone());
                    dialog.open();
                    self.open_file_dialog = Some(dialog);
                    self.open = false;
                }



                if let Some(dialog) = &mut self.open_file_dialog {
                    if dialog.show(ctx).selected() {
                        if let Some(file) = dialog.path() {
                            self.opened_file = Some(file);
                        }
                    }
                }


            });
    }
}

fn new_worker(
    thread_nr: usize,
    on_done_tx: mpsc::SyncSender<()>,
    open: bool,
) -> (JoinHandle<()>, mpsc::SyncSender<egui::Context>) {
    let (show_tx, show_rc) = mpsc::sync_channel(0);
    let handle = std::thread::Builder::new()
        .name(format!("EguiPanelWorker {}", thread_nr))
        .spawn(move || {
            let mut state = ThreadState::new(thread_nr, open);
            while let Ok(ctx) = show_rc.recv() {
                state.show(&ctx);
                let _ = on_done_tx.send(());
            }
        })
        .expect("failed to spawn thread");
    (handle, show_tx)
}

struct MyApp {
    threads: Vec<(JoinHandle<()>, mpsc::SyncSender<egui::Context>)>,
    on_done_tx: mpsc::SyncSender<()>,
    on_done_rc: mpsc::Receiver<()>,
    open: bool,
}

impl MyApp {
    fn new() -> Self {
        let threads = Vec::with_capacity(3);
        let (on_done_tx, on_done_rc) = mpsc::sync_channel(0);

        let mut slf = Self {
            threads,
            on_done_tx,
            on_done_rc,
            open: false,
        };

        slf.spawn_thread();
        slf.spawn_thread();

        slf
    }

    fn spawn_thread(&mut self) {
        let thread_nr = self.threads.len();
        self.threads
            .push(new_worker(thread_nr, self.on_done_tx.clone(), self.open));
    }
}

impl std::ops::Drop for MyApp {
    fn drop(&mut self) {
        for (handle, show_tx) in self.threads.drain(..) {
            std::mem::drop(show_tx);
            handle.join().unwrap();
        }
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::Window::new("Main thread").show(ctx, |ui| {
            if ui.button("Spawn another thread").clicked() {
                self.open = true;
                self.spawn_thread();

            }
            
        });

        for (_handle, show_tx) in &self.threads {
            let _ = show_tx.send(ctx.clone());
        }

        for _ in 0..self.threads.len() {
            let _ = self.on_done_rc.recv();
        }
    }
}
