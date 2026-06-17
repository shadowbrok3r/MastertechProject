use eframe::egui::{Color32, RichText, Ui};

/// Per-column aggregate over the currently selected rows.
pub struct SelColStat {
    pub label: &'static str,
    pub sum: f64,
    pub avg: f64,
    pub min: f64,
    pub max: f64,
    pub money: bool,
}

/// Column spec for selection stats: (label, value extractor, render as money).
pub type SelCol<T> = (&'static str, fn(&T) -> f64, bool);

/// Count of selected rows plus per-column sum/avg/min/max.
pub fn selection_stats<'a, T: 'a>(
    rows: impl Iterator<Item = &'a T>,
    cols: &[SelCol<T>],
) -> (usize, Vec<SelColStat>) {
    let mut count = 0usize;
    let mut sums = vec![0.0f64; cols.len()];
    let mut mins = vec![f64::INFINITY; cols.len()];
    let mut maxs = vec![f64::NEG_INFINITY; cols.len()];
    for r in rows {
        count += 1;
        for (i, (_, extract, _)) in cols.iter().enumerate() {
            let v = extract(r);
            sums[i] += v;
            if v < mins[i] { mins[i] = v; }
            if v > maxs[i] { maxs[i] = v; }
        }
    }
    let stats = cols
        .iter()
        .enumerate()
        .map(|(i, (label, _, money))| SelColStat {
            label,
            sum: sums[i],
            avg: if count > 0 { sums[i] / count as f64 } else { 0.0 },
            min: if count > 0 { mins[i] } else { 0.0 },
            max: if count > 0 { maxs[i] } else { 0.0 },
            money: *money,
        })
        .collect();
    (count, stats)
}

/// Render an Excel-style selection strip. No-op when nothing is selected.
/// Call inside a horizontal / horizontal_wrapped layout.
pub fn render_selection_stats(ui: &mut Ui, count: usize, stats: &[SelColStat]) {
    if count == 0 {
        return;
    }
    ui.colored_label(Color32::GOLD, format!("Selected: {count}"));
    for s in stats {
        let fmt = |v: f64| if s.money { format!("${:.2}", v) } else { format!("{:.2}", v) };
        ui.separator();
        ui.label(RichText::new(s.label).strong());
        ui.label(format!("sum {}", fmt(s.sum)));
        ui.label(format!("avg {}", fmt(s.avg)));
        ui.weak(format!("min {} / max {}", fmt(s.min), fmt(s.max)));
    }
}
