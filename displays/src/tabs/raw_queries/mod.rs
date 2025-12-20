use crate::{file_viewer::{ColorTheme, FileViewer, Syntax}, PlatformSpawner, Spawner};
use eframe::egui::{CentralPanel, ScrollArea, TopBottomPanel, Ui};
use crossbeam::channel::{Receiver, Sender};
use database::DATABASE;
use serde_json::Value;
use std::fmt::Display;
use serde::Serialize;
use editor::Show;
pub mod editor;

#[derive(Serialize)]
pub struct QueryEditor {
    query: String,
    #[serde(skip)]
    query_tx: Sender<Value>,
    #[serde(skip)]
    query_rx: Receiver<Value>,
    response: Value,
    #[serde(skip)]
    editor: editor::Editor,
}

impl Default for QueryEditor {
    fn default() -> Self {
        let (tx, rx) = crossbeam::channel::unbounded();
        Self {
            query: String::new(),
            query_tx: tx,
            query_rx: rx,
            response: Default::default(),
            editor: editor::Editor::default(),
        }
    }
}

impl QueryEditor {
    pub fn ui(&mut self, ui: &mut Ui) {
        TopBottomPanel::bottom("query_editor_top")
            .show_inside(ui, |ui| {
                ui.vertical_centered(|ui| {
                    FileViewer::default()
                        .id_source("Script Editor")
                        .with_rows(5)
                        .vscroll(true)
                        .auto_shrink(false)
                        .with_fontsize(14.0)
                        .with_theme(ColorTheme::TOKYO_DARK)
                        .with_syntax(Syntax::powershell())
                        .with_numlines(true)
                        .show(ui, &mut self.query);
                });

                if ui.button("Execute Query").clicked() {
                    let tx = self.query_tx.clone();
                    let query = self.query.clone();
                    PlatformSpawner::spawn(async move {
                        let res = Self::execute_query(tx, query).await;
                        log::info!("Response: {:?}", res);
                    });
                }
            });

            CentralPanel::default()
                .show_inside(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ScrollArea::vertical()
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                self.show(ui);
                            });
                    });
                });
    }

    pub fn receive(&mut self) {
        if let Ok(value) = self.query_rx.try_recv() {
            self.response = value;
        }
    }

    pub async fn execute_query(tx: Sender<Value>, query: impl Display) -> anyhow::Result<(), anyhow::Error> {
        let val = DATABASE.query(query.to_string()).await?.take::<database::schema::SurrealDBValue>(0)?;
        let value = serde_json::to_value(&val)?;
        // log::info!("Query executed: {val:?}\nValue: {value:?}");
        let _ = tx.try_send(value);
        Ok(())
    }
}

