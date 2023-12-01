use self_update::{
    self, 
    cargo_crate_version,
    backends::github::{
        ReleaseList,
        Update,
    }
};
use dotenv::*;



pub fn run() -> core::result::Result<(String, String), Box<dyn ::std::error::Error>> {

    let token: &str = dotenv::var("GITHUB_KEY").unwrap().as_str();

    let releases = ReleaseList::configure()
        .repo_owner("shadowbrok3r")
        .repo_name("Mastertech4.0")
        .auth_token(token)
        .build()?
        .fetch()?;

    println!("{releases:#?}\n");

    let status = Update::configure()
        .repo_owner("shadowbrok3r")
        .repo_name("Mastertech4.0")
        .bin_name("github")
        .show_download_progress(true)
        .target("Mastertech")
        //.bin_install_path(bin_install_path)
        .no_confirm(true)
        .auth_token(token)
        .current_version(cargo_crate_version!())
        .build()?
        .update()?;

    println!("Update status: `{}`!", status.version());
    let update_status = format!("{}", status.version());
    let release_versions = format!("{releases:#?}");
    Ok((release_versions, update_status))
}