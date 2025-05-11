use eframe::egui::{Color32, Frame, Label, Pos2, Rect, Scene, TextEdit, Ui, Vec2};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct StickyNote {
    title: String,
    body: String,
    position: Pos2, // Position of the sticky note in the scene
}

#[derive(Serialize, Deserialize)]
pub struct SceneEditor {
    scene_rect: Rect,
    sticky_notes: Vec<StickyNote>,
}

impl Default for SceneEditor {
    fn default() -> Self {
        Self {
            scene_rect: Rect::ZERO,
            sticky_notes: Vec::new(),
        }
    }
}

impl SceneEditor {
    pub fn ui(&mut self, ui: &mut Ui) {
        // ui.ctx().te
        Frame::group(ui.style())
            .inner_margin(0.0)
            .fill(Color32::BLACK)
            .show(ui, |ui| {
                let scene = Scene::new()
                    .max_inner_size([1000.0, 1000.0]) // Larger canvas for sticky notes
                    .zoom_range(0.1..=2.0);

                let mut reset_view = false;
                let mut inner_rect = Rect::NAN;
                let response = scene
                    .show(ui, &mut self.scene_rect, |ui| {
                        // Reset view button
                        reset_view = ui.button("Reset view").clicked();

                        // Plus button to add a new sticky note
                        if ui.button("+ Add Sticky Note").clicked() {
                            self.sticky_notes.push(StickyNote {
                                title: "New Note".to_string(),
                                body: "".to_string(),
                                position: Pos2::new(
                                    50.0 + (self.sticky_notes.len() as f32 * 20.0), // Slight offset for new notes
                                    50.0 + (self.sticky_notes.len() as f32 * 20.0),
                                ),
                            });
                        }

                        ui.add_space(16.0);

                        // Display each sticky note
                        for (i, note) in self.sticky_notes.iter_mut().enumerate() {
                            let note_rect = Rect::from_min_size(
                                note.position,
                                Vec2::new(200.0, 150.0), // Fixed size for each sticky note
                            );

                            ui.put(note_rect, |ui: &mut Ui| {
                                Frame::group(ui.style())
                                    .inner_margin(8.0)
                                    .fill(Color32::from_rgb(20,20,24)) 
                                    .stroke(ui.style().visuals.window_stroke)
                                    .show(ui, |ui| {
                                        // Title
                                        ui.horizontal(|ui| {
                                            ui.label("Title: ");
                                            ui.add(
                                                TextEdit::singleline(&mut note.title)
                                                    .desired_width(120.0),
                                            );
                                        });

                                        // Body
                                        ui.label("Body:");
                                        ui.add(
                                            TextEdit::multiline(&mut note.body)
                                                .desired_rows(5)
                                                .desired_width(180.0),
                                        );
                                    }).response
                            });
                        }

                        inner_rect = ui.min_rect();
                    })
                    .response;

                if reset_view || response.double_clicked() {
                    self.scene_rect = inner_rect;
                }
            });
    }
}