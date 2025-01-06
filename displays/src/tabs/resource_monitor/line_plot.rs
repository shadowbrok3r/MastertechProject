use egui_plot::{Corner, Legend, Line, Plot, PlotPoints};
use eframe::egui::{Color32, Response, TextStyle, Ui};
use std::collections::{HashMap, VecDeque};

#[derive(Clone, PartialEq, Default)]
pub struct LinePlot {
    data: HashMap<String, VecDeque<(f32, f32)>>, // Map line names to VecDeque of (x, y) points
    max_points: usize,                          // Maximum number of points per line
}

impl LinePlot {
    pub fn new(max_points: usize) -> Self {
        Self {
            data: HashMap::new(),
            max_points,
        }
    }

    pub fn add_line(&mut self, name: &str) {
        self.data.entry(name.to_string()).or_insert_with(VecDeque::new);
    }

    pub fn update_line(&mut self, name: &str, x_value: f32, y_value: f32) {
        if let Some(points) = self.data.get_mut(name) {
            if points.len() >= self.max_points {
                points.pop_front();
            }
            points.push_back((x_value, y_value));
        }
    }

    pub fn lines(&self, colors: &HashMap<String, Color32>) -> Vec<Line> {
        self.data
            .iter()
            .filter_map(|(name, points)| {
                let color = colors.get(name)?;
                let plot_points: Vec<[f64; 2]> =
                    points.iter().map(|&(x, y)| [x as f64, y as f64]).collect();
                Some(
                    Line::new(PlotPoints::new(plot_points))
                        .color(*color)
                        .name(name),
                )
            })
            .collect()
    }

    pub fn ui(&self, ui: &mut Ui, plot_name: &str, colors: &HashMap<String, Color32>) -> Response {
        let plot = Plot::new(plot_name)
        .legend(
            Legend::default()
            .position(
                Corner::LeftTop
            )
            .text_style(
                TextStyle::Body
            )
            .background_alpha(0.90)
        )
        .width(ui.available_size_before_wrap().x/1.5)
        .height(ui.available_size_before_wrap().y/1.5)
        .allow_drag(false)
        .show_background(false);

        plot.show(ui, |plot_ui| {
            for line in self.lines(colors) {
                plot_ui.line(line);
            }
        })
        .response
    }
}
