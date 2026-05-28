use std::{env, path::PathBuf};

include!("../build_hash.rs");

fn main() {
    emit_build_hash();

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let env_path = PathBuf::from(&manifest_dir).join("..").join(".env");

    println!("cargo:rerun-if-changed={}", env_path.display());

    if env_path.exists() {
        for item in dotenvy::from_path_iter(&env_path).expect("read .env") {
            let (key, val) = item.expect("parse .env");
            println!("cargo:rustc-env={}={}", key, val);
        }
    } else {
        if let Ok(val) = env::var("GEMINI_API_KEY") {
            println!("cargo:rustc-env=GEMINI_API_KEY={}", val);
        }
        println!("cargo:warning=.env file not found. Using system environment variables.");
    }
}
