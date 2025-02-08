use windows::{
    core::{Result, BSTR, GUID, PCWSTR},
    Win32::{Foundation::{HANDLE, VARIANT_TRUE}, Security::{AdjustTokenPrivileges, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED, SE_SHUTDOWN_NAME, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY}, System::{
        Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED}, Threading::OpenProcessToken, UpdateAgent::{
            IDownloadJob, IUpdate, IUpdateCollection, IUpdateDownloader, IUpdateSearcher, IUpdateSession, ServerSelection
        }, Variant::VARIANT
    }},
};

// Correct CLSID for IUpdateSession
const CLSID_UPDATE_SESSION: GUID = GUID::from_u128(0x4CB43D7F_7EEE_4906_8698_60DA1C38F2FE);

/// Enables required privileges for performing update operations
unsafe fn enable_privilege(privilege: PCWSTR) -> bool {
    let mut h_token: HANDLE = HANDLE(std::ptr::null_mut());
    if OpenProcessToken(windows::Win32::System::Threading::GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut h_token).is_ok() {
        let mut token_privileges: TOKEN_PRIVILEGES = std::mem::zeroed();
        if LookupPrivilegeValueW(None, privilege, &mut token_privileges.Privileges[0].Luid).is_ok() {
            token_privileges.PrivilegeCount = 1;
            token_privileges.Privileges[0].Attributes = SE_PRIVILEGE_ENABLED;
            AdjustTokenPrivileges(h_token, false, Some(&token_privileges), 0, None, None).is_ok()
        } else {
            false
        }
    } else {
        false
    }
}

/// Searches for available updates using the specified update server
unsafe fn search_updates(update_searcher: &IUpdateSearcher, selection: i32) -> Result<IUpdateCollection> {
    update_searcher.SetServerSelection(ServerSelection(selection))?;
    let search_result = match selection {
        2 => update_searcher.Search(&BSTR::from("IsInstalled=0 and DeploymentAction='Installation'"))?,
        3 => update_searcher.Search(&BSTR::from("IsInstalled=0 and DeploymentAction='Installation' and IsHidden=0"))?,
        _ => return Err(windows::core::Error::from_win32()),
    };
    Ok(search_result.Updates()?)
}

pub fn install_windows_updates() -> Result<()> {
    unsafe {
        // Enable shutdown privileges for system updates
        enable_privilege(PCWSTR::from_raw(SE_SHUTDOWN_NAME.as_ptr()));
        println!("Initializing COM...");
        CoInitializeEx(Some(std::ptr::null_mut()), COINIT_MULTITHREADED).unwrap();

        // Create an update session
        println!("Creating Update Session...");
        let update_session: IUpdateSession = CoCreateInstance(&CLSID_UPDATE_SESSION, None, CLSCTX_INPROC_SERVER)?;
        let update_searcher: IUpdateSearcher = update_session.CreateUpdateSearcher()?;
        update_searcher.SetIncludePotentiallySupersededUpdates(VARIANT_TRUE)?;

        // Perform separate searches for Windows Update and Microsoft Update
        // Perform separate searches for Windows Update and Microsoft Update
        //     The ServerSelection enum has the following values:
        // 0: ssDefault → Uses system-configured update source (WSUS, Windows Update, etc.).
        // 1: ssManagedServer → Uses a WSUS (Windows Server Update Services) server.
        // 2: ssWindowsUpdate → Uses Windows Update (default for OS updates).
        // 3: ssMicrosoftUpdate → Uses Microsoft Update (needed for feature updates, optional updates, and drivers).
        println!("Searching Windows Update...");
        let updates_wu = search_updates(&update_searcher, 2)?;
        println!("Searching Microsoft Update...");
        let updates_mu = search_updates(&update_searcher, 3)?;
        
        let update_count = updates_wu.Count()? + updates_mu.Count()?;
        println!("Search completed. Found {update_count} updates.");

        if update_count == 0 {
            println!("No updates available.");
        } else {
            println!("List of updates:");
            let mut all_updates: Vec<IUpdate> = Vec::new();
            
            // Add Windows Update results
            for i in 0..updates_wu.Count()? {
                let update = updates_wu.get_Item(i)?;
                all_updates.push(update.clone());
                println!("- {}", update.Title()?.to_string());
            }
            // Add Microsoft Update results
            for i in 0..updates_mu.Count()? {
                let update = updates_mu.get_Item(i)?;
                all_updates.push(update.clone());
                println!("- {}", update.Title()?.to_string());
            }
            
            // Accept EULAs if required
            println!("Checking for EULA requirements...");
            for update in &all_updates {
                if !update.EulaAccepted()?.as_bool() {
                    println!("Accepting EULA for update: {}", update.Title()?.to_string());
                    update.AcceptEula()?;
                }
            }

            // Prepare to download updates
            println!("Preparing to download updates...");
            let update_downloader: IUpdateDownloader = update_session.CreateUpdateDownloader()?;
            let update_collection: IUpdateCollection = update_session.CreateUpdateSearcher()?.Search(&BSTR::from("IsInstalled=0"))?.Updates()?;
            update_downloader.SetUpdates(&update_collection)?;

            // Begin downloading updates
            println!("Downloading updates...");
            let download_job: IDownloadJob = update_downloader.BeginDownload(None, None, &VARIANT::default())?;
            
            // Monitor download progress
            while download_job.IsCompleted()?.as_bool() == false {
                let progress = download_job.GetProgress()?;
                println!("Download Progress: {}%", progress.PercentComplete()?);
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
            
            // Print update results
            for i in 0..all_updates.len() {
                let result = download_job.GetProgress()?.GetUpdateResult(i as i32)?;
                let code = result.ResultCode()?;
                println!("Update {i} result code: {code:?}");
            }
            download_job.CleanUp()?;
        }

        println!("Uninitializing COM...");
        drop(update_searcher);
        drop(update_session);
        CoUninitialize();
    };
    println!("Process completed.");
    Ok(())
}
