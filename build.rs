#[cfg(target_os = "windows")]
extern crate embed_resource;

#[cfg(target_os = "windows")]
fn main() {
    static_vcruntime::metabuild();
    println!("cargo:rerun-if-changed=MasterTech.rc");
    embed_resource::compile("MasterTech.rc", embed_resource::NONE);
    // println!("cargo rustc -- -Ctarget-feature=+crt-static");
    // println!("cargo:rustc-link-lib=static=stdc++");
}