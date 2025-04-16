use crossbeam::channel::Sender;
use windows_core::*;
use windows::{
    core::{
        implement, Ref, Result, BSTR, GUID, HRESULT, PCWSTR
    },
    Win32::{
        Foundation::{HANDLE, VARIANT_TRUE}, 
        Security::{
            AdjustTokenPrivileges, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED, 
            SE_SHUTDOWN_NAME, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY
        }, 
        System::{
            Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED}, 
            Shutdown::{self, ExitWindowsEx, EWX_FORCE, EWX_REBOOT}, 
            Threading::OpenProcessToken, 
            UpdateAgent::{
                IDownloadCompletedCallback_Impl, IDownloadJob, IDownloadProgressChangedCallbackArgs, IDownloadProgressChangedCallback_Impl, IUpdateCollection, IUpdateDownloader, IUpdateSearcher, IUpdateServiceManager, IUpdateSession, OperationResultCode, ServerSelection, UpdateType
            }, 
            Variant::VARIANT
        }
    }
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
const DCAT_SERVICE_ID: &str = "855E8A7C-ECB4-4CA3-B045-1DFA50104289";
const STORE_SERVICE_ID: &str = "117cab2d-82b1-4b5a-a08c-4d62dbee7782";

/// Our custom struct to hold basic update info
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct UpdateInfo {
    pub title: String,
    pub is_installed: bool,
    pub is_downloaded: bool,
    pub description: String,
}

/// A container for multiple updates
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct WindowsUpdates {
    pub updates: Vec<UpdateInfo>,
}

#[derive(Default)]
#[implement(windows::Win32::System::UpdateAgent::IDownloadProgressChangedCallback)]
pub struct DummyProgressCallback {}

#[derive(Default)]
#[implement(windows::Win32::System::UpdateAgent::IDownloadCompletedCallback)]
pub struct DummyCompletedCallback {}

#[derive(Debug)]
pub enum WindowsUpdateEvent {
    UpdateLogs(String),
    ReturnedUpdates(WindowsUpdates),
}


impl IDownloadProgressChangedCallback_Impl for DummyProgressCallback_Impl {
    fn Invoke(
        &self,
        downloadjob: Ref<'_, IDownloadJob>,
        _callbackargs: Ref<'_, IDownloadProgressChangedCallbackArgs>,
    ) -> Result<()> {
        unsafe {
            let progress = downloadjob.unwrap().GetProgress()?.PercentComplete()?;
            let cb = _callbackargs.unwrap();
            let total_downloaded = cb.Progress()?.TotalBytesDownloaded()?.Hi32;
            let total_download_size = cb.Progress()?.TotalBytesToDownload()?.Hi32;
            log::info!("Update Download progress: {progress:?}\ntotal_downloaded: {total_downloaded:?} / total_download_size: {total_download_size:?}");
        }
        Ok(())
    }
}

impl IDownloadCompletedCallback_Impl for DummyCompletedCallback_Impl {
    fn Invoke(
        &self, 
        _downloadjob: windows_core::Ref<'_, IDownloadJob>, 
        _callbackargs: windows_core::Ref<'_, windows::Win32::System::UpdateAgent::IDownloadCompletedCallbackArgs>
    ) -> windows_core::Result<()> {
        Ok(())
    }
}

pub fn install_windows_updates(event_sender: Sender<WindowsUpdateEvent>, _shutdown: bool, install: bool) -> Result<()> {
    let mut installed_updates = WindowsUpdates::default();
    
    unsafe {
        enable_privilege(PCWSTR::from_raw(SE_SHUTDOWN_NAME.as_ptr()));

        event_sender.try_send(WindowsUpdateEvent::UpdateLogs("Initializing COM...".to_string())).ok();
        let x = CoInitializeEx(Some(std::ptr::null_mut()), COINIT_MULTITHREADED).map(|| {});
        event_sender.try_send(WindowsUpdateEvent::UpdateLogs(format!("COM init: {x:?}"))).ok();
        event_sender.try_send(WindowsUpdateEvent::UpdateLogs("Ensuring Microsoft Update is enabled...".to_string())).ok();
        match ensure_microsoft_update_enabled() {
            Ok(_) => log::info!("Microsoft Update enabled."),
            Err(e) => log::info!("Error enabling Update Services: {e:?}"),
        }

        // Create an update session
        event_sender.try_send(WindowsUpdateEvent::UpdateLogs("Creating Update Session...".to_string())).ok();

        let update_session: IUpdateSession = CoCreateInstance(
            &CLSID_UPDATE_SESSION, 
            None, 
            CLSCTX_INPROC_SERVER
        )?;
        

        let update_searcher: IUpdateSearcher = update_session.CreateUpdateSearcher()?;

        event_sender.try_send(WindowsUpdateEvent::UpdateLogs("Searching Windows Update...".to_string())).ok();

        let services = [
            ("Windows Update", ServerSelection(2), None),
            ("Microsoft Update", ServerSelection(3), Some(MICROSOFT_UPDATE_SERVICE_ID)),
            ("Driver Catalog (DCAT)", ServerSelection(3), Some(DCAT_SERVICE_ID)),
            // ("Microsoft Store", ServerSelection(3), Some(STORE_SERVICE_ID)),
        ];

        for (name, selection, service_id) in services.iter() {
            event_sender.try_send(WindowsUpdateEvent::UpdateLogs(format!("Searching {}...", name))).ok();
            let updates = search_updates(&update_searcher, *selection, *service_id)
                .inspect_err(|e| {
                    event_sender.try_send(WindowsUpdateEvent::UpdateLogs(format!(
                        "{} error: {:?} - {:?}", name, WindowsUpdateError::from(e.code()), e.code()
                    ))).ok();
                })?;

            installed_updates.append_from_collection(&updates)?;

            event_sender.try_send(WindowsUpdateEvent::UpdateLogs(format!(
                "{} updates found: {} items", name, updates.Count()?
            ))).ok();

            if install && updates.Count()? > 0 {
                let res = process_updates(&update_session, &updates, event_sender.clone());
                event_sender.try_send(WindowsUpdateEvent::UpdateLogs(format!("{} result: {res:?}", name))).ok();
            }
        }

        drop(update_searcher);
        drop(update_session);
        CoUninitialize();
    };

    event_sender.try_send(WindowsUpdateEvent::UpdateLogs("Process completed.".to_string())).ok();
    event_sender.try_send(WindowsUpdateEvent::ReturnedUpdates(installed_updates)).ok();

    Ok(())
}

/// Enables required privileges for performing update operations
unsafe fn enable_privilege(privilege: PCWSTR) -> bool {
    unsafe { 
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
}

/// Checks if Microsoft Update is enabled and enables it if necessary
unsafe fn ensure_microsoft_update_enabled() -> Result<()> {
    unsafe { 
        log::info!("Checking if Microsoft Update is enabled...");
        let service_manager: IUpdateServiceManager = CoCreateInstance(&CLSID_UPDATE_SERVICE_MANAGER, None, CLSCTX_INPROC_SERVER)?;
        let services = service_manager.Services()?;
        let count = services.Count()?;
        
        for i in 0..count {
            let service = services.get_Item(i)?;
            let service_id = service.ServiceID()?;
            log::info!("Checking service ID: {:?}", service_id.to_string());
        }
        
        log::info!("Adding Service MICROSOFT_UPDATE_SERVICE_ID");
        service_manager.AddService(&BSTR::from(MICROSOFT_UPDATE_SERVICE_ID),  &BSTR::from(""))?;
        log::info!("MICROSOFT_UPDATE_SERVICE_ID has been successfully enabled.");

        log::info!("Adding Service DCAT_SERVICE_ID");
        service_manager.AddService(&BSTR::from(DCAT_SERVICE_ID),  &BSTR::from(""))?;
        log::info!("DCAT_SERVICE_ID has been successfully enabled.");

        log::info!("Adding Service STORE_SERVICE_ID");
        service_manager.AddService(&BSTR::from(STORE_SERVICE_ID),  &BSTR::from(""))?;
        log::info!("STORE_SERVICE_ID has been successfully enabled.");

        Ok(())
    }
}

/// Searches for available updates using the specified update server
unsafe fn search_updates(
    update_searcher: &IUpdateSearcher,
    selection: ServerSelection,
    service_id: Option<&str>,
) -> Result<IUpdateCollection> {
    unsafe { 

        let name = match (selection, service_id) {
            (ServerSelection(2), None) => "Windows Update",
            (ServerSelection(3), Some(id)) if id == MICROSOFT_UPDATE_SERVICE_ID => "Microsoft Update",
            (ServerSelection(3), Some(id)) if id == DCAT_SERVICE_ID => "Driver Catalog (DCAT)",
            (ServerSelection(3), Some(id)) if id == STORE_SERVICE_ID => "Microsoft Store",
            _ => "Unknown Service",
        };

        log::info!("ServerSelection: {name}\nServerSelection ID: {service_id:?}");
        update_searcher.SetServerSelection(selection)?;
        log::info!("update_searcher.SetOnline(VARIANT_TRUE)?;");
        update_searcher.SetOnline(VARIANT_TRUE)?; // Ensure online search
        // log::info!("update_searcher.SetCanAutomaticallyUpgradeService(VARIANT_TRUE)?;");
        update_searcher.SetIncludePotentiallySupersededUpdates(VARIANT_TRUE)?;
        
        if let Some(id) = service_id {
            log::info!("service_id: {id}");
            update_searcher.SetServiceID(&BSTR::from(id))?;
        }

        log::info!("QUERY => \"IsInstalled=0 and (Type='Software' or Type='Driver')\"");
        
        let search_result = update_searcher.Search(&BSTR::from(
            "(IsInstalled=0) or (IsHidden=1 and IsInstalled=0)"
        ))?;

        let update_result = search_result.Updates()?;
        log::info!("update_result: {update_result:?}");
        for i in (0..update_result.Count()?).rev() {
            let update = update_result.get_Item(i)?;
            if update.IsInstalled()?.as_bool() {
                log::info!("Update already installed, removing: {:?}", update.Title()?);
                update_result.RemoveAt(i)?;
            } else {
                let update_type = update.Type()?;
                log::info!(
                    "Adding update to collection: {:?} (Type: {})",
                    update.Title()?,
                    if update_type == UpdateType(1) { "Software" } else { "Driver" }
                );
            }
        }

        Ok(update_result)
    }
}

/// Handles installation of updates from a given update collection
unsafe fn install_updates_from_collection(
    update_session: &IUpdateSession, 
    updates: &IUpdateCollection,
    dummy_progress_cb: DummyProgressCallback,
    dummy_completed_cb: DummyCompletedCallback,
) -> Result<bool> {
    unsafe { 
        let update_downloader: IUpdateDownloader = update_session.CreateUpdateDownloader()?;
        update_downloader.SetUpdates(updates)?;
        log::info!("Beginning download of updates...");
        let async_result = VARIANT::default();
        let download_job: IDownloadJob = update_downloader
            .BeginDownload(
                Some(&dummy_progress_cb.into()), 
                Some(&dummy_completed_cb.into()), 
                &async_result
            )
            .inspect_err(|e| 
                log::info!("BeginDownload Err => {e:?}")
            )?;

        while !download_job.IsCompleted()?.as_bool() {
            let progress = download_job.GetProgress()?;
            log::info!("Download Progress: {}%", progress.PercentComplete()?);
            // std::thread::sleep(std::time::Duration::from_secs(5));
        }
        download_job.CleanUp()?;
        log::info!("Download completed. Installing updates...");

        let installer = update_session.CreateUpdateInstaller()?;
        installer.SetUpdates(updates)?;
        let install_result = installer.Install()?;
        log::info!("--------- Installation result ---------");
        match install_result.ResultCode()? {
            OperationResultCode(0) => log::info!("orcNotStarted"),
            OperationResultCode(1) => log::info!("orcInProgress"),
            OperationResultCode(2) => log::info!("orcSucceeded"),
            OperationResultCode(3) => log::info!("orcSucceededWithErrors"),
            OperationResultCode(4) => log::info!("orcFailed"),
            OperationResultCode(5) => log::info!("orcAborted"),
            _ => {}
        }   
        
        // update_downloader.EndDownload(value)
        Ok(install_result.RebootRequired()?.as_bool())
    }
}

pub fn reboot_system() -> Result<()> {
    unsafe {
        // EWX_REBOOT specifies that the system should reboot.
        // EWX_FORCE forces all running applications to close.
        if let Err(e) = ExitWindowsEx(EWX_REBOOT | EWX_FORCE, Shutdown::SHUTDOWN_REASON(0)) {
            log::info!("{e:?}");
            return Err(Error::from_win32());
        }
    }
    Ok(())
}

/// Filters and installs updates separately for Windows Update and Microsoft Update
unsafe fn process_updates(update_session: &IUpdateSession, update_collection: &IUpdateCollection, event_sender: Sender<WindowsUpdateEvent>) -> Result<()> {
    unsafe { 
        // Iterate backwards to avoid shifting indices during removal
        let count = update_collection.Count()?;
        for i in (0..count).rev() {
            let update = update_collection.get_Item(i)?;
            let is_installed = update.IsInstalled()?.as_bool();
            // Remove only if the update is already installed.
            if is_installed {
                event_sender.try_send(WindowsUpdateEvent::UpdateLogs(format!("Skipping update (already installed): {}", update.Title()?.to_string()))).ok();
                update_collection.RemoveAt(i)?;
            } else {
                event_sender.try_send(WindowsUpdateEvent::UpdateLogs(format!("Adding update: {}", update.Title()?.to_string()))).ok();
                event_sender.try_send(WindowsUpdateEvent::UpdateLogs(format!("=>  IsInstalled: {:?}", is_installed))).ok();
                event_sender.try_send(WindowsUpdateEvent::UpdateLogs(format!("=>  IsMandatory: {:?}", update.IsMandatory()?.as_bool()))).ok();
                event_sender.try_send(WindowsUpdateEvent::UpdateLogs(format!("=>  IsHidden: {:?}", update.IsHidden()?.as_bool()))).ok();
                event_sender.try_send(WindowsUpdateEvent::UpdateLogs(format!("=>  AutoSelectOnWebSites: {:?}", update.AutoSelectOnWebSites()?.as_bool()))).ok();
                event_sender.try_send(WindowsUpdateEvent::UpdateLogs(format!("=>  Description: {:?}", update.Description()?))).ok();
            }
        }
        if update_collection.Count()? != 0 {
            let shutdown_required = install_updates_from_collection(
                update_session, 
                &update_collection,
                DummyProgressCallback::default().into(),
                DummyCompletedCallback::default().into()
            );

            event_sender.try_send(WindowsUpdateEvent::UpdateLogs(format!("Shutdown Required: {}", shutdown_required?))).ok();
        }
        
        Ok(())
    }
}

impl WindowsUpdates {
    /// Create a new `WindowsUpdates` from an existing `IUpdateCollection`
    pub unsafe fn new(collection: &IUpdateCollection) -> Result<Self> {
        unsafe { 
            let mut wu = WindowsUpdates::default();
            wu.append_from_collection(collection)?;
            Ok(wu)
        }
    }

    /// Append updates from an additional `IUpdateCollection` to this object
    pub unsafe fn append_from_collection(&mut self, collection: &IUpdateCollection) -> Result<&mut Self> {
        unsafe { 
            for i in 0..collection.Count()? {
                let update = collection.get_Item(i)?;
                let title = update.Title()?.to_string();
                let is_installed = update.IsInstalled()?.as_bool();
                let is_downloaded = update.IsDownloaded()?.as_bool();
                let description = update.Description()?.to_string();

                self.updates.push(UpdateInfo {
                    title,
                    is_installed,
                    is_downloaded,
                    description,
                });
            }
            Ok(self)
        }
    }
}

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
    Other(String),
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
            0x80070020 => WindowsUpdateError::Other("InstallFileLocked => Couldn't access the file because it is already in use. This can occur when the installer tries to replace a file that an antivirus, antimalware or backup program is currently scanning.".to_string()),
            0x80240002 => WindowsUpdateError::Other("WU_E_MAX_CAPACITY_REACHED => The maximum capacity of the service was exceeded.".to_string()),
            0x80240004 => WindowsUpdateError::Other("WU_E_NOT_INITIALIZED => The object couldn't be initialized.".to_string()),
            0x80240005 => WindowsUpdateError::Other("WU_E_RANGEOVERLAP => The update handler requested a byte range overlapping a previously requested range.".to_string()),
            0x80240006 => WindowsUpdateError::Other("WU_E_TOOMANYRANGES => The requested number of byte ranges exceeds the maximum number (2^31 - 1).".to_string()),
            0x80240008 => WindowsUpdateError::Other("WU_E_ITEMNOTFOUND => The key for the item queried couldn't be found.".to_string()),
            0x8024000A => WindowsUpdateError::Other("WU_E_COULDNOTCANCEL => Cancellation of the operation wasn't allowed.".to_string()),
            0x8024000B => WindowsUpdateError::Other("WU_E_CALL_CANCELLED => Operation was canceled.".to_string()),
            0x8024000C => WindowsUpdateError::Other("WU_E_NOOP => No operation was required.".to_string()),
            0x8024000D => WindowsUpdateError::Other("WU_E_XML_MISSINGDATA => Windows Update Agent couldn't find required information in the update's XML data.".to_string()),
            0x8024000E => WindowsUpdateError::Other("WU_E_XML_INVALID => Windows Update Agent found invalid information in the update's XML data.".to_string()),
            0x8024000F => WindowsUpdateError::Other("WU_E_CYCLE_DETECTED => Circular update relationships were detected in the metadata.".to_string()),
            0x80240010 => WindowsUpdateError::Other("WU_E_TOO_DEEP_RELATION => Update relationships too deep to evaluate were evaluated.".to_string()),
            0x80240011 => WindowsUpdateError::Other("WU_E_INVALID_RELATIONSHIP => An invalid update relationship was detected.".to_string()),
            0x80240012 => WindowsUpdateError::Other("WU_E_REG_VALUE_INVALID => An invalid registry value was read.".to_string()),
            0x80240013 => WindowsUpdateError::Other("WU_E_DUPLICATE_ITEM => Operation tried to add a duplicate item to a list.".to_string()),
            0x80240016 => WindowsUpdateError::Other("WU_E_INSTALL_NOT_ALLOWED => Operation tried to install while another installation was in progress or the system was pending a mandatory restart.".to_string()),
            0x80240018 => WindowsUpdateError::Other("WU_E_NO_USERTOKEN => Operation failed because a required user token is missing.".to_string()),
            0x80240019 => WindowsUpdateError::Other("WU_E_EXCLUSIVE_INSTALL_CONFLICT => An exclusive update can't be installed with other updates at the same time.".to_string()),
            0x8024001A => WindowsUpdateError::Other("WU_E_POLICY_NOT_SET => A policy value wasn't set.".to_string()),
            0x8024001B => WindowsUpdateError::Other("WU_E_SELFUPDATE_IN_PROGRESS => The operation couldn't be performed because the Windows Update Agent is self-updating.".to_string()),
            0x8024001D => WindowsUpdateError::Other("WU_E_INVALID_UPDATE => An update contains invalid metadata.".to_string()),
            0x8024001E => WindowsUpdateError::Other("WU_E_SERVICE_STOP => Operation didn't complete because the service or system was being shut down.".to_string()),
            0x8024001F => WindowsUpdateError::Other("WU_E_NO_CONNECTION => Operation didn't complete because the network connection was unavailable.".to_string()),
            0x80240020 => WindowsUpdateError::Other("WU_E_NO_INTERACTIVE_USER => Operation didn't complete because there's no logged-on interactive user.".to_string()),
            0x80240021 => WindowsUpdateError::Other("WU_E_TIME_OUT => Operation didn't complete because it timed out.".to_string()),
            0x80240022 => WindowsUpdateError::Other("WU_E_ALL_UPDATES_FAILED => Operation failed for all the updates.".to_string()),
            0x80240023 => WindowsUpdateError::Other("WU_E_EULAS_DECLINED => The license terms for all updates were declined.".to_string()),
            0x80240024 => WindowsUpdateError::Other("WU_E_NO_UPDATE => There are no updates.".to_string()),
            0x80240025 => WindowsUpdateError::Other("WU_E_USER_ACCESS_DISABLED => Group Policy settings prevented access to Windows Update.".to_string()),
            0x80240026 => WindowsUpdateError::Other("WU_E_INVALID_UPDATE_TYPE => The type of update is invalid.".to_string()),
            0x80240027 => WindowsUpdateError::Other("WU_E_URL_TOO_LONG => The URL exceeded the maximum length.".to_string()),
            0x80240028 => WindowsUpdateError::Other("WU_E_UNINSTALL_NOT_ALLOWED => The update couldn't be uninstalled because the request didn't originate from a WSUS server.".to_string()),
            0x80240029 => WindowsUpdateError::Other("WU_E_INVALID_PRODUCT_LICENSE => Search may have missed some updates before there's an unlicensed application on the system.".to_string()),
            0x8024002A => WindowsUpdateError::Other("WU_E_MISSING_HANDLER => A component required to detect applicable updates was missing.".to_string()),
            0x8024002B => WindowsUpdateError::Other("WU_E_LEGACYSERVER => An operation didn't complete because it requires a newer version of server.".to_string()),
            0x8024002C => WindowsUpdateError::Other("WU_E_BIN_SOURCE_ABSENT => A delta-compressed update couldn't be installed because it required the source.".to_string()),
            0x8024002D => WindowsUpdateError::Other("WU_E_SOURCE_ABSENT => A full-file update couldn't be installed because it required the source.".to_string()),
            0x8024002E => WindowsUpdateError::Other("WU_E_WU_DISABLED => Access to an unmanaged server isn't allowed.".to_string()),
            0x8024002F => WindowsUpdateError::Other("WU_E_CALL_CANCELLED_BY_POLICY => Operation didn't complete because the DisableWindowsUpdateAccess policy was set.".to_string()),
            0x80240030 => WindowsUpdateError::Other("WU_E_INVALID_PROXY_SERVER => The format of the proxy list was invalid.".to_string()),
            0x80240031 => WindowsUpdateError::Other("WU_E_INVALID_FILE => The file is in the wrong format.".to_string()),
            0x80240032 => WindowsUpdateError::Other("WU_E_INVALID_CRITERIA => The search criteria string was invalid.".to_string()),
            0x80240033 => WindowsUpdateError::Other("WU_E_EULA_UNAVAILABLE => License terms couldn't be downloaded.".to_string()),
            0x80240035 => WindowsUpdateError::Other("WU_E_UPDATE_NOT_PROCESSED => The update wasn't processed.".to_string()),
            0x80240038 => WindowsUpdateError::Other("WU_E_WINHTTP_INVALID_FILE => The downloaded file has an unexpected content type.".to_string()),
            0x80240039 => WindowsUpdateError::Other("WU_E_TOO_MANY_RESYNC => Agent is asked by server to resync too many times.".to_string()),
            0x80240040 => WindowsUpdateError::Other("WU_E_NO_SERVER_CORE_SUPPORT => WUA API method doesn't run on Server Core installation.".to_string()),
            0x80240041 => WindowsUpdateError::Other("WU_E_SYSPREP_IN_PROGRESS => Service isn't available while sysprep is running.".to_string()),
            0x80240042 => WindowsUpdateError::Other("WU_E_UNKNOWN_SERVICE => The update service is no longer registered with AU.".to_string()),
            0x80240043 => WindowsUpdateError::Other("WU_E_NO_UI_SUPPORT => There's no support for WUA UI.".to_string()),
            0x80240FFF => WindowsUpdateError::Other("WU_E_UNEXPECTED => An operation failed due to reasons not covered by another error code.".to_string()),
            _ => WindowsUpdateError::Other(format!("Unknown: {hr:?}")) // WindowsUpdateError::Other(hr.0 as u32),
        }
    }
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
