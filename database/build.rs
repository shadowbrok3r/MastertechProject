use std::{env, path::PathBuf};

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let env_path = PathBuf::from(&manifest_dir).join("..").join(".env");

    println!("cargo:rerun-if-changed={}", env_path.display());

    if env_path.exists() {
        // LOCAL DEV: Load from file
        for item in dotenvy::from_path_iter(&env_path).expect("read .env") {
            let (key, val) = item.expect("parse .env");
            println!("cargo:rustc-env={}={}", key, val);
        }
    } else {
        let required_vars = [
            "SCAFFOLD_USER", 
            "SCAFFOLD_PASS", 
            "DB_URL", 
            "DOWNLOAD_TOKEN", 
            "ISSUE_TOKEN", 
            "STORAGE_URL", 
            "REGION", 
            "DB_URL_DEV", 
            "DB_URL_LOCAL", 
            "WS_CLIENT_URL_LOCAL", 
            "WS_MASTER_URL_LOCAL", 
            "WS_CLIENT_URL", 
            "WS_MASTER_URL", 
            "USER_SCOPE", 
            "DB", 
            "NS"
        ];
        
        for key in required_vars {
            if let Ok(val) = env::var(key) {
                println!("cargo:rustc-env={}={}", key, val);
            }
        }
        
        println!("cargo:warning=.env file not found. Using system environment variables.");
    }
}