use eframe::egui::{Color32, NumExt, Response, TextStyle, Ui, Vec2b};
use egui_plot::{CoordinatesFormatter, Corner, Legend, Line, LineStyle, Plot, PlotPoint, PlotPoints, VPlacement};
use log::info;

const MINS_PER_DAY: f64 = 24.0 * 60.0;
const MINS_PER_H: f64 = 60.0;

#[derive(Copy, Clone, PartialEq)]
pub struct LinePlot<'a> {
    animate: bool,
    time: f64,
    square: bool,
    proportional: bool,
    coordinates: bool,
    x_values: &'a [f32],
    y_values: &'a [f32],
    width: f32
}

// pub struct Bchart<'a> {
//     y_values: &'a [f32],
// }

// impl <'a> Bchart<'a> {
//     pub fn histogram(&self, y: &'a [f32], color: Color32) -> Line {
//         Bar::new(y, 50.0)
//         BarChart::new(PlotPoints::from_ys_f32(self.y_values))
//             .color(color)
            
//             .name(name)
//     }
// }

impl<'a> LinePlot<'a> {
    pub fn new(x: &'a [f32], y: &'a [f32], width: f32) -> Self {
        Self {
            animate: true,
            time: 0.0,
            square: true,
            proportional: false,
            coordinates: true,
            x_values: x,
            y_values: y,
            width
        }
    }

    pub fn line(&self, name: &str, color: Color32) -> Line {
        // PlotPoints::new([f32, f32])
        Line::new(PlotPoints::from_ys_f32(self.y_values))
            .color(color)
            .style(LineStyle::Solid)
            .name(name)
    }

    pub fn ui(&mut self, ui: &mut Ui, plot_name: &str, line: Line) -> Response {

        let label_fmt = |_s: &str, val: &PlotPoint| {
            format!("{h}:{m:02}\n{p:.2}%", h = hour(val.x), m = minute(val.x), p = percent(val.y))
        };
        if self.animate {
            ui.ctx().request_repaint();
            self.time += ui.input(|i| i.stable_dt).at_most(1.0 / 60.0) as f64;
            info!("Time: {:?}", self.time);
            if self.time == 60.0 {
                self.time = 0.0;
            }
        };

        let mut plot = Plot::new(plot_name)
            .legend(Legend::default().position(Corner::RightTop).text_style(TextStyle::Small))
            // .custom_x_axes(x_axes)
            .label_formatter(label_fmt)
            .show_axes(false)
            .allow_drag(Vec2b::new(true, false))
            .allow_scroll(Vec2b::new(true, false))
            .x_axis_position(VPlacement::Bottom)
            .y_axis_position(egui_plot::HPlacement::Left)
            .width(self.width)
            .height(100.0)
            .clamp_grid(true)
            .show_grid(true);

        if self.square {
            plot = plot.view_aspect(1.0);
        }
        if self.proportional {
            plot = plot.data_aspect(1.0);
        }
        if self.coordinates {
            plot = plot.coordinates_formatter(Corner::LeftBottom, CoordinatesFormatter::default());
        }
        plot.show(ui, |plot_ui| {
            plot_ui.line(line);
        })
        .response
    }
}

fn hour(x: f64) -> f64 {
    (x.rem_euclid(MINS_PER_DAY) / MINS_PER_H).floor()
}

fn minute(x: f64) -> f64 {
    x.rem_euclid(MINS_PER_H).floor()
}

fn percent(y: f64) -> f64 {
    100.0 * y
}