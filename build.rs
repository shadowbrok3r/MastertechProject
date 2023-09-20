#[cfg(target_os = "windows")]
extern crate embed_resource;

fn main() {
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rerun-if-changed=MasterTech.rc");
        // println!("cargo rustc -- -Ctarget-feature=+crt-static");
        // println!("cargo:rustc-link-lib=static=stdc++");
        static_vcruntime::metabuild();
        embed_resource::compile("MasterTech.rc", embed_resource::NONE);
    }
}