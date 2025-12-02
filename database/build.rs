use std::{env, path::PathBuf};

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let env_path = PathBuf::from(&manifest_dir).join("..").join(".env");

    println!("cargo:rerun-if-changed={}", env_path.display());

    for item in dotenvy::from_path_iter(&env_path).expect("read .env") {
        let (key, val) = item.expect("parse .env");
        println!("cargo:rustc-env={}={}", key, val);
    }
}