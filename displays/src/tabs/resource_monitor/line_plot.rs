use egui_plot::{Corner, Legend, Line, Plot, PlotBounds, PlotPoints};
use eframe::egui::{Color32, Response, TextStyle, Ui};
use std::collections::{HashMap, VecDeque};

#[derive(Clone, PartialEq, Default)]
pub struct LinePlot {
    pub data: HashMap<String, VecDeque<(f32, f32)>>, // Map line names to VecDeque of (x, y) points
    pub max_points: usize,                          // Maximum number of points per line
}

impl LinePlot {
    pub fn new(max_points: usize) -> Self {
        Self {
            data: HashMap::new(),
            max_points,
        }
    }

    pub fn update_line(&mut self, name: &str, x_value: f32, y_value: f32) {
        self.data
            .entry(name.to_string())
            .and_modify(|points| {
                if points.len() >= self.max_points {
                    points.pop_front();
                }
                points.push_back((x_value, y_value));
            })
            .or_insert_with(|| {
                let mut deque = VecDeque::new();
                deque.push_back((x_value, y_value));
                deque
            });
    }

    pub fn lines(&self, colors: &mut HashMap<String, Color32>) -> Vec<Line> {
        // A set of default colors to use
        let default_colors = [
            Color32::RED,
            Color32::GREEN,
            Color32::BLUE,
            Color32::from_rgb(235, 12, 38),
            Color32::from_rgb(12, 235, 97),
            Color32::from_rgb(240, 141, 55),
            Color32::from_rgb(0, 255, 255), // Cyan
            Color32::from_rgb(255, 0, 255), // Magenta
            Color32::from_rgb(128, 0, 128), // Indigo
        ];
    
        // Iterator over the default colors
        let mut color_iter = default_colors.iter().cycle();
    
        self.data
            .iter()
            .map(|(name, points)| {
                // Get or insert a color for this name
                let color = *colors.entry(name.clone()).or_insert_with(|| {
                    // Use the next color in the iterator
                    *color_iter.next().unwrap()
                });
    
                // Map the points to PlotPoints
                // let plot_points: Vec<[f64; 2]> =
                //     points.iter().map(|&(x, y)| [x as f64, y as f64]).collect();
    
                // Interpolate the points for a smooth curve
                let interpolated_points = interpolate_points(&*points);
                // Create and return the line
                Line::new(PlotPoints::new(interpolated_points))
                    .color(color)
                    .width(3.)
                    .name(name)
            })
            .collect()
    }
    
    pub fn ui(&self, ui: &mut Ui, plot_name: &str, colors: &mut HashMap<String, Color32>) -> Response {
        log::info!("self.data: {:?}", self.data);
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
        .width(ui.available_width()/1.1)
        .height(ui.available_height()/1.1)
        .allow_drag(false)
        .show_background(false);

        plot.show(ui, |plot_ui| {
            plot_ui.set_plot_bounds(PlotBounds::NOTHING.make_x_symmetrical());
            for line in self.lines(colors) {
                plot_ui.line(line);
            }
        })
        .response
    }
}


pub fn interpolate_points(points: &VecDeque<(f32, f32)>) -> Vec<[f64; 2]> {
    if points.len() < 2 {
        // Not enough points to interpolate
        return points.iter().map(|&(x, y)| [x as f64, y as f64]).collect();
    }

    let mut result = Vec::new();

    // Generate interpolated points between each pair of points
    for i in 0..points.len() - 1 {
        let p0 = if i == 0 { points[i] } else { points[i - 1] };
        let p1 = points[i];
        let p2 = points[i + 1];
        let p3 = if i + 2 < points.len() {
            points[i + 2]
        } else {
            points[i + 1]
        };

        // Interpolate between p1 and p2 using Catmull-Rom spline
        for t in 0..=10 {
            let t = t as f64 / 10.0;
            let x = catmull_rom(t, p0.0 as f64, p1.0 as f64, p2.0 as f64, p3.0 as f64);
            let y = catmull_rom(t, p0.1 as f64, p1.1 as f64, p2.1 as f64, p3.1 as f64);
            result.push([x, y]);
        }
    }

    result
}

fn catmull_rom(t: f64, p0: f64, p1: f64, p2: f64, p3: f64) -> f64 {
    0.5 * ((2.0 * p1)
        + (-p0 + p2) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t * t
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t * t * t)
}
