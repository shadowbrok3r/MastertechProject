use self_update::{self, cargo_crate_version};

pub fn run() -> Result<(), Box<dyn ::std::error::Error>> {
    let releases = self_update::backends::github::ReleaseList::configure()
        .repo_owner("shadowbrok3r")
        .repo_name("Mastertech4.0")
        .build()?
        .fetch()?;
    println!("found releases:");
    println!("{:#?}\n", releases);

    let status = self_update::backends::github::Update::configure()
        .repo_owner("shadowbrok3r")
        .repo_name("Mastertech4.0")
        .bin_name("github")
        .show_download_progress(true)
        //.target_version_tag("v9.9.10")
        //.show_output(false)
        //.no_confirm(true)
        .auth_token("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")
        .current_version(cargo_crate_version!())
        .build()?
        .update()?;
    println!("Update status: `{}`!", status.version());
    Ok(())
}