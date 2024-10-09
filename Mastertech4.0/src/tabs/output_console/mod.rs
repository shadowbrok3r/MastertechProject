use displays::ui_tools::{color_between_delimiters, color_matching_text};
use eframe::egui::{text::LayoutJob, Color32, FontId, TextEdit, TextFormat, Ui};

use crate::app_state::MastertechContext;

impl MastertechContext {
    pub fn output_console(&mut self, ui: &mut Ui) {
        self.ctx.request_repaint();
        // The layouter function that uses the above two functions
        let mut layouter = |ui: &Ui, txt: &str, wrap_width: f32| {
            let mut layout_job = LayoutJob::default();
            layout_job.wrap.max_width = wrap_width;

            // First, color all occurrences of "Copying" in red
            let remaining_text =
                color_matching_text(&mut layout_job, txt, "Copying ", Color32::LIGHT_RED);

            // Then, color the text between "Copying" and "->" in yellow
            let final_text = color_between_delimiters(
                &mut layout_job,
                &remaining_text,
                ("Copying ", "->"),
                Color32::LIGHT_BLUE,
            );

            // Append any remaining text that wasn't processed by the previous functions
            layout_job.append(&final_text, 0.0, TextFormat::default());

            ui.fonts(|f| f.layout_job(layout_job))
        };

        // setup_terminal(ui, &self.output_text).unwrap();
        // if let Ok(data) = self.prestashop_api_rx.try_recv() { let res: String = serde_json::from_value(data).unwrap(); self.output_text += res.as_str(); }

        // let value = serde_json::json!({ "foo": "bar", "fizz": [1, 2, 3]});

        // // Simple:
        // JsonTree::new("simple-tree", &value).show(ui);
        // // Customised:
        // let response = JsonTree::new("customised-tree", &value)
        //     .style(JsonTreeStyle {
        //         bool_color: Color32::YELLOW,
        //         ..Default::default()
        //     })
        //     .default_expand(DefaultExpand::All)
        //     .abbreviate_root(true) // Show {...} when the root object is collapsed.
        //     .on_render(|ui, ctx| {
        //         // Customise rendering of the JsonTree, and/or handle interactions.
        //         match ctx {
        //             RenderContext::Property(ctx) => {
        //                 ctx.render_default(ui).context_menu(|ui| {
        //                     // Show a context menu when right clicking
        //                     // an array index or object key.
        //                 });
        //             }
        //             RenderContext::BaseValue(ctx) => {
        //                 // Show a button after non-recursive JSON values.
        //                 ctx.render_default(ui);
        //                 if ui.small_button("+").clicked() {
        //                     // ...
        //                 }
        //             }
        //             RenderContext::ExpandableDelimiter(ctx) => {
        //                 // Render array brackets and object braces as normal.
        //                 ctx.render_default(ui);
        //             }
        //         };
        //     })
        //     .show(ui);
        // // Reset the expanded state of all arrays/objects to respect the `default_expand` setting.
        // response.reset_expanded(ui);

        ui.add_sized(
            ui.available_size(),
            TextEdit::multiline(&mut self.output_text.to_string())
                .font(FontId::proportional(10.0))
                .hint_text("Output")
                .layouter(&mut layouter),
        );
    }
}
