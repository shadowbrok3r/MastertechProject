use dotenvy::dotenv;

fn main() {
    dotenv().ok();
    // Pass environment variables to the compiler
    for (key, value) in std::env::vars() {
        if key.starts_with("DB_")
            || key.starts_with("NS")
            || key.starts_with("USER_SCOPE")
            || key.starts_with("PRESTA")
            || key.starts_with("SCAFFOLD")
            || key.starts_with("WS_")
            || key.starts_with("STORAGE")
            || key.starts_with("REGION")
            || key.starts_with("ISSUE")
            || key.starts_with("DOWNLOAD")
        {
            println!("cargo:rustc-env={key}={value}");
        }
    }
}

