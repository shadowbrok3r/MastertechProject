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



/*** IUpdateSearcher::Search method (wuapi.h)

* Criterion 	Type 	Allowed operators 	Description
* Type 	string 	=, != 	Finds updates of a specific type, such as "'Driver'" and "'Software'".
* DeploymentAction 	string 	= 	Finds updates that are deployed for a specific action, such as an installation or uninstallation that the administrator of a server specifies.
* 
* "DeploymentAction='Installation'" finds updates that are deployed for installation on a destination computer. "DeploymentAction='Uninstallation'" depends on the other query criteria.
* 
* "DeploymentAction='Uninstallation'" finds updates that are deployed for uninstallation on a destination computer. "DeploymentAction='Uninstallation'" depends on the other query criteria.
* 
* If this criterion is not explicitly specified, each group of criteria that is joined to an AND operator implies "DeploymentAction='Installation'".
* IsAssigned 	int(bool) 	= 	Finds updates that are intended for deployment by Automatic Updates.
* 
* "IsAssigned=1" finds updates that are intended for deployment by Automatic Updates, which depends on the other query criteria. At most, one assigned Windows-based driver update is returned for each local device on a destination computer.
* 
* "IsAssigned=0" finds updates that are not intended to be deployed by Automatic Updates.
* BrowseOnly 	int(bool) 	= 	"BrowseOnly=1" finds updates that are considered optional.
* 
* "BrowseOnly=0" finds updates that are not considered optional.
* AutoSelectOnWebSites 	int(bool) 	= 	Finds updates where the AutoSelectOnWebSites property has the specified value.
* 
* "AutoSelectOnWebSites=1" finds updates that are flagged to be automatically selected by Windows Update.
* 
* "AutoSelectOnWebSites=0" finds updates that are not flagged for Automatic Updates.
* UpdateID 	string(UUID) 	=, != 	Finds updates for which the value of the UpdateIdentity.UpdateID property matches the specified value. Can be used with the != operator to find all the updates that do not have an UpdateIdentity.UpdateID of the specified value.
* 
* For example, "UpdateID='12345678-9abc-def0-1234-56789abcdef0'" finds updates for UpdateIdentity.UpdateID that equal 12345678-9abc-def0-1234-56789abcdef0.
* 
* For example, "UpdateID!='12345678-9abc-def0-1234-56789abcdef0'" finds updates for UpdateIdentity.UpdateID that are not equal to 12345678-9abc-def0-1234-56789abcdef0.
* Note  A RevisionNumber clause can be combined with an UpdateID clause that contains an = (equal) operator. However, the RevisionNumber clause cannot be combined with an UpdateID clause that contains the != (not-equal) operator.
*  
* 
* For example, "UpdateID='12345678-9abc-def0-1234-56789abcdef0' and RevisionNumber=100" can be used to find the update for UpdateIdentity.UpdateID that equals 12345678-9abc-def0-1234-56789abcdef0 and whose UpdateIdentity.RevisionNumber equals 100.
* RevisionNumber 	int 	= 	Finds updates for which the value of the UpdateIdentity.RevisionNumber property matches the specified value.
* 
* For example, "RevisionNumber=2" finds updates where UpdateIdentity.RevisionNumber equals 2.
* 
* This criterion must be combined with the UpdateID property.
* CategoryIDs 	string(uuid) 	contains 	Finds updates that belong to a specified category.
* IsInstalled 	int(bool) 	= 	Finds updates that are installed on the destination computer.
* 
* "IsInstalled=1" finds updates that are installed on the destination computer.
* 
* "IsInstalled=0" finds updates that are not installed on the destination computer.
* IsHidden 	int(bool) 	= 	Finds updates that are marked as hidden on the destination computer.
* 
* "IsHidden=1" finds updates that are marked as hidden on a destination computer. When you use this clause, you can set the UpdateSearcher.IncludePotentiallySupersededUpdates property to VARIANT_TRUE so that a search returns the hidden updates. The hidden updates might be superseded by other updates in the same results.
* 
* "IsHidden=0" finds updates that are not marked as hidden. If the UpdateSearcher.IncludePotentiallySupersededUpdates property is set to VARIANT_FALSE, it is better to include that clause in the search filter string so that the updates that are superseded by hidden updates are included in the search results. VARIANT_FALSE is the default value.
* IsPresent 	int(bool) 	= 	When set to 1, finds updates that are present on a computer.
* 
* "IsPresent=1" finds updates that are present on a destination computer. If the update is valid for one or more products, the update is considered present if it is installed for one or more of the products.
* 
* "IsPresent=0" finds updates that are not installed for any product on a destination computer.
* RebootRequired 	int(bool) 	= 	Finds updates that require a computer to be restarted to complete an installation or uninstallation.
* 
* "RebootRequired=1" finds updates that require a computer to be restarted to complete an installation or uninstallation.
* 
* "RebootRequired=0" finds updates that do not require a computer to be restarted to complete an installation or uninstallation.
***/
