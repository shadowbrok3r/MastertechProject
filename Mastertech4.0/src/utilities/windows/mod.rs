use windows::{
    core::{Result, BSTR, GUID, HRESULT, PCWSTR},
    Win32::{Foundation::HANDLE, Security::{AdjustTokenPrivileges, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED, SE_SHUTDOWN_NAME, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY}, System::{
        Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED}, Threading::OpenProcessToken, UpdateAgent::{
            IDownloadJob, IUpdate, IUpdateCollection, IUpdateDownloader, IUpdateSearcher, IUpdateServiceManager, IUpdateSession, ServerSelection
        }, Variant::VARIANT
    }},
};

/*
 * we can also output some better diagnostic info about updates / upgrades by downloading 
 * SetupDiag:
 * https://go.microsoft.com/fwlink/?linkid=870142
    SetupDiag.exe /ZipLogs:False /Format:Json /Output:%windir%\logs\SetupDiag\SetupDiagResults.json 
    /RegPath:HKEY_LOCAL_MACHINE\SYSTEM\Setup\SetupDiag\Results
*/

// Correct CLSID for IUpdateSession and IUpdateServiceManager
const CLSID_UPDATE_SESSION: GUID = GUID::from_u128(0x4CB43D7F_7EEE_4906_8698_60DA1C38F2FE);
const CLSID_UPDATE_SERVICE_MANAGER: GUID = GUID::from_u128(0xf8d253d9_89a4_4daa_87b6_1168369f0b21);
const MICROSOFT_UPDATE_SERVICE_ID: &str = "7971f918-a847-4430-9279-4a52d1efe18d";

/// Enum mapping HRESULT error codes to readable descriptions
#[derive(Debug)]
enum WindowsUpdateError {
    NotSupported,
    NoService,
    UnknownId,
    InvalidIndex,
    OperationInProgress,
    InvalidOperation,
    DownloadFailed,
    NotApplicable,
    Other(u32),
}

impl From<HRESULT> for WindowsUpdateError {
    fn from(hr: HRESULT) -> Self {
        match hr.0 as u32 {
            0x80240037 => WindowsUpdateError::NotSupported,
            0x80240001 => WindowsUpdateError::NoService,
            0x80240003 => WindowsUpdateError::UnknownId,
            0x80240007 => WindowsUpdateError::InvalidIndex,
            0x80240009 => WindowsUpdateError::OperationInProgress,
            0x80240036 => WindowsUpdateError::InvalidOperation,
            0x80240034 => WindowsUpdateError::DownloadFailed,
            0x80240017 => WindowsUpdateError::NotApplicable,
            _ => WindowsUpdateError::Other(hr.0 as u32),
        }
    }
}

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

/// Checks if Microsoft Update is enabled and enables it if necessary
unsafe fn ensure_microsoft_update_enabled() -> Result<()> {
    println!("Checking if Microsoft Update is enabled...");
    let service_manager: IUpdateServiceManager = CoCreateInstance(&CLSID_UPDATE_SERVICE_MANAGER, None, CLSCTX_INPROC_SERVER)?;
    let services = service_manager.Services()?;
    let count = services.Count()?;
    
    for i in 0..count {
        let service = services.get_Item(i)?;
        let service_id = service.ServiceID()?;
        if service_id.to_string().eq_ignore_ascii_case(MICROSOFT_UPDATE_SERVICE_ID) {
            println!("Microsoft Update is already enabled.");
            return Ok(());
        }
    }
    
    println!("Microsoft Update is not enabled. Enabling it now...");
    service_manager.AddService(&BSTR::from(MICROSOFT_UPDATE_SERVICE_ID),  &BSTR::from(""))?;
    println!("Microsoft Update has been successfully enabled.");
    Ok(())
}

/// Searches for available updates using the specified update server
unsafe fn search_updates(update_searcher: &IUpdateSearcher, selection: i32) -> Result<IUpdateCollection> {
    let result = match selection {
        2 => {
            println!("ServerSelection: Windows Update");
            update_searcher.SetServerSelection(ServerSelection(2))?;
            update_searcher.Search(&BSTR::from("(IsInstalled=0 and DeploymentAction='Installation' and BrowseOnly=1 or BrowseOnly=0) or (IsHidden=1 and IsInstalled=0)"))?
        },
        3 => {
            println!("ServerSelection: Microsoft Update");
            update_searcher.SetServerSelection(ServerSelection(3))?;
            update_searcher.SetServiceID(&BSTR::from(MICROSOFT_UPDATE_SERVICE_ID))?;
            update_searcher.Search(&BSTR::from("IsInstalled=0 and DeploymentAction='Installation'"))?
        },
        _ => return Err(windows::core::Error::from_win32()),
    };
    
    Ok(result.Updates()?)
}

/// Handles installation of updates from a given update collection
unsafe fn install_updates(update_session: &IUpdateSession, updates: &IUpdateCollection) -> Result<()> {
    let update_downloader: IUpdateDownloader = update_session.CreateUpdateDownloader()?;
    update_downloader.SetUpdates(updates)?;
    println!("Beginning download of updates...");
    let download_job: IDownloadJob = update_downloader.BeginDownload(None, None, &VARIANT::default())?;

    while !download_job.IsCompleted()?.as_bool() {
        let progress = download_job.GetProgress()?;
        println!("Download Progress: {}%", progress.PercentComplete()?);
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
    download_job.CleanUp()?;
    println!("Download completed. Installing updates...");

    let installer = update_session.CreateUpdateInstaller()?;
    installer.SetUpdates(updates)?;
    let install_result = installer.Install()?;
    println!("Installation result: {:?}", install_result.ResultCode()?);
    // update_downloader.EndDownload(value)
    Ok(())
}

/// Filters and installs updates separately for Windows Update and Microsoft Update
unsafe fn process_updates(update_session: &IUpdateSession, update_collection: &IUpdateCollection) -> Result<()> {
    // let update_collection: IUpdateCollection = update_session.CreateUpdateSearcher()?.Search(&BSTR::from("IsInstalled=0 and DeploymentAction='Installation'"))?.Updates()?;
    for i in 0..update_collection.Count()? {
        let update = update_collection.get_Item(i)?;
        let is_installed = update.IsInstalled()?.as_bool();
        let is_downloaded = update.IsDownloaded()?.as_bool();
        if !is_installed && !is_downloaded {
            println!("Adding update: {}", update.Title()?.to_string());
            println!("=>  IsInstalled: {:?}", is_installed);
            println!("=>  IsMandatory: {:?}", update.IsMandatory()?.as_bool());
            println!("=>  IsHidden: {:?}", update.IsHidden()?.as_bool());
            println!("=>  AutoSelectOnWebSites: {:?}", update.AutoSelectOnWebSites()?.as_bool());
            println!("=>  IsDownloaded: {:?}", is_downloaded);
            println!("=>  Description: {:?}", update.Description()?);
            // update_collection.Add(&update)?;
        } else {
            println!("Skipping update: {}", update.Title()?.to_string());
            update_collection.RemoveAt(i)?;
        }
    }
    install_updates(update_session, &update_collection)
}

pub fn install_windows_updates() -> Result<()> {
    unsafe {
        // Enable shutdown privileges for system updates
        enable_privilege(PCWSTR::from_raw(SE_SHUTDOWN_NAME.as_ptr()));
        println!("Initializing COM...");
        CoInitializeEx(Some(std::ptr::null_mut()), COINIT_MULTITHREADED).unwrap();

        println!("Ensuring Microsoft Update is enabled...");
        ensure_microsoft_update_enabled()?;
        
        // Create an update session
        println!("Creating Update Session...");
        let update_session: IUpdateSession = CoCreateInstance(&CLSID_UPDATE_SESSION, None, CLSCTX_INPROC_SERVER)?;
        let update_searcher: IUpdateSearcher = update_session.CreateUpdateSearcher()?;
        
        // Perform separate searches for Windows Update and Microsoft Update
        // Perform separate searches for Windows Update and Microsoft Update
        //     The ServerSelection enum has the following values:
        // 0: ssDefault → Uses system-configured update source (WSUS, Windows Update, etc.).
        // 1: ssManagedServer → Uses a WSUS (Windows Server Update Services) server.
        // 2: ssWindowsUpdate → Uses Windows Update (default for OS updates).
        // 3: ssMicrosoftUpdate → Uses Microsoft Update (needed for feature updates, optional updates, and drivers).
        println!("Searching Windows Update...");
        let updates_wu = search_updates(
            &update_searcher, 
            2
        )
        .inspect_err(|e| 
            println!("Windows update error: {:?}", WindowsUpdateError::from(e.code()))
        )?;

        println!("Searching Microsoft Update...");
        let updates_mu = search_updates(
            &update_searcher, 
            3
        )
        .inspect_err(|e| 
            println!("Microsoft update error: {:?}", WindowsUpdateError::from(e.code()))
        )?;

        let res = process_updates(&update_session, &updates_wu);
        let res1 = process_updates(&update_session, &updates_mu);

        println!("Res: {res:?}\nRes1: {res1:?}");
        // let update_count = updates_wu.Count()? + updates_mu.Count()?;
        // println!("Search completed. Found {update_count} updates.");

        // if update_count == 0 {
        //     println!("No updates available.");
        // } else {
        //     println!("List of updates:");
        //     let mut all_updates: Vec<IUpdate> = Vec::new();
            
        //     // Add Windows Update results
        //     for i in 0..updates_wu.Count()? {
        //         let update = updates_wu.get_Item(i)?;
        //         all_updates.push(update.clone());
        //         println!("- {}", update.Title()?.to_string());
        //     }
        //     // Add Microsoft Update results
        //     for i in 0..updates_mu.Count()? {
        //         let update = updates_mu.get_Item(i)?;
        //         all_updates.push(update.clone());
        //         println!("- {}", update.Title()?.to_string());
        //     }
            
        //     // Accept EULAs if required
        //     println!("Checking for EULA requirements...");
        //     for update in &all_updates {
        //         if !update.EulaAccepted()?.as_bool() {
        //             println!("Accepting EULA for update: {}", update.Title()?.to_string());
        //             update.AcceptEula()?;
        //         }
        //     }

        //     // Prepare to download WINDOWS updates
        //     println!("Preparing to Download Microsoft Updates...");
        //     let update_downloader: IUpdateDownloader = update_session.CreateUpdateDownloader()?;
        //     let update_collection: IUpdateCollection = update_session.CreateUpdateSearcher()?.Search(&BSTR::from("IsInstalled=0"))?.Updates()?;

        //     for i in 0..updates_wu.Count()? {
        //         println!("Adding wu updates to collection");
        //         let update = updates_wu.get_Item(i)?;
        //         let is_installed = update.IsInstalled()?.as_bool();
        //         println!("Checking update: {}", update.Title()?.to_string());
        //         println!("=>  IsInstalled: {:?}", is_installed);
        //         println!("=>  IsMandatory: {:?}", update.IsMandatory()?.as_bool());
        //         println!("=>  IsHidden: {:?}", update.IsHidden()?.as_bool());
        //         println!("=>  AutoSelectOnWebSites: {:?}", update.AutoSelectOnWebSites()?.as_bool());
        //         println!("=>  IsDownloaded: {:?}", update.IsDownloaded()?.as_bool());
        //         println!("=>  Description: {:?}", update.Description()?);
        //         if !is_installed && !update.IsDownloaded()?.as_bool() {
        //             println!("Adding update: {}", update.Title()?.to_string());
        //             update_collection.Add(&update).inspect_err(|e| println!("Add Err: {e:?}"))?;
        //         } else {
        //             println!("Skipping update: {}", update.Title()?.to_string());
        //         }
        //     }

        //     // now install MICROSOFT updates
        //     for i in 0..updates_mu.Count()? {
        //         println!("Adding mu updates to collection");
        //         let update = updates_mu.get_Item(i)?;
        //         let is_installed = update.IsInstalled()?.as_bool();
        //         println!("Checking update: {}", update.Title()?.to_string());
        //         println!("=>  IsInstalled: {:?}", is_installed);
        //         println!("=>  IsMandatory: {:?}", update.IsMandatory()?.as_bool());
        //         println!("=>  IsHidden: {:?}", update.IsHidden()?.as_bool());
        //         println!("=>  AutoSelectOnWebSites: {:?}", update.AutoSelectOnWebSites()?.as_bool());
        //         println!("=>  IsDownloaded: {:?}", update.IsDownloaded()?.as_bool());
        //         println!("=>  Description: {:?}", update.Description()?);
        //         if !is_installed && !update.IsDownloaded()?.as_bool() {
        //             println!("Adding update: {}", update.Title()?.to_string());
        //             update_collection.Add(&update).inspect_err(|e| println!("Add Err: {e:?}"))?;
        //         } else {
        //             println!("Skipping update: {}", update.Title()?.to_string());
        //         }
        //     }

        //     update_downloader.SetIsForced(true.into()).inspect_err(|e| println!("SetIsForced Failed: {e:?}"))?;
        //     let total_updates = update_downloader
        //         .Updates()
        //         .inspect_err(|e| println!("Updates() Failed: {e:?}"))?
        //         .Count()
        //         .inspect_err(|e| println!("Count(): {e:?}"))?;

        //     println!("Total updates to install: {total_updates:?}");

        //     update_downloader
        //     .SetUpdates(&update_collection)
        //     .inspect_err(|e| 
        //         println!("SetUpdates Err: {:?}", WindowsUpdateError::from(e.code()))
        //     )?;

        //     // Begin downloading updates
        //     println!("Beginning Downloads of Microsoft Updates...");
        //     let download_job: IDownloadJob = update_downloader.BeginDownload(
        //         None, 
        //         None, 
        //         &VARIANT::default()
        //     ).inspect_err(|e| 
        //         println!("BeginDownload Err: {:?}", WindowsUpdateError::from(e.code()))
        //     )?;
            
        //     println!("Started DownloadJob");
        //     // Monitor download progress
        //     while download_job.IsCompleted()?.as_bool() == false {
        //         let progress = download_job.GetProgress()?;
        //         println!("Download Progress: {}%", progress.PercentComplete()?);
        //         std::thread::sleep(std::time::Duration::from_secs(5));
        //     }
            
        //     // Print update results
        //     for i in 0..all_updates.len() {
        //         let result = download_job.GetProgress()?.GetUpdateResult(i as i32)?;
        //         let code = result.ResultCode()?;
        //         println!("Update {i} result code: {code:?}");
        //     }
        //     download_job.CleanUp()?;
        // }

        println!("Uninitializing COM...");
        drop(update_searcher);
        drop(update_session);
        CoUninitialize();
    };
    println!("Process completed.");
    Ok(())
}




/*** 
 **** ServerSelection enumeration
 *
 * typedef enum  { 
 *   ssDefault        = 0,
 *   ssManagedServer  = 1,
 *   ssWindowsUpdate  = 2,
 *   ssOthers         = 3
 * } ServerSelection;
 * Constants
 * ssDefault
 * Used only by IUpdateSearcher. Indicates that the search call should search the default server.
 * 
 * The default server used by the Windows Update Agent (WUA) is the same as ssMangagedServer if the computer is set up to have a managed server. If the computer is not been set up to have a managed server, WUA uses the first update service for which the IsRegisteredWithAU property of IUpdateService is VARIANT_TRUE and the IsManaged property of IUpdateService is VARIANT_FALSE
 * 
 * ssManagedServer
 * Indicates the managed server, in an environment that uses Windows Server Update Services or a similar corporate update server to manage the computer.
 * 
 * ssWindowsUpdate
 * Indicates the Windows Update service.
 * 
 * ssOthers
 * Indicates some update service other than those listed previously. If the ServerSelection property of a Windows Update Agent API object is set to ssOthers, then the ServiceID property of the object contains the ID of the service.
 * 
 * 
 * 
 **** IUpdateSearcher::Search method (wuapi.h)
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
 