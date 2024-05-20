#[cfg(target_os = "windows")]
extern crate embed_resource;

fn main() {
    #[cfg(target_os = "windows")]
    {
        static_vcruntime::metabuild();
        println!("cargo:rerun-if-changed=MasterTech.rc");
        embed_resource::compile("src/assets/MasterTech.rc", embed_resource::NONE);
        // println!("cargo rustc -- -Ctarget-feature=+crt-static");
        // println!("cargo:rustc-link-lib=static=stdc++");
    }
}