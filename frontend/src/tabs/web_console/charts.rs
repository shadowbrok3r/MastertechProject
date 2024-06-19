use std::{f64::consts::TAU, ops::RangeInclusive};

use egui::{Color32, NumExt, Response, ScrollArea, TextEdit, Ui, Vec2b, Widget};
use egui_plot::{
    AxisHints, Bar, BarChart, BoxElem, BoxPlot, BoxSpread, CoordinatesFormatter, Corner,
    GridInput, GridMark, HLine, Legend, Line, LineStyle, MarkerShape, Plot, PlotImage, PlotPoint,
    PlotPoints, VPlacement
};

const MINS_PER_DAY: f64 = 24.0 * 60.0;
const MINS_PER_H: f64 = 60.0;

#[derive(Copy, Clone, PartialEq)]
pub struct LinePlot<'a> {
    pub animate: bool,
    pub time: f64,
    pub square: bool,
    pub proportional: bool,
    pub coordinates: bool,
    pub x_values: &'a [f32],
    pub y_values: &'a [f32],
}


impl <'a>LinePlot<'a>{
    pub fn new(x: &'a [f32], y: &'a [f32]) -> Self {
        Self {
            animate: false,
            time: 0.0,
            square: true,
            proportional: true,
            coordinates: true,
            x_values: x,
            y_values: y
        }
    }
    pub fn thingy(&self) -> Line {
        Line::new(PlotPoints::from_ys_f32(self.y_values))
            .color(Color32::from_rgb(170,10,150))
            .style(LineStyle::Solid)
            .name("x = CPU Usage (%)")
    }

    pub fn other(&self) -> Line {
        Line::new(PlotPoints::from_ys_f32(self.x_values))
            .color(Color32::from_rgb(21, 232, 165))
            .style(LineStyle::Solid)
            .name("x = CPU Usage (%)")
    }

    pub fn ui(&mut self, ui: &mut Ui) -> Response {
        let time_formatter = |mark: GridMark, _digits, _range: &RangeInclusive<f64>| {
            let minutes = mark.value;
            if minutes < 0.0 || 5.0 * MINS_PER_DAY <= minutes {
                String::new()                                           // No labels outside value bounds
            } else {                                                   // Hours and minutes
                format!("{h}:{m:02}", h = hour(minutes), m = minute(minutes))
            }
        };

        let percentage_formatter = |mark: GridMark, _digits, _range: &RangeInclusive<f64>| {
            if is_approx_zero(mark.value) {
                String::new()                              // skip zero
            } else if is_approx_integer(mark.value) {
                format!("{:.0}%", mark.value)              // Display only integer percentages
            } else { String::new() }
        };

        let label_fmt = |_s: &str, val: &PlotPoint| {
            format!( "{h}:{m:02}\n{p:.2}%", h = hour(val.x), m = minute(val.x), p = percent(val.y))
        };

        let x_axes = vec![
            AxisHints::new_x().label("Time").formatter(time_formatter),
            AxisHints::new_x().label("Value"),
        ];
        let y_axes = vec![
            AxisHints::new_y()
                .label("Percent")
                .formatter(percentage_formatter)
                .max_digits(3),
            AxisHints::new_y()
                .label("Absolute")
                .placement(egui_plot::HPlacement::Right),
        ];

        if self.animate {
            ui.ctx().request_repaint();
            self.time += ui.input(|i| i.unstable_dt).at_most(1.0 / 30.0) as f64;
        };
        
        let mut plot = Plot::new("lines_demo")
            .legend(Legend::default())
            .custom_x_axes(x_axes)
            .custom_y_axes(y_axes)
            .label_formatter(label_fmt)
            .y_axis_width(4)
            .show_axes(true)
            .allow_drag(Vec2b::new(false, false))
            .allow_zoom(Vec2b::new(false, false))
            .allow_scroll(Vec2b::new(false, false))
            .x_axis_position(VPlacement::Top)
            .clamp_grid(true)
            .width(400.0)
            .height(400.0)
            .sharp_grid_lines(true)
            .show_grid(true);

        if self.square { plot = plot.view_aspect(1.0); }
        if self.proportional { plot = plot.data_aspect(1.0); }
        if self.coordinates {
            plot = plot.coordinates_formatter(Corner::LeftBottom, CoordinatesFormatter::default());
        }
        plot.show(ui, |plot_ui| {
            plot_ui.line(self.thingy());
            plot_ui.line(self.other())
        })
        .response
    }

    fn logistic_fn() -> Line {
        fn days(min: f64) -> f64 {
            MINS_PER_DAY * min
        }

        let values = PlotPoints::from_explicit_callback(
            move |x| 1.0 / (1.0 + (-2.5 * (x / MINS_PER_DAY - 2.0)).exp()),
            hour(0.0)..hour(5.0),
            100,
        );
        Line::new(values)
    }

    #[allow(clippy::needless_pass_by_value)]
    fn x_grid(input: GridInput) -> Vec<GridMark> {
        // Note: this always fills all possible marks. For optimization, `input.bounds`
        // could be used to decide when the low-interval grids (minutes) should be added.

        let mut marks = vec![];

        let (min, max) = input.bounds;
        let min = min.floor() as i32;
        let max = max.ceil() as i32;

        for i in min..=max {
            let step_size = if i % MINS_PER_DAY as i32 == 0 {
                // 1 day
                MINS_PER_DAY
            } else if i % MINS_PER_H as i32 == 0 {
                // 1 hour
                MINS_PER_H
            } else if i % 5 == 0 {
                // 5min
                5.0
            } else {
                // skip grids below 5min
                continue;
            };

            marks.push(GridMark {
                value: i as f64,
                step_size,
            });
        }

        marks
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

fn is_approx_zero(val: f64) -> bool {
    val.abs() < 1e-6
}

fn is_approx_integer(val: f64) -> bool {
    val.fract().abs() < 1e-6
}