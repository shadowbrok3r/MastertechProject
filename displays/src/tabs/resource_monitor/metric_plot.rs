use egui_plot::{Corner, Legend, Line, LineStyle, Plot, PlotPoints};
use eframe::egui::{Color32, Response, RichText, TextStyle, Ui};
use std::collections::VecDeque;
use web_time::Instant;

use super::line_plot::interpolate_points;

#[derive(Clone, PartialEq, Default)]
pub struct MetricPlot {
    /// (x, y) pairs for the chart
    pub data: VecDeque<(f32, f32)>,  
    /// Label for x-axis
    x_label: String,
    /// Label for y-axis
    y_label: String,
    /// Track the start time for resetting x-axis
    start_time: Option<Instant>,
    /// Interval in seconds to reset the x-axis
    reset_interval: f32,

}

impl MetricPlot {
    pub fn new(x_label: &str, y_label: &str) -> Self {
        Self {
            data: VecDeque::new(),
            x_label: x_label.to_string(),
            y_label: y_label.to_string(),
            start_time: None,
            reset_interval: 30.0,
        }
    }

    pub fn update(&mut self, value: f32) {

        // (Optional) Trim old data if you don’t want it piling up forever
        // e.g., keep up to 2 cycles’ worth, etc.
        let now = if let Some(start_time) = self.start_time {
            start_time.elapsed().as_secs_f64()
        } else {
            // Initialize `start_time` on first call
            self.start_time = Some(Instant::now());
            0.0 // Assume the first point is at time 0.0
        };

        self.data.push_back((now as f32, value));

        // Remove data older than the last `reset_interval` seconds (if you desire a rolling window)
        while let Some(&(t , _)) = self.data.front() {
            if t < now  as f32 - self.reset_interval {
                self.data.pop_front();
            } else {
                break;
            }
        }
    }

    

    pub fn line(&self, name: &str, color: Color32) -> Line {
        // Convert data to PlotPoints (interpolation optional)
        let points: VecDeque<(f32, f32)> = self
            .data
            .iter()
            .map(|&(t, val)| (t, val))
            .collect();

        let interpolated = interpolate_points(&points);

        Line::new(PlotPoints::new(interpolated))
            .color(color)
            .style(LineStyle::Solid)
            .name(name)
    }


    pub fn ui(&mut self, ui: &mut Ui, plot_name: &str, color: Color32) -> Response {
        let x_label = RichText::new(&self.x_label).size(14.0).strong();
        let y_label = RichText::new(&self.y_label).size(14.0).strong();
        let line_chart = self.line(plot_name, color);

        if let Some(time) = self.start_time {
            let t = time.elapsed().as_secs_f32();
            if t > self.reset_interval {
                self
                    .data
                    .clear();
                self.start_time = Some(Instant::now());
            }
        }

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
            .label_formatter(|name, value| {
                if !name.is_empty() {
                    format!("{}: {:.*}%", name, 1, value.y)
                } else {
                    "".to_owned()
                }
            })
            // .include_x(left)
            // .include_x(right)
            .y_axis_label(y_label)
            .show(ui, |plot_ui| {
                plot_ui.line(line_chart);
            })
            .response
    }
}

// fn interpolate_cycle(
//     points: &[(f32, f32)], 
//     reset_interval: f32
// ) -> Vec<[f64; 2]> {
//     // If there are fewer than 2 points, just return them
//     if points.len() < 2 {
//         return points
//             .iter()
//             .map(|&(x, y)| [x as f64, y as f64])
//             .collect();
//     }

//     let mut out = Vec::new();

//     // Use your Catmull-Rom or other interpolation here.
//     // This is a basic linear interpolation example:
//     for i in 0..points.len() - 1 {
//         let (x1, y1) = points[i];
//         let (x2, y2) = points[i + 1];

//         // For example, 10 steps between points:
//         for step in 0..=10 {
//             let t = step as f64 / 10.0;
//             let x = x1 as f64 * (1.0 - t) + x2 as f64 * t;
//             let y = y1 as f64 * (1.0 - t) + y2 as f64 * t;
//             // Modulo so it shows up in [0, reset_interval)
//             out.push([x % reset_interval as f64, y]);
//         }
//     }
//     out
// }

// pub fn interpolate_points(
//     all_data: &[(f32, f32, u64)],
//     reset_interval: f32
// ) -> Vec<[f64; 2]> {
//     use std::collections::BTreeMap;

//     // Group points by cycle
//     let mut cycles = BTreeMap::<u64, Vec<(f32, f32)>>::new();

//     for &(abs_time, val, cycle) in all_data {
//         let local_x = abs_time % reset_interval;
//         cycles
//             .entry(cycle)
//             .or_default()
//             .push((local_x, val));
//     }

//     // Sort each cycle’s points by local_x
//     for vec_pts in cycles.values_mut() {
//         vec_pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
//     }

//     // Interpolate within each cycle separately, then concat
//     let mut result = Vec::new();
//     for (_cycle, pts) in cycles {
//         let mut interp = interpolate_cycle(&pts, reset_interval);
//         result.append(&mut interp);
//     }

//     result
// }
