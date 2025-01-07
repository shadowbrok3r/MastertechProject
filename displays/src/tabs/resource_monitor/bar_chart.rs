use egui_plot::{Bar, BarChart, Corner, Legend, Plot};
use eframe::egui::{Color32, Response, TextStyle, Ui};
use std::collections::VecDeque;

#[derive(Clone, PartialEq, Default)]
pub struct MetricBarChart {
    name: String,
    color: Color32,
    pub data: VecDeque<(f32, f32)>, // (x, y) pairs
    max_points: usize,          // Maximum number of bars
}

impl MetricBarChart {
    pub fn new(name: &str, color: Color32, max_points: usize) -> Self {
        Self {
            name: name.to_string(),
            color,
            data: VecDeque::new(),
            max_points,
        }
    }

    pub fn update(&mut self, x_value: f32, y_value: f32) {
        if self.data.len() >= self.max_points {
            self.data.pop_front();
        }
        self.data.push_back((x_value, y_value));
    }

    pub fn to_bar_chart(&self) -> BarChart {
        let bars: Vec<Bar> = self
            .data
            .iter()
            .map(|&(x, y)| Bar::new(x as f64, y as f64))
            .collect();

        BarChart::new(bars).name(self.name.clone()).color(self.color)
    }

    pub fn ui(&self, ui: &mut Ui, plot_name: &str) -> Response {
        Plot::new(plot_name)
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
            .show_background(false)
            .show(ui, |plot_ui| {
                plot_ui.bar_chart(self.to_bar_chart());
            })
            .response
    }
}
