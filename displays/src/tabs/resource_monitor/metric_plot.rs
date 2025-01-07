use egui_plot::{Corner, Legend, Line, LineStyle, Plot, PlotPoints};
use eframe::egui::{Color32, Response, RichText, TextStyle, Ui};
use std::collections::VecDeque;

use super::line_plot::interpolate_points;

#[derive(Clone, PartialEq, Default)]
pub struct MetricPlot {
    pub data: VecDeque<(f32, f32)>, // (x, y) pairs for the chart
    x_label: String,            // Label for x-axis
    y_label: String,            // Label for y-axis
}

impl MetricPlot {
    pub fn new(x_label: &str, y_label: &str) -> Self {
        Self {
            data: VecDeque::new(),
            x_label: x_label.to_string(),
            y_label: y_label.to_string(),
        }
    }

    pub fn update(&mut self, x_value: f32, y_value: f32) {
        const MAX_BARS: usize = 50;
        if self.data.len() >= MAX_BARS {
            self.data.pop_front();
        }
        self.data.push_back((x_value, y_value));
    }

    pub fn line(&self, name: &str, color: Color32) -> Line {
        // let update_interval = 2.0; // Time between updates in seconds

        // let points: Vec<[f64; 2]> = self.data
        //     .iter()
        //     // .zip(&self.y_values)
        //     .map(|(x, y)| [*x as f64 * update_interval, *y as f64])
        //     .collect();
        
        let interpolated_points = interpolate_points(&self.data);

        Line::new(PlotPoints::new(interpolated_points))
            .color(color)
            .style(LineStyle::Solid)
            .name(name)
    }

    pub fn ui(&self, ui: &mut Ui, plot_name: &str, color: Color32) -> Response {
        let x_label = RichText::new(&self.x_label).size(14.0).strong();
        let y_label = RichText::new(&self.y_label).size(14.0).strong();

        // let bars: Vec<Bar> = self
        //     .data
        //     .iter()
        //     .map(|&(x, y)| Bar::new(x as f64, y as f64))
        //     .collect();

        // let bar_chart = BarChart::new(bars).name(plot_name).color(color);
        let line_chart = self.line(plot_name, color);

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
            .x_axis_label(x_label)
            .y_axis_label(y_label)
            .show(ui, |plot_ui| {
                plot_ui.line(line_chart);
            })
            .response
    }
}