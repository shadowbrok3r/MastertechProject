use eframe::egui::{text::LayoutJob, Color32, FontId, Stroke, TextEdit, TextFormat, Ui};

use crate::app_state::MastertechContext;


impl MastertechContext{
    pub fn output_console(&mut self, ui: &mut Ui) { 
        // ui.style_mut().visuals.selection.stroke.color =  Color32::BLACK;
        // ui.style_mut().visuals.selection.bg_fill = Color32::from_rgb(120, 10, 120);
        // ui.style_mut().visuals.widgets.inactive.fg_stroke =  Stroke::new(1.0, Color32::WHITE);
        // ui.style_mut().visuals.widgets.inactive.weak_bg_fill =  Color32::from_rgb(20, 20, 25);
        // ui.style_mut().visuals.widgets.inactive.bg_stroke =  Stroke::new(1.0, Color32::from_rgb(80, 80, 80));
        // ui.style_mut().visuals.widgets.open.bg_fill =  Color32::from_black_alpha(50);
        // ui.style_mut().visuals.widgets.open.weak_bg_fill =  Color32::from_black_alpha(50);
        // ui.style_mut().visuals.widgets.active.weak_bg_fill =  Color32::from_rgb(30,30,30);
        // ui.style_mut().visuals.widgets.hovered.weak_bg_fill =  Color32::TRANSPARENT;
        // ui.style_mut().visuals.widgets.hovered.bg_fill =  Color32::from_rgb(12, 12, 12);

        self.ctx.request_repaint();
        // The layouter function that uses the above two functions
        let mut layouter = |ui: &Ui, txt: &str, wrap_width: f32| {
            let mut layout_job = LayoutJob::default();
            layout_job.wrap.max_width = wrap_width;

            // First, color all occurrences of "Copying" in red
            let remaining_text = color_matching_text(&mut layout_job, txt, "Copying ", Color32::LIGHT_RED);

            // Then, color the text between "Copying" and "->" in yellow
            let final_text = color_between_delimiters(&mut layout_job, &remaining_text, ("Copying ", "->"), Color32::LIGHT_BLUE);

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


        ui.add_sized(ui.available_size(), 
            TextEdit::multiline(&mut self.output_text.to_string())
                .font(FontId::proportional(10.0))
                .hint_text("Output")
                .layouter(&mut layouter)
        );
    }
}


/// Function to color text between two delimiters
fn color_between_delimiters(
    layout_job: &mut LayoutJob,
    text: &str,
    delimiters: (&str, &str),
    color: Color32,
) -> String {
    let mut remaining_text = String::from(text);

    while let Some(start_idx) = remaining_text.find(delimiters.0) {
        let after_start = start_idx + delimiters.0.len();
        if let Some(end_idx) = remaining_text[after_start..].find(delimiters.1) {
            let end_idx = after_start + end_idx;

            // Append the text before the first delimiter
            layout_job.append(&remaining_text[..start_idx], 0.0, TextFormat::default());

            // Append the first delimiter itself
            layout_job.append(delimiters.0, 0.0, TextFormat::default());

            // Append the text between the delimiters with the given color
            layout_job.append(&remaining_text[after_start..end_idx], 0.0, TextFormat::simple(FontId::default(), color));

            // Append the second delimiter
            layout_job.append(delimiters.1, 0.0, TextFormat::default());

            // Update remaining_text to the part after the second delimiter
            remaining_text = remaining_text[end_idx + delimiters.1.len()..].to_string();
        } else {
            break;
        }
    }

    remaining_text
}

/// Function to color text that matches a specific substring
fn color_matching_text(
    layout_job: &mut LayoutJob,
    text: &str,
    pattern: &str,
    color: Color32,
) -> String {
    let mut remaining_text = String::from(text);

    while let Some(start_idx) = remaining_text.find(pattern) {
        let end_idx = start_idx + pattern.len();

        // Append the text before the pattern
        layout_job.append(&remaining_text[..start_idx], 0.0, TextFormat::default());

        // Append the pattern with the given color
        layout_job.append(pattern, 0.0, TextFormat::simple(FontId::default(), color));

        // Update remaining_text to the part after the pattern
        remaining_text = remaining_text[end_idx..].to_string();
    }

    remaining_text
}