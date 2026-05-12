#[cfg(windows)]
extern crate embed_resource;

#[cfg(windows)]
extern crate winres;

// #[cfg(windows)]
// use embed_manifest::{embed_manifest, new_manifest};


fn main() {
    #[cfg(windows)]
    {
        static_vcruntime::metabuild();
        println!("cargo:rerun-if-changed=QcApp.rc");
        println!("cargo:rerun-if-changed=build.rs");
        let _ = embed_resource::compile("src/assets/QcApp.rc", embed_resource::NONE);
        println!("cargo rustc -- -Ctarget-feature=+crt-static");
        // println!("cargo:rustc-link-lib=static=stdc++");
        let mut res = winres::WindowsResource::new();
        res.set_manifest(r#"
            <assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
            <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
                <security>
                    <requestedPrivileges>
                        <requestedExecutionLevel level="requireAdministrator" uiAccess="false" />
                    </requestedPrivileges>
                </security>
            </trustInfo>
            </assembly>
        "#);
        res.compile().unwrap();   
    }
}