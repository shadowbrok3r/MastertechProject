//! Manual smoke run for the GPU stressors: `cargo run -p stress-kit --example gpu_smoke`.
//!
//! Optional args pick the stressors and seconds instead of the default pair:
//! `cargo run -p stress-kit --example gpu_smoke -- gpu_display 10`. `gpu_display`
//! covers the screen while it runs and honours `STRESSKIT_DISPLAY_MODESET`.

use std::time::{Duration, Instant};

use stress_kit::{StressConfig, StressSession, Stressor};

struct StdoutLog;

impl log::Log for StdoutLog {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        if record.level() <= log::Level::Info {
            println!("[{}] {}", record.level(), record.args());
        }
    }
    fn flush(&self) {}
}

fn run(stressor: Stressor, memory_cap_mb: u64, secs: u64) {
    println!("=== {} ({} MiB cap, {secs}s) ===", stressor.label(), memory_cap_mb);
    let session = StressSession::start(StressConfig {
        stressor,
        threads: 0,
        timeout: Some(Duration::from_secs(secs)),
        memory_cap_mb,
        disk_file_mb: 1,
    });
    let deadline = Instant::now() + Duration::from_secs(secs + 5);
    let mut last = None;
    while Instant::now() < deadline {
        if let Some(m) = session.try_recv() {
            println!(
                "  t={:5.1}s  {:9.1} {}  errors={}  {}",
                m.elapsed_secs,
                m.throughput,
                stressor.throughput_unit(),
                m.errors,
                m.last_error.as_deref().unwrap_or("-")
            );
            last = Some(m);
        }
        if session.is_stopping() {
            std::thread::sleep(Duration::from_millis(300));
            if let Some(m) = session.try_recv() {
                last = Some(m);
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    match last {
        Some(m) => println!(
            "  FINAL: throughput={:.1} errors={} fatal={} last_error={:?}",
            m.throughput, m.errors, m.fatal, m.last_error
        ),
        None => println!("  FINAL: no metrics received"),
    }
}

fn main() {
    let _ = log::set_boxed_logger(Box::new(StdoutLog));
    log::set_max_level(log::LevelFilter::Info);

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        run(Stressor::GpuVram, 256, 8);
        run(Stressor::GpuPcie, 64, 6);
        return;
    }

    let secs = args
        .last()
        .and_then(|a| a.parse::<u64>().ok())
        .unwrap_or(8);
    for name in args.iter().filter(|a| a.parse::<u64>().is_err()) {
        match Stressor::from_str(name) {
            Some(s) => run(s, 256, secs),
            None => println!("unknown stressor {name:?}; try one of: {}", Stressor::labels_csv()),
        }
    }
}
