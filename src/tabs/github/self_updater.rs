use log::info;
use self_update::{
    self, 
    cargo_crate_version,
    backends::github::{
        ReleaseList,
        Update,
    }
};

pub fn run() -> core::result::Result<(String, String), Box<dyn ::std::error::Error>> {
    // let token = var("GITHUB_KEY").unwrap();
    let token = "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";

    let releases = ReleaseList::configure()
        .repo_owner("shadowbrok3r")
        .repo_name("Mastertech4.0")
        .auth_token(token)
        .build()?
        .fetch()?;

    info!("{releases:#?}\n");

    let status = Update::configure()
        .repo_owner("shadowbrok3r")
        .repo_name("Mastertech4.0")
        .bin_name("github")
        .target("MasterTech")
        .show_download_progress(true)
        .show_output(true)
        .no_confirm(true)
        .auth_token(token)
        .current_version(cargo_crate_version!())
        .build()?
        .update()?;
        // .bin_install_path(bin_install_path)
        // .current_version(ver)

    info!("Update status: `{}`!", status.version());

    let update_status = format!("{}", status.version());
    let release_versions = format!("{releases:#?}");
    Ok((release_versions, update_status))
}