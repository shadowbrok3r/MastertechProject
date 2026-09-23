use {
    crate::terminal_mode::tabs::scripts::ScriptsTab,
    displays::scripts::ScriptCategory,
    powershell_script::PsScriptBuilder,
    serde::Deserialize,
    std::path::{Path, PathBuf},
    sysinfo::Disks,
    walkdir::WalkDir
};

impl ScriptsTab<'_> {
    /// Scans user profiles and opens the destination picker.
    pub fn data_transfer(&mut self, item_text: &str, category: &ScriptCategory) {
        self.loading = true;
        self.data_path_buttons.clear();
        self.log_message("Finding Data transfer candidates");
        let tx = self.path_size_tx.clone();
        std::thread::spawn(move || {
            match get_data_transfer_candidates() {
                Ok(paths) => { let _ = tx.try_send(paths); },
                Err(e) => log::error!("Error getting paths: {e:?}"),
            };
        });
        self.update_checklist(category.clone(), item_text, true);
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct LicenseStatus {
    #[serde(rename = "Description")]
    pub _description: String,
    #[serde(rename = "LicenseStatus")]
    pub license_status: i32
}

pub fn check_windows_activation() -> anyhow::Result<LicenseStatus, anyhow::Error> {
    let script = r#"
        Get-CimInstance SoftwareLicensingProduct -Filter "Name like 'Windows%'" | 
        where { $_.PartialProductKey } | select Description, LicenseStatus | ConvertTo-Json
    "#;

    let output = PsScriptBuilder::new()
        .no_profile(true)
        .non_interactive(true)
        .hidden(false)
        .print_commands(false)
        .build()
        .run(script)?;

    let result: LicenseStatus = serde_json::from_str(&output.stdout().unwrap_or_default())?;

    Ok(result)
}

/// Sets all sleep/display/hibernate timeouts to never, turns hibernation off,
/// and clears Fast Startup.
pub fn disable_hibernation_and_sleep() -> anyhow::Result<bool, anyhow::Error> {
    use crate::utilities::windows::power;

    let mut failures = power::disable_sleep_states();
    failures.extend(power::disable_display_timeout());

    if failures.is_empty() {
        log::info!("disable_hibernation_and_sleep -> all settings applied");
        Ok(true)
    } else {
        Err(anyhow::anyhow!("powercfg reported: {}", failures.join("; ")))
    }
}

pub fn get_data_transfer_candidates() -> anyhow::Result<Vec<(String, String)>, anyhow::Error> {
    let user_data = windows::Storage::UserDataPaths::GetDefault()?;
    let sys_data = windows::Storage::SystemDataPaths::GetDefault()?;

    log::info!(
        "User data: {:?}\n {:?}",
        user_data.Desktop()?,
        sys_data.UserProfiles()?
    );

    // user_data.
    let disks = Disks::new_with_refreshed_list();
    let mount_points = disks
        .iter()
        .map(|d| d.mount_point())
        .collect::<Vec<&Path>>();

    let mut paths_with_sizes = Vec::new();

    for drive in mount_points {

        let results = read_folder(
            drive.to_path_buf(), 
            1, 
            true
        );
        if !results.is_empty() {
            for path in results {
                let dir_size = get_directory_size(path.as_path());
                let formatted_size = format_size(dir_size);
        
                log::info!("Directory: {:>10} | Size: {}", path.display(), formatted_size);
                paths_with_sizes.push((path.to_string_lossy().to_string(), formatted_size));
            }
        }
    }
    

    Ok(paths_with_sizes)
}

/// Get the total size of a directory (recursive) in bytes
pub fn get_directory_size(path: &Path) -> u64 {
    WalkDir::new(path)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter_map(|e| e.metadata().ok())
        .filter(|metadata| metadata.is_file()) // Only count file sizes
        .map(|metadata| metadata.len())
        .sum()
}

/// Convert bytes to human-readable MB/GB
pub fn format_size(bytes: u64) -> String {
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    }
}

pub fn read_folder(mut path: PathBuf, depth: usize, read_dirs_only: bool) -> Vec<PathBuf> {
    // Construct the expected "Users" prefix from the input path (e.g., "C:/Users/")
    path.push("Users");

    let mut result: Vec<PathBuf> = WalkDir::new(path)
        .min_depth(depth)
        .max_depth(depth)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|entry| !read_dirs_only || entry.path().is_dir())
        .map(|entry| entry.path().to_path_buf())
        .filter(|path| {
            let is_users_path = path.starts_with(&path);

            let exclude = path.file_name()
                .map(|name| {
                    let name_str = name.to_string_lossy().to_lowercase();
                    name_str.contains("default") 
                    || name_str == "all users"
                    || name_str == "application data"
                    || name_str == "local settings"
                    || name_str == "nethood"
                    || name_str == "printhood"
                    || name_str == "recent items"
                    || name_str == "sendto"
                    || name_str == "start menu"
                    || name_str == "templates"
                })
                .unwrap_or(false);

            is_users_path && !exclude
        })
        .collect();

    result.sort_by(|a, b| {
        let da = a.is_dir();
        let db = b.is_dir();
        match da == db {
            true => a.file_name().cmp(&b.file_name()),
            false => db.cmp(&da),
        }
    });

    result
}



#[test]
fn test_read_folder() {
    use std::fs;
    let temp_dir = tempfile::tempdir().unwrap();
    let users_dir = temp_dir.path().join("Users");
    let alice = users_dir.join("Alice");
    let other_user = users_dir.join("Another User");
    let mut bob = users_dir.join("Bob");

    fs::create_dir(&users_dir).unwrap();
    fs::create_dir(&alice).unwrap();
    fs::create_dir(&other_user).unwrap();
    fs::create_dir(users_dir.join("Public")).unwrap();
    fs::create_dir(users_dir.join("Default")).unwrap();
    fs::create_dir(&bob).unwrap();

    
    fs::File::create(&other_user.join("test.txt")).unwrap();
    fs::File::create(&other_user.join("test1.txt")).unwrap();
    fs::File::create(&other_user.join("test2.txt")).unwrap();

    fs::File::create(&alice.join("test.txt")).unwrap();
    fs::File::create(&alice.join("test1.txt")).unwrap();
    fs::File::create(&alice.join("test2.txt")).unwrap();

    let source_user_name = alice.file_name().clone().unwrap_or_default();
    let source1_user_name = other_user.file_name().clone().unwrap_or_default();

    bob.push("Desktop");
    let desktop_backup_folder = if bob.ends_with("UsersBackup") {
        bob.clone()
    } else {
        let new_bob = bob.join("UsersBackup");
        std::fs::create_dir_all(&new_bob).unwrap();
        new_bob
    };
    let user_folder = desktop_backup_folder.join(source_user_name);
    let user_folder1 = desktop_backup_folder.join(source1_user_name);
    println!("desktop_backup_folder: {desktop_backup_folder:?}\nuser: {user_folder:?}\nuser1 {user_folder1:?}");
    std::fs::create_dir_all(&user_folder).unwrap();
    std::fs::create_dir_all(&user_folder1).unwrap();

    // println!(
    //     "user_backup: {user_backup:?}\nuser_backup_1: {user_backup_1:?}"
    // );

    // assert_eq!(names, vec!["Alice", "Bob"]); // Excludes Public, Default
}