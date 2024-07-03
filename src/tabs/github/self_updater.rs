use log::info;
use reqwest::{header::{ACCEPT, CONTENT_TYPE}, Client};
use self_update::{
    self, backends::github::{
        ReleaseList, UpdateBuilder,
    }, cargo_crate_version
};

pub fn run(client: Client) -> core::result::Result<(String, String), Box<dyn ::std::error::Error>> {
    // let token = var("GITHUB_KEY").unwrap();
    let token = "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";
    // info!("Current version: {}", cargo_crate_version!());
    let releases = ReleaseList::configure()
        .repo_owner("shadowbrok3r")
        .repo_name("Mastertech4.0")
        .auth_token(token)
        .build()?
        .fetch()?;

    let release = releases[0].assets[0].download_url.clone();
    // info!("{release:#?}\n");

    // let tmp_dir = ::std::env::current_dir()?;
    // info!("tmp_tarball_path: {tmp_dir:?}");
    // let tmp_tarball_path = tmp_dir.as_path().join(&"git-MasterTech.exe");
    // info!("tmp_tarball_path: {tmp_tarball_path:?}");
    // let mut tmp_file = std::fs::File::create(&tmp_tarball_path)?;


    // let response = client.get(release) 
    //     .header(ACCEPT, "application/vnd.github+json")
    //     // .header(CONTENT_TYPE, "application/octet-stream")
    //     .bearer_auth(token)
    //     .send()
    //     .await?;

    // // Check if the request was successful
    // if response.status().is_success() {
    //     // Copy the response content into the file
    //     let content = response.bytes().await?;
    //     std::io::copy(&mut content.as_ref(), &mut tmp_file)?;
    //     info!("Download completed successfully!");
    // } else {
    //     eprintln!("Failed to download file: {:?}", response.status());
    // }
    let release_versions = format!("releases:#?");
    Ok((release_versions, "".to_string()))
}