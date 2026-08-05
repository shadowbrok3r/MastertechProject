//! Out-of-process host for the `gpu_display` stressor.
//!
//! Presenting is what resets the display miniport, and a reset invalidates
//! every graphics context in the process. Running the load here means the
//! reset kills this process instead of the app that launched it, so the app
//! stays alive to report what happened.
//!
//! Ticks go to stdout as one JSON [`Metrics`] object per line. Anything else
//! goes to stderr, which the parent logs.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        eprintln!(
            "stresskit-display [--modeset off|refresh|full] [--max-outputs N] [--max-secs N]\n\
             Streams gpu_display Metrics to stdout as JSON lines."
        );
        return;
    }
    let modeset = flag_value(&args, "--modeset");
    let max_outputs = flag_value(&args, "--max-outputs").and_then(|v| v.parse::<usize>().ok());
    let max_secs = flag_value(&args, "--max-secs").and_then(|v| v.parse::<u64>().ok());

    #[cfg(not(feature = "gpu"))]
    {
        let _ = (modeset, max_outputs, max_secs);
        eprintln!("stresskit-display: built without the gpu feature; nothing to run");
        std::process::exit(2);
    }

    #[cfg(feature = "gpu")]
    {
        let options = stress_kit::DisplayOptions {
            modeset: modeset.as_deref().and_then(stress_kit::DisplayModeSet::parse),
            max_outputs: max_outputs.filter(|c| *c > 0),
        };
        eprintln!(
            "stresskit-display: modeset={:?} max_outputs={:?} max_secs={:?}",
            options.modeset, options.max_outputs, max_secs
        );

        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let started_at = Instant::now();

        // Self-terminate if the parent goes away without stopping us.
        if let Some(secs) = max_secs {
            let belt = cancel.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(secs));
                belt.store(true, Ordering::SeqCst);
            });
        }

        let load_cancel = cancel.clone();
        let load = std::thread::spawn(move || {
            stress_kit::run_gpu_display_load(options, &load_cancel, &tx, started_at);
        });

        let mut out = std::io::stdout();
        for m in rx {
            match serde_json::to_string(&m) {
                Ok(line) => {
                    // A closed pipe means the parent is gone; stop presenting.
                    if writeln!(out, "{line}").is_err() || out.flush().is_err() {
                        cancel.store(true, Ordering::SeqCst);
                        break;
                    }
                }
                Err(e) => eprintln!("stresskit-display: could not encode tick: {e}"),
            }
        }
        cancel.store(true, Ordering::SeqCst);
        let _ = load.join();
        eprintln!("stresskit-display: finished after {:.1}s", started_at.elapsed().as_secs_f32());
    }
}

/// Value following `name`, or None when absent or last.
fn flag_value(args: &[String], name: &str) -> Option<String> {
    let i = args.iter().position(|a| a == name)?;
    args.get(i + 1).cloned()
}
