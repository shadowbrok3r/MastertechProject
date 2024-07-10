use eframe::egui::{Color32, NumExt, Response, TextStyle, Ui, Vec2b};
use egui_plot::{
    CoordinatesFormatter, Corner, 
    Legend, Line, LineStyle, Plot, PlotPoint,
    PlotPoints, VPlacement
};

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
}

impl<'a> LinePlot<'a> {
    pub fn new(x: &'a [f32], y: &'a [f32]) -> Self {
        Self {
            animate: false,
            time: 0.0,
            square: true,
            proportional: true,
            coordinates: true,
            x_values: x,
            y_values: y,
        }
    }

    pub fn line(&self, name: &str, color: Color32) -> Line {
        Line::new(PlotPoints::from_ys_f32(self.y_values))
            .color(color)
            .style(LineStyle::Solid)
            .name(name)
    }

    pub fn ui(&mut self, ui: &mut Ui, plot_name: &str, line: Line) -> Response {
        // let time_formatter = |mark: GridMark, _digits, _range: &RangeInclusive<f64>| {
        //     let minutes = mark.value;
        //     if minutes < 0.0 || 5.0 * MINS_PER_DAY <= minutes {
        //         String::new() // No labels outside value bounds
        //     } else { // Hours and minutes
        //         format!("{h}:{m:02}", h = hour(minutes), m = minute(minutes))
        //     }
        // };

        let label_fmt = |_s: &str, val: &PlotPoint| {
            format!("{h}:{m:02}\n{p:.2}%", h = hour(val.x), m = minute(val.x), p = percent(val.y))
        };

        // let x_axes = vec![AxisHints::new_x().formatter(time_formatter)];

        if self.animate {
            ui.ctx().request_repaint();
            self.time += ui.input(|i| i.stable_dt).at_most(1.0 / 60.0) as f64;
            if self.time == 60.0 {
                self.time = 0.0;
            }
        };


        let mut plot = Plot::new(plot_name)
            .legend(Legend::default().position(Corner::RightBottom).text_style(TextStyle::Small))
            // .custom_x_axes(x_axes)
            .label_formatter(label_fmt)
            .show_axes(true)
            .allow_drag(Vec2b::new(false, false))
            .allow_scroll(Vec2b::new(false, false))
            .x_axis_position(VPlacement::Bottom)
            .width(ui.available_width() - 20.0)
            .height(100.0)
            .center_x_axis(true)
            .center_y_axis(true)
            .sharp_grid_lines(true)
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