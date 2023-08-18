#[cfg(target_os = "windows")]
extern crate embed_resource;

fn main() {
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rerun-if-changed=MasterTech.rc");
        static_vcruntime::metabuild();
        embed_resource::compile("MasterTech.rc", embed_resource::NONE);
    }

    // #[cfg(windows)]
    // if std::env::var("CARGO_CFG_TARGET_FAMILY").unwrap() == "windows" {
    //     let mut res = WindowsResource::new();
    //     match std::env::var("CARGO_CFG_TARGET_ENV").unwrap().as_str() {
    //         "gnu" => {
    //             res.set_ar_path("x86_64-w64-mingw32-ar")
    //                 .set_windres_path("x86_64-w64-mingw32-windres");
    //         }
    //         "msvc" => {}
    //         _ => panic!("unsupported env"),
    //     };
    //     res.set_icon("assets/icon.ico");
    //     res.compile()?;
    // }
}