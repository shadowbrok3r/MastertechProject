use {
    crate::terminal_mode::tabs::scripts::{
        checklist::{Category, TodoItem}, 
        render::Reporter, ScriptsTab
    }, 
    powershell_script::PsScriptBuilder, 
    serde::Deserialize, 
    std::path::{Path, PathBuf}, 
    sysinfo::Disks, 
    walkdir::WalkDir
};

pub mod informational;
pub mod tuneup;
pub mod junkware;
pub mod prechecks;
pub mod stress_tests;

// pub trait ScriptTask: Send + 'static {
//     fn name(&self) -> &'static str;
//     fn run(&self) -> ScriptOutcome;
// }

// pub enum ScriptOutcome {
//     Passed,
//     Warning(String),
//     Failed(String),
//     Error(String),
// }

impl <'a> ScriptsTab <'a> {
    pub fn run_selected_scripts(&mut self, rerun: bool) {
        let selected = if rerun {
            let scripts = self.scripts_waiting_for_data.clone();
            if scripts.is_empty() {
                self.log_message("No scripts selected to run.");
                return;
            } else {
                scripts
            }
        } else {
            let scripts = self.get_selected_scripts();

            self.scripts_waiting_for_data = scripts.iter().filter(|s| {
                matches!(s.text.as_str(), "Activate Webroot" | "Activate SuperAnti" | "Activate SEB")
            }).cloned().collect::<Vec<TodoItem>>();

            if scripts.is_empty() {
                self.log_message("No scripts selected to run.");
                return;
            } else {
                scripts
            }
        };

        // Track scripts that report back via checklist_completion_tx; stress tests do not.
        {
            let mut batch = self.batch_pending.borrow_mut();
            for item in &selected {
                if !matches!(item.category(), Category::StressTests) {
                    batch.insert(item.text.clone());
                }
            }
        }

        for item in selected {
            let category = item.category().clone();
            self.current_script.replace(Some((category.clone(), item.text.clone())));
            log::info!("Set current script: {:?}", *self.current_script.borrow());

            match category {
                Category::Tuneup => self.handle_tuneup(item.text.as_str(), &category),
                Category::Informational => self.handle_informational(item.text.as_str(), &category),
                Category::JunkwareRemoval => self.handle_junkware_removal(item.text.as_str(), &category),
                Category::StressTests => self.handle_stress_tests(item.text.as_str(), &category),
                Category::UserScripts(ref script) => self.handle_custom(&script, item.text.as_str(), &category),
            }

            self.current_reporter.replace(match category {
                Category::Tuneup => {
                    if item.text.as_str() == "Data Transfer" {
                        Reporter::Robocopy
                    } else {
                        Reporter::Tuneup
                    }
                },
                Category::Informational => Reporter::Informational,
                Category::JunkwareRemoval => Reporter::JunkwareRemoval,
                Category::StressTests => Reporter::StressTest,
                Category::UserScripts(_) => Reporter::UserScript,
            });
      
            log::info!("Cleared current script");
        }
        self.log_message("All selected scripts completed.");
        if !rerun {
            self.clear_selected_scripts();
        }
        self.current_script.replace(None);
    }



    /// Runs a user script from the SurrealDB bucket via PowerShell.
    pub fn handle_custom(&mut self, full_path: &str, item_text: &str, category: &Category) {
        self.current_reporter.replace(Reporter::UserScript);
        self.log_message(format!("Running custom script '{item_text}'"));

        let bucket = self.filesystem.user.get_user_bucket_name();
        let path = full_path.to_string();
        let log_tx = self.script_log_tx.clone();
        let checklist_tx = self.checklist_completion_tx.clone();
        let category_clone = category.clone();
        let item_clone = item_text.to_string();

        tokio::spawn(async move {
            use database::schema::file_storage;

            let script_content = match file_storage::get_file_as_string(&bucket, &path).await {
                Ok(Some(content)) => content,
                Ok(None) => {
                    let _ = log_tx.try_send(format!("Script not found: {path}"));
                    let _ = checklist_tx.try_send((category_clone, item_clone, false));
                    return;
                }
                Err(e) => {
                    let _ = log_tx.try_send(format!("Failed to load script: {e}"));
                    let _ = checklist_tx.try_send((category_clone, item_clone, false));
                    return;
                }
            };

            #[cfg(target_os = "windows")]
            {
                let result = tokio::task::spawn_blocking(move || {
                    PsScriptBuilder::new()
                        .no_profile(true)
                        .non_interactive(true)
                        .hidden(true)
                        .print_commands(false)
                        .build()
                        .run(&script_content)
                })
                .await;

                match result {
                    Ok(Ok(output)) => {
                        if let Some(stdout) = output.stdout() {
                            let trimmed = stdout.trim();
                            if !trimmed.is_empty() {
                                let _ = log_tx.try_send(trimmed.to_string());
                            }
                        }
                        if let Some(stderr) = output.stderr() {
                            let trimmed = stderr.trim();
                            if !trimmed.is_empty() {
                                let _ = log_tx.try_send(format!("stderr: {trimmed}"));
                            }
                        }
                        let success = output.success();
                        let _ = checklist_tx.try_send((category_clone, item_clone, success));
                    }
                    Ok(Err(e)) => {
                        let _ = log_tx.try_send(format!("Script error: {e}"));
                        let _ = checklist_tx.try_send((category_clone, item_clone, false));
                    }
                    Err(e) => {
                        let _ = log_tx.try_send(format!("Script task failed: {e}"));
                        let _ = checklist_tx.try_send((category_clone, item_clone, false));
                    }
                }
            }

            #[cfg(not(target_os = "windows"))]
            {
                let _ = log_tx.try_send("User scripts require Windows".into());
                let _ = checklist_tx.try_send((category_clone, item_clone, false));
            }
        });
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

/// Sets all sleep/display/hibernate timeouts to never and turns hibernation
/// off. powercfg prints nothing on success, so success is judged by exit codes.
pub fn disable_hibernation_and_sleep() -> anyhow::Result<bool, anyhow::Error> {
    let ps_script = r#"
        $failures = @()
        powercfg /change standby-timeout-ac 0 | Out-Null;   if ($LASTEXITCODE -ne 0) { $failures += 'standby-timeout-ac' }
        powercfg /change standby-timeout-dc 0 | Out-Null;   if ($LASTEXITCODE -ne 0) { $failures += 'standby-timeout-dc' }
        powercfg /change monitor-timeout-ac 0 | Out-Null;   if ($LASTEXITCODE -ne 0) { $failures += 'monitor-timeout-ac' }
        powercfg /change monitor-timeout-dc 0 | Out-Null;   if ($LASTEXITCODE -ne 0) { $failures += 'monitor-timeout-dc' }
        powercfg /change hibernate-timeout-ac 0 | Out-Null; if ($LASTEXITCODE -ne 0) { $failures += 'hibernate-timeout-ac' }
        powercfg /change hibernate-timeout-dc 0 | Out-Null; if ($LASTEXITCODE -ne 0) { $failures += 'hibernate-timeout-dc' }
        powercfg /hibernate off | Out-Null;                 if ($LASTEXITCODE -ne 0) { $failures += 'hibernate-off' }
        if ($failures.Count -eq 0) { 'ALL_OK' } else { "FAILED: $($failures -join ', ')" }
    "#;

    let output = PsScriptBuilder::new()
        .no_profile(true)
        .non_interactive(true)
        .hidden(true)
        .print_commands(false)
        .build()
        .run(ps_script)?;

    let stdout = output.stdout().unwrap_or_default();
    let stdout = stdout.trim();
    log::info!("disable_hibernation_and_sleep -> {stdout:?}");

    if stdout.contains("ALL_OK") {
        Ok(true)
    } else {
        Err(anyhow::anyhow!(
            "powercfg reported: {stdout} (stderr: {:?})",
            output.stderr().unwrap_or_default().trim()
        ))
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