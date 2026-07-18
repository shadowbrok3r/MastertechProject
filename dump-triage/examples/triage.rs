//! CLI triage of a kernel dump: `cargo run -p dump-triage --release --example triage -- <path>`

fn main() {
    let path = std::env::args().nth(1).expect("usage: triage <dump.dmp>");
    match dump_triage::analyze_file(std::path::Path::new(&path)) {
        Ok(triage) => println!("{}", serde_json::to_string_pretty(&triage).unwrap()),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
