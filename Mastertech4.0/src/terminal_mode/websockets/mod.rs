#![allow(unused)]
use database::{schema::{utilities::{check_id_existence, query_id}, ConnectedClient, CONNECTED_CLIENT_TABLE}, websocket_url_with_room, db, WS_CLIENT_URL, WS_CLIENT_URL_LOCAL};
use displays::{deserialize_command, remote_viewer::{encode_buffer_with_timestamp, ratagui::TerminalEvent}, serialize_system_info, tabs::admin_console::client_action::ClientHandler, Cmd, EventLogEntry, FileSystemAction, RegistryEdit, RegistryKeyInfo, RegistryValueEntry, RemoteDirEntry, RemoteScriptItem, RemoteScriptStatus, ScheduledTask, ServiceActionType, StartupApp, WindowsService};
use crate::{filesystem::{get_client_hash, system_info::{get_sysinfo, get_sysinfo_no_gpu}}, tabs::file_browser::read_folder, transport::ClientTransport};
use std::{path::Path, time::{Duration, Instant}};
use command::{handle_windows_cmd_interactive, PersistentShell};
use bincode::{config::standard, serde::*};
use ewebsock::{WsEvent, WsMessage};
use ratatui::buffer::Buffer;

use super::{data::LocalTermEvent, TerminalApp};

pub mod command;

const FILE_CHUNK_SIZE: usize = 4 * 1024 * 1024;

/// Stream a file from disk to the master in `FILE_CHUNK_SIZE` chunks over the
/// bounded file channel. Each `send().await` blocks when the channel is full,
/// pacing reads to the socket so we never buffer the whole file in RAM.
async fn stream_file_download(
    path: &str,
    file_tx: tokio::sync::mpsc::Sender<crate::transport::TcpFrame>,
) -> anyhow::Result<()> {
    use tokio::io::AsyncReadExt;

    let mut file = tokio::fs::File::open(path).await?;
    let total = file.metadata().await?.len();
    let mut sent: u64 = 0;
    let mut buf = vec![0u8; FILE_CHUNK_SIZE];

    // Empty file: still send a single terminal chunk so the master finalizes.
    if total == 0 {
        let frame = Cmd::FileChunk(Vec::new(), true);
        let payload = encode_to_vec(&frame, standard())?;
        file_tx
            .send(crate::transport::TcpFrame::Binary(payload))
            .await
            .map_err(|_| anyhow::anyhow!("writer channel closed"))?;
        return Ok(());
    }

    let mut sent_terminal = false;
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        sent += n as u64;
        let is_last = sent >= total;
        let frame = Cmd::FileChunk(buf[..n].to_vec(), is_last);
        let payload = encode_to_vec(&frame, standard())?;
        file_tx
            .send(crate::transport::TcpFrame::Binary(payload))
            .await
            .map_err(|_| anyhow::anyhow!("writer channel closed"))?;
        if is_last {
            sent_terminal = true;
            break;
        }
    }
    // File was shorter than its metadata (truncated mid-read): send an empty
    // terminal chunk so the master finalizes instead of waiting forever.
    if !sent_terminal {
        let frame = Cmd::FileChunk(Vec::new(), true);
        let payload = encode_to_vec(&frame, standard())?;
        file_tx
            .send(crate::transport::TcpFrame::Binary(payload))
            .await
            .map_err(|_| anyhow::anyhow!("writer channel closed"))?;
    }
    log::info!("Streamed {sent} bytes for {path}");
    Ok(())
}

fn send_file_chunks(data: Vec<u8>, sender: &mut ClientTransport) {
    let chunks: Vec<&[u8]> = if data.len() > FILE_CHUNK_SIZE {
        data.chunks(FILE_CHUNK_SIZE).collect()
    } else {
        vec![data.as_slice()]
    };
    let total_chunks = chunks.len();
    for (i, chunk) in chunks.into_iter().enumerate() {
        let is_last = i + 1 == total_chunks;
        let response = Cmd::FileChunk(chunk.to_vec(), is_last);
        match encode_to_vec(&response, standard()) {
            Ok(payload) => {
                log::info!(
                    "Sending chunk {}/{} ({} bytes)",
                    i + 1,
                    total_chunks,
                    payload.len()
                );
                sender.send(WsMessage::Binary(payload));
            }
            Err(e) => {
                log::error!("Failed to serialize file chunk {i}: {e}");
                sender.send(WsMessage::Text(format!(
                    "Error: Failed to serialize chunk {i} - {e}"
                )));
                return;
            }
        }
    }
    if total_chunks > 1 {
        log::info!("All {total_chunks} chunks sent successfully");
    } else {
        log::info!("File chunk sent successfully");
    }
}

/// Which custom stress plan a `RunRemoteScenario` / `RunRemoteConcurrent` carries.
enum RemoteStressPlanRequest {
    Scenario {
        stages: Vec<displays::RemoteScenarioStage>,
        total_wall_secs: Option<u64>,
        repeat_until_total: bool,
    },
    Concurrent {
        lanes: Vec<displays::RemoteScenarioStage>,
        duration_secs: u64,
    },
}

/// Map a wire stage to a `stress_runner::RunStage`; errors on an unknown stressor name.
fn remote_stage_to_run_stage(
    s: &displays::RemoteScenarioStage,
    concurrent: bool,
) -> Result<stress_runner::RunStage, String> {
    let stressor = stress_runner::Stressor::from_str(&s.stressor).ok_or_else(|| {
        format!(
            "unknown stressor '{}'. valid: {}",
            s.stressor,
            stress_runner::Stressor::labels_csv()
        )
    })?;
    Ok(stress_runner::RunStage {
        label: s.label.clone().unwrap_or_else(|| s.stressor.clone()),
        stressor,
        threads: s.threads,
        duration_secs: if concurrent { 0 } else { s.duration_secs },
        memory_cap_mb: s.memory_cap_mb.unwrap_or(if concurrent { 1024 } else { 256 }),
        disk_file_mb: s.disk_file_mb.unwrap_or(512),
    })
}

/// Drive a custom scenario/concurrent stress plan on this client and stream
/// RemoteScriptLog/RemoteScriptResult/RemoteScriptsComplete back to the admin.
fn run_remote_stress_plan(
    tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    request: RemoteStressPlanRequest,
    service_number: Option<String>,
    diagnostic_session_id: Option<String>,
    preset_label: Option<String>,
    notes: Option<String>,
) {
    use stress_runner::{RunPlan, RunResult, RunSpec, RunUpdate, TargetKind, TestTool};

    let send_log = |tx: &tokio::sync::mpsc::UnboundedSender<Vec<u8>>, msg: String| {
        if let Ok(payload) = encode_to_vec(&Cmd::RemoteScriptLog(msg), standard()) {
            let _ = tx.send(payload);
        }
    };
    let send_result =
        |tx: &tokio::sync::mpsc::UnboundedSender<Vec<u8>>, name: &str, status: RemoteScriptStatus| {
            if let Ok(payload) =
                encode_to_vec(&Cmd::RemoteScriptResult { name: name.to_string(), status }, standard())
            {
                let _ = tx.send(payload);
            }
        };
    let send_complete = |tx: &tokio::sync::mpsc::UnboundedSender<Vec<u8>>| {
        if let Ok(payload) = encode_to_vec(&Cmd::RemoteScriptsComplete, standard()) {
            let _ = tx.send(payload);
        }
    };

    let (concurrent, result_name) = match &request {
        RemoteStressPlanRequest::Scenario { .. } => (false, displays::REMOTE_SCENARIO_RESULT_NAME),
        RemoteStressPlanRequest::Concurrent { .. } => (true, displays::REMOTE_CONCURRENT_RESULT_NAME),
    };

    let service_number = service_number.unwrap_or_default();
    if service_number.trim().is_empty() {
        send_log(
            &tx,
            format!(
                "{result_name}: service_number is required so stress_test_run carries service_order / customer / computer linkage — aborting."
            ),
        );
        send_result(&tx, result_name, RemoteScriptStatus::Failed);
        send_complete(&tx);
        return;
    }

    let stages_result: Result<Vec<stress_runner::RunStage>, String> = match &request {
        RemoteStressPlanRequest::Scenario { stages, .. } => {
            if stages.is_empty() || stages.len() > 16 {
                Err("provide 1-16 stages".to_string())
            } else {
                stages.iter().map(|s| remote_stage_to_run_stage(s, false)).collect()
            }
        }
        RemoteStressPlanRequest::Concurrent { lanes, .. } => {
            if lanes.is_empty() || lanes.len() > 8 {
                Err("provide 1-8 concurrent lanes".to_string())
            } else {
                lanes.iter().map(|s| remote_stage_to_run_stage(s, true)).collect()
            }
        }
    };
    let stages = match stages_result {
        Ok(s) => s,
        Err(e) => {
            send_log(&tx, format!("{result_name}: {e}"));
            send_result(&tx, result_name, RemoteScriptStatus::Failed);
            send_complete(&tx);
            return;
        }
    };

    let plan = match &request {
        RemoteStressPlanRequest::Scenario { total_wall_secs, repeat_until_total, .. } => {
            RunPlan::Scenario {
                stages: stages.clone(),
                total_wall_secs: *total_wall_secs,
                repeat_until_total: *repeat_until_total,
            }
        }
        RemoteStressPlanRequest::Concurrent { duration_secs, .. } => RunPlan::Concurrent {
            lanes: stages.clone(),
            duration_secs: Some(*duration_secs),
        },
    };

    let target_kind = if concurrent {
        TargetKind::System
    } else {
        let kinds: Vec<TargetKind> = stages
            .iter()
            .map(|s| stress_runner::default_target_kind(s.stressor))
            .collect();
        if kinds.windows(2).all(|w| w[0] == w[1]) {
            kinds.first().copied().unwrap_or(TargetKind::Mixed)
        } else {
            TargetKind::Mixed
        }
    };

    let preset = preset_label.unwrap_or_else(|| {
        if concurrent {
            "mcp:concurrent-remote-v1".to_string()
        } else {
            "mcp:scenario-remote-v1".to_string()
        }
    });
    let preset_tag = if concurrent { "preset:concurrent" } else { "preset:scenario" };

    let name_owned = result_name.to_string();
    tokio::spawn(async move {
        send_log(&tx, format!("{name_owned}: running custom stress plan via stress-runner (persisted)"));

        enum PlanMsg {
            Log(String),
            Done(bool),
        }
        let (plan_tx, plan_rx) = crossbeam::channel::unbounded::<PlanMsg>();

        std::thread::spawn(move || {
            use std::sync::Arc;
            use stress_kit::telemetry::TelemetryAgent;

            let client = crate::filesystem::get_client_hash();
            let computer = match client.computer.clone() {
                Some(c) => c,
                None => {
                    let _ = plan_tx.send(PlanMsg::Log(
                        "get_client_hash returned no computer record".into(),
                    ));
                    let _ = plan_tx.send(PlanMsg::Done(false));
                    return;
                }
            };

            let mut spec = RunSpec::single_stresskit(
                computer,
                stages.first().map(|s| s.stressor).unwrap_or(stress_runner::Stressor::Cpu),
                None,
            );
            spec.plan = plan;
            spec.target_kind = target_kind;
            spec.tool = TestTool::StressKitScenario { name: Some(preset.clone()) };
            spec.tech = Some("mcp".to_string());
            spec.notes = notes;
            spec.preset_label = Some(preset.clone());
            spec.tags = vec!["origin:mcp".into(), "origin:remote_scripts".into(), preset_tag.into()];
            spec.hostname = std::env::var("COMPUTERNAME")
                .or_else(|_| std::env::var("HOSTNAME"))
                .ok();
            spec.machine_id = Some(client.client_hash.clone());
            spec.service_order = Some(database::schema::RecordId::new(
                database::schema::TICKET_TABLE,
                service_number,
            ));
            if let Some(sess) = diagnostic_session_id.filter(|s| !s.trim().is_empty()) {
                spec.session_ref = Some(database::schema::entity_link::parse_record_id(
                    &sess,
                    database::schema::DIAGNOSTIC_SESSION_TABLE,
                ));
            }

            let telemetry = Arc::new(TelemetryAgent::start(1000));
            let mut success = false;
            stress_runner::drive_blocking(spec, telemetry, |update| match update {
                RunUpdate::Started { run_id } => {
                    use database::schema::RecordIdExt;
                    let _ = plan_tx.send(PlanMsg::Log(format!(
                        "stress_test_run id: {}",
                        run_id.key_string()
                    )));
                }
                RunUpdate::StageStarted { index, label, stage_count } => {
                    let _ = plan_tx.send(PlanMsg::Log(format!(
                        "Stage {}/{}: {label}",
                        index + 1,
                        stage_count
                    )));
                }
                RunUpdate::Tick { metrics, stage_label, .. } => {
                    if let Some(err) = metrics.last_error.as_ref() {
                        let stage = stage_label.unwrap_or_else(|| "run".into());
                        let _ = plan_tx.send(PlanMsg::Log(format!("{stage}: {err}")));
                    }
                }
                RunUpdate::StageFinished { .. } => {}
                RunUpdate::StageVerdict { index, label, pass, violations, .. } => {
                    let _ = plan_tx.send(PlanMsg::Log(format!(
                        "stage {} '{label}': {}",
                        index + 1,
                        if pass { "PASS" } else { "FAIL" }
                    )));
                    for violation in violations {
                        let _ = plan_tx.send(PlanMsg::Log(format!(
                            "stage {} violation: {violation}",
                            index + 1
                        )));
                    }
                }
                RunUpdate::Finished(v) => {
                    success = v.result == RunResult::Pass;
                    let result_str = match v.result {
                        RunResult::Pass => "PASSED",
                        RunResult::Fail => "FAILED",
                        RunResult::Aborted => "ABORTED",
                        RunResult::Inconclusive => "INCONCLUSIVE",
                        RunResult::InProgress => "IN_PROGRESS",
                    };
                    let _ = plan_tx.send(PlanMsg::Log(format!(
                        "{result_str} in {:.1}s (run persisted)",
                        v.duration_secs
                    )));
                }
                RunUpdate::Warning { message } => {
                    let _ = plan_tx.send(PlanMsg::Log(format!("warning: {message}")));
                }
                RunUpdate::Error { message } => {
                    let _ = plan_tx.send(PlanMsg::Log(format!("error: {message}")));
                }
            });
            let _ = plan_tx.send(PlanMsg::Done(success));
        });

        let mut final_success: Option<bool> = None;
        while final_success.is_none() {
            while let Ok(msg) = plan_rx.try_recv() {
                match msg {
                    PlanMsg::Log(line) => send_log(&tx, line),
                    PlanMsg::Done(ok) => final_success = Some(ok),
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        send_result(
            &tx,
            &name_owned,
            if final_success.unwrap_or(false) {
                RemoteScriptStatus::Success
            } else {
                RemoteScriptStatus::Failed
            },
        );
        send_complete(&tx);
    });
}

fn scan_directory_size(root: &Path) -> Result<(u64, u64, u64), String> {
    use walkdir::WalkDir;

    if !root.is_dir() {
        return Err("Path is not a directory".to_string());
    }
    let mut total_bytes = 0u64;
    let mut file_count = 0u64;
    let mut dir_count = 0u64;
    for entry in WalkDir::new(root).into_iter() {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_type().is_dir() {
            dir_count += 1;
        } else if let Ok(m) = entry.metadata() {
            file_count += 1;
            total_bytes = total_bytes.saturating_add(m.len());
        }
    }
    Ok((total_bytes, file_count, dir_count))
}

fn zip_directory(dir_path: &Path) -> Result<Vec<u8>, String> {
    use std::io::Write;
    use walkdir::WalkDir;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    if !dir_path.is_dir() {
        return Err("Path is not a directory".to_string());
    }
    let mut buffer = Vec::new();
    {
        let mut zip = ZipWriter::new(std::io::Cursor::new(&mut buffer));
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for entry in WalkDir::new(dir_path).into_iter() {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.is_dir() {
                continue;
            }
            let name = path.strip_prefix(dir_path).map_err(|e| e.to_string())?;
            let name_str = name.to_string_lossy().replace('\\', "/");
            zip.start_file(name_str, options)
                .map_err(|e| e.to_string())?;
            let data = std::fs::read(path).map_err(|e| e.to_string())?;
            zip.write_all(&data).map_err(|e| e.to_string())?;
        }
        zip.finish().map_err(|e| e.to_string())?;
    }
    Ok(buffer)
}

/// Every Windows kernel crash dump on this machine: MEMORY.DMP plus every
/// `.dmp` under Minidump and LiveKernelReports.
fn enumerate_crash_dumps() -> Vec<std::path::PathBuf> {
    use walkdir::WalkDir;
    let mut out = Vec::new();
    let memdmp = Path::new(r"C:\Windows\MEMORY.DMP");
    if memdmp.is_file() {
        out.push(memdmp.to_path_buf());
    }
    for dir in [r"C:\Windows\Minidump", r"C:\Windows\LiveKernelReports"] {
        let d = Path::new(dir);
        if !d.is_dir() {
            continue;
        }
        for e in WalkDir::new(d).into_iter().filter_map(|e| e.ok()) {
            let p = e.path();
            if p.is_file()
                && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("dmp"))
            {
                out.push(p.to_path_buf());
            }
        }
    }
    out
}

/// Zip all Windows crash dumps (MEMORY.DMP + Minidump\* + LiveKernelReports\*)
/// into a temp file, streaming each source through the deflate encoder so a
/// multi-GB MEMORY.DMP never lands in RAM. Returns the temp zip path.
fn build_crash_dump_zip() -> Result<std::path::PathBuf, String> {
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let out_path = std::env::temp_dir().join(format!("mtech-crashdumps-{}.zip", std::process::id()));
    let file = std::fs::File::create(&out_path).map_err(|e| e.to_string())?;
    let mut zip = ZipWriter::new(std::io::BufWriter::new(file));
    let options =
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let mut added: u32 = 0;

    // MEMORY.DMP (full/kernel/automatic dump).
    let memdmp = Path::new(r"C:\Windows\MEMORY.DMP");
    if memdmp.is_file() {
        if let Ok(mut f) = std::fs::File::open(memdmp) {
            if zip.start_file("MEMORY.DMP", options).is_ok()
                && std::io::copy(&mut f, &mut zip).is_ok()
            {
                added += 1;
            }
        }
    }
    // Minidump\* (BSOD triage dumps) and LiveKernelReports\** (watchdog live dumps).
    add_dir_to_zip(&mut zip, Path::new(r"C:\Windows\Minidump"), "Minidump", options, &mut added);
    add_dir_to_zip(
        &mut zip,
        Path::new(r"C:\Windows\LiveKernelReports"),
        "LiveKernelReports",
        options,
        &mut added,
    );

    zip.finish().map_err(|e| e.to_string())?;
    log::info!("Crash-dump zip built with {added} file(s): {}", out_path.display());
    Ok(out_path)
}

/// Stream every file under `dir` into `zip` beneath `prefix`.
fn add_dir_to_zip<W: std::io::Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    dir: &Path,
    prefix: &str,
    options: zip::write::SimpleFileOptions,
    added: &mut u32,
) {
    use walkdir::WalkDir;
    if !dir.is_dir() {
        return;
    }
    for entry in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let rel = path.strip_prefix(dir).unwrap_or(path);
        let name = format!("{prefix}/{}", rel.to_string_lossy().replace('\\', "/"));
        if let Ok(mut f) = std::fs::File::open(path) {
            if zip.start_file(name, options).is_ok() && std::io::copy(&mut f, zip).is_ok() {
                *added += 1;
            }
        }
    }
}

/// Resolve special folder paths using Windows API or fallback to environment variables
#[cfg(target_os = "windows")]
fn resolve_special_path(path: &str) -> String {
    // Check for special folder keywords
    let lower_path = path.to_lowercase();
    
    // Try to use Windows API for known special folders
    if let Ok(user_data) = windows::Storage::UserDataPaths::GetDefault() {
        let resolved = match lower_path.as_str() {
            "desktop" | "%userprofile%\\desktop" => user_data.Desktop().ok().map(|p| p.to_string()),
            "documents" | "%userprofile%\\documents" => user_data.Documents().ok().map(|p| p.to_string()),
            "downloads" | "%userprofile%\\downloads" => user_data.Downloads().ok().map(|p| p.to_string()),
            "pictures" | "%userprofile%\\pictures" => user_data.Pictures().ok().map(|p| p.to_string()),
            "music" | "%userprofile%\\music" => user_data.Music().ok().map(|p| p.to_string()),
            "videos" | "%userprofile%\\videos" => user_data.Videos().ok().map(|p| p.to_string()),
            "appdata" | "%appdata%" => user_data.RoamingAppData().ok().map(|p| p.to_string()),
            "localappdata" | "%localappdata%" => user_data.LocalAppData().ok().map(|p| p.to_string()),
            _ => None,
        };
        
        if let Some(resolved_path) = resolved {
            log::info!("Resolved '{}' to '{}'", path, resolved_path);
            return resolved_path;
        }
    }
    
    // Fallback to environment variable expansion
    expand_env_vars(path)
}

#[cfg(not(target_os = "windows"))]
fn resolve_special_path(path: &str) -> String {
    expand_env_vars(path)
}

/// Expand environment variables like %USERPROFILE%, $HOME, etc.
fn expand_env_vars(path: &str) -> String {
    let mut result = path.to_string();
    
    // Windows-style environment variables
    let env_vars = [
        "USERPROFILE", "HOME", "APPDATA", "LOCALAPPDATA", 
        "TEMP", "TMP", "USERNAME", "HOMEDRIVE", "HOMEPATH",
        "PROGRAMFILES", "PROGRAMFILES(X86)", "SYSTEMROOT",
        "WINDIR", "SYSTEMDRIVE"
    ];
    
    for var_name in env_vars {
        let pattern = format!("%{}%", var_name);
        if result.contains(&pattern) {
            if let Ok(value) = std::env::var(var_name) {
                result = result.replace(&pattern, &value);
            }
        }
    }
    
    // Unix-style environment variables (e.g., $HOME)
    if result.starts_with('$') {
        let var_name = result.trim_start_matches('$').split('/').next().unwrap_or("");
        if let Ok(value) = std::env::var(var_name) {
            result = result.replacen(&format!("${}", var_name), &value, 1);
        }
    }
    
    result
}

pub struct TerminalWebsocketClient {
    // explorer: FileSystem,
    pub bin_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    pub bin_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    pub command_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    pub command_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    // process: Arc<Mutex<Option<ChildStdin>>>,
    pub interactive_input_tx: tokio::sync::mpsc::UnboundedSender<String>,
    pub interactive_input_rx: tokio::sync::mpsc::UnboundedReceiver<String>,
    pub client: ConnectedClient,
    pub live_stats_stop_tx: Option<tokio::sync::watch::Sender<bool>>,
    pub sysinfo_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    pub sysinfo_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    pub join_handle: Option<tokio::task::JoinHandle<()>>,
    pub persistent_shell: Option<PersistentShell>,
    /// Accumulates chunks for direct file transfers: filename → (total_chunks, received_chunks_data)
    pub file_transfer_buffers: std::collections::HashMap<String, (u32, Vec<(u32, Vec<u8>)>)>,
    /// Accumulates incoming self-update binary chunks from the admin console.
    pub self_update_buffer: crate::remote_self_update::SelfUpdateBuffer,
}

/// Returns `true` when `command` is one of the bare-text control-plane
/// signals the admin sends out-of-band (terminal viewer readiness,
/// presence beacons, status pings) rather than something the user typed
/// into the shell.  The single source of truth so the WebSocket relay
/// path and the direct-TCP path filter the same set; if you add a new
/// sentinel to `start_websocket_sender`'s text branch, add it here too.
fn is_control_plane_sentinel(command: &str) -> bool {
    matches!(
        command,
        "READY"
            | "MASTER_CONNECTED"
            | "MASTER_DISCONNECTED"
            | "CLIENT_CONNECTED"
            | "CLIENT_DISCONNECTED"
    ) || command.starts_with("MASTER_STATUS:")
        || command.starts_with("CLIENT_STATUS:")
}

impl TerminalWebsocketClient {
    // Constructor to initialize the client
    pub fn new() -> Self {
        let (bin_tx, bin_rx) = tokio::sync::mpsc::unbounded_channel();
        let (command_tx, command_rx) = tokio::sync::mpsc::unbounded_channel();
        let (interactive_input_tx, interactive_input_rx) = tokio::sync::mpsc::unbounded_channel();
        let (sysinfo_tx, sysinfo_rx) = tokio::sync::mpsc::unbounded_channel();
        // let process = Arc::new(Mutex::new(None));


        Self {
            bin_tx, bin_rx,
            sysinfo_tx, sysinfo_rx,
            client: get_client_hash(),
            // process,
            command_tx,
            command_rx,
            interactive_input_tx,
            interactive_input_rx,
            live_stats_stop_tx: None,
            join_handle: None,
            persistent_shell: None,
            file_transfer_buffers: std::collections::HashMap::new(),
            self_update_buffer: Default::default(),
        }
    }
    
    /// Route a plain-text shell command from the admin over the TCP path.
    ///
    /// Mirrors the `WsMessage::Text` branch in `start_websocket_sender`:
    /// lazily creates a `PersistentShell` on the first call and sends
    /// subsequent commands to the running instance.  Output is piped through
    /// `command_tx` which the TCP session loop (`run_session_loop`) drains via
    /// `client.command_rx` and forwards back to the admin as binary frames
    /// (which the admin side displays as text in the shell history panel).
    ///
    /// Filters out the same control-plane sentinels the WebSocket relay path
    /// handles inline (`READY`, `MASTER_CONNECTED`, etc.) — see
    /// [`is_control_plane_sentinel`] — so they don't get executed as shell
    /// commands when delivered over direct TCP.
    pub async fn handle_text_command(&mut self, command: String) {
        if command.is_empty() {
            return;
        }
        // Drop control-plane sentinels before they reach the shell.
        //
        // The admin side sends a handful of bare text frames as in-band
        // signals: `READY` (terminal viewer ready for buffers),
        // `MASTER_CONNECTED` / `MASTER_DISCONNECTED`,
        // `CLIENT_CONNECTED` / `CLIENT_DISCONNECTED`, and the
        // `MASTER_STATUS:` / `CLIENT_STATUS:` prefixed variants.  Over the
        // WebSocket relay path these are filtered up in
        // `start_websocket_sender` before this function is called.  Over
        // the direct-TCP path the listener calls us directly with the
        // raw text frame, so without this filter the literal string
        // "READY" gets piped into PowerShell stdin and the user sees
        // `'READY' is not recognized as the name of a cmdlet…`.
        //
        // Doing it here (single chokepoint) instead of in tcp_listener.rs
        // means every future transport is protected automatically.
        if is_control_plane_sentinel(&command) {
            log::debug!("handle_text_command: ignoring sentinel {command:?}");
            return;
        }
        if self.persistent_shell.is_none() {
            let mut shell = PersistentShell::new(self.command_tx.clone());
            match shell.start().await {
                Ok(()) => {
                    self.persistent_shell = Some(shell);
                }
                Err(e) => {
                    log::error!("tcp: failed to start persistent shell: {e}");
                    return;
                }
            }
        }
        if let Some(shell) = &mut self.persistent_shell {
            if let Err(e) = shell.send_command(command).await {
                log::error!("tcp: persistent shell send failed: {e}");
                self.persistent_shell = None;
            }
        }
    }

    // Migrated start_websocket_sender function
    pub async fn start_websocket_sender(
        &mut self,
        mut buffer_rx: tokio::sync::mpsc::UnboundedReceiver<(usize, Buffer)>,
        start_tx: tokio::sync::mpsc::UnboundedSender<bool>,
        connection_state_tx: tokio::sync::mpsc::UnboundedSender<(bool, String)>,
        event_tx: tokio::sync::mpsc::UnboundedSender<LocalTermEvent>,
        mut shutdown_rx: tokio::sync::broadcast::Receiver<()>,
    ) 
        -> anyhow::Result<()> 
    {
        let connection_url = websocket_url_with_room(
            if cfg!(debug_assertions) {
                WS_CLIENT_URL_LOCAL
            } else {
                WS_CLIENT_URL
            },
            &self.client.connection_string,
            "client",
        );

        // After a drop (e.g. network driver during Windows Update), reconnect instead of spinning on a dead sender.
        const RECONNECT_DELAY: Duration = Duration::from_secs(5);
        // Keepalive: ping cadence and the silence window after which the socket is presumed dead.
        const PING_INTERVAL: Duration = Duration::from_secs(10);
        const LIVENESS_TIMEOUT: Duration = Duration::from_secs(45);
        // Stop after this many consecutive reconnects; reset on a successful Open.
        const MAX_RECONNECT_ATTEMPTS: u32 = 5;
        let mut reconnect_attempts: u32 = 0;

        'ws_session: loop {
            if reconnect_attempts > MAX_RECONNECT_ATTEMPTS {
                log::error!("start_websocket_sender -> giving up after {MAX_RECONNECT_ATTEMPTS} reconnect attempts");
                let _ = connection_state_tx.send((false, format!("Disconnected — gave up after {MAX_RECONNECT_ATTEMPTS} reconnect attempts")));
                return Ok(());
            }
            let connection = ewebsock::connect(connection_url.clone(), ewebsock::Options::default());

            match connection {
                Ok((ws_sender, receiver)) => {
                    let mut last_event_at = Instant::now();
                    let mut last_ping_at = Instant::now();
                    // Wrap the raw `WsSender` in our transport-agnostic
                    // `ClientTransport`. Existing `sender.send(WsMessage::...)`
                    // call sites inside `handle_command` work unchanged
                    // because the wrapper preserves the same `send(WsMessage)`
                    // method shape — see `Mastertech4.0/src/transport.rs`.
                    let mut sender = ClientTransport::WebSocket(ws_sender);
                    let ready = &mut false;
                    log::info!("start_websocket_sender -> connecting");
                    loop {
                        let mut socket_lost = false;

                    // Handle WebSocket events (e.g., READY or TerminalEvent from egui)
                    while let Some(event) = receiver.try_recv() {
                        // log::info!("Received WebSocket event: {:?}", event);
                        // update client to connected = false in db
                        last_event_at = Instant::now();
                        match event {
                            WsEvent::Opened => {
                                log::info!("start_websocket_sender -> Connection Opened");
                                reconnect_attempts = 0;
                                let _ = connection_state_tx.send((true, "Connected".to_string()));
                            },
                            WsEvent::Error(e) => { 
                                log::error!("start_websocket_sender -> Error: {e:?}");
                                let _ = connection_state_tx.send((false, format!("{e:?}"))); 
                                let _ = start_tx.send(false);
                                *ready = false;
                                self.persistent_shell = None;
                                socket_lost = true;
                                break;
                            },
                            WsEvent::Closed => { 
                                log::info!("start_websocket_sender -> Connection Closed — will reconnect");
                                let _ = connection_state_tx.send((false, "Disconnected".to_string())); 
                                let _ = start_tx.send(false);
                                *ready = false;
                                self.persistent_shell = None;
                                socket_lost = true;
                                break;
                            },
                            WsEvent::Message(ws_message) => {
                                match ws_message {
                                    WsMessage::Pong(_) => { let _ = connection_state_tx.send((true, "Pong".to_string())); },
                                    WsMessage::Text(txt) => {
                                        // Handle master presence notifications
                                        if txt == "MASTER_CONNECTED" {
                                            log::info!("Master connected - resuming data transmission");
                                            let _ = connection_state_tx.send((true, "Master Connected".to_string()));
                                            // If we were waiting for master, mark as ready
                                            if !*ready {
                                                let _ = start_tx.send(true);
                                                *ready = true;
                                            }
                                            continue;
                                        } else if txt == "MASTER_DISCONNECTED" {
                                            log::info!("Master disconnected - pausing data transmission");
                                            let _ = connection_state_tx.send((true, "Master Disconnected - Waiting...".to_string()));
                                            // Don't set ready to false - keep the connection alive
                                            // but the render system will check master_connected before sending
                                            continue;
                                        } else if txt == "CLIENT_CONNECTED" || txt == "CLIENT_DISCONNECTED" {
                                            // These are for master-side, ignore on client
                                            continue;
                                        } else if txt.starts_with("MASTER_STATUS:") || txt.starts_with("CLIENT_STATUS:") {
                                            // Activity status is now tracked via SurrealDB, not websocket messages
                                            // Ignore these to prevent them from being executed as shell commands
                                            continue;
                                        }
                                        
                                        if !*ready && txt == "READY".to_string() {
                                            let _ = start_tx.send(true);
                                            *ready = true;
                                            log::info!("WebSocket sender marked as ready");
                                        } else if *ready && txt != "READY".to_string() {
                                            log::info!("GOT TEXT: {txt:?}");
                                            // Check if we need to start a persistent shell
                                            if self.persistent_shell.is_none() {
                                                log::error!("persistent_shell IS NONE");
                                                let shell = PersistentShell::new(
                                                    self.command_tx.clone()
                                                );
                                                self.persistent_shell = Some(shell);
                                                
                                                if let Some(shell) = &mut self.persistent_shell {
                                                    log::error!("persistent_shell IS SOME");
                                                    if let Err(e) = shell.start().await {
                                                        log::error!("Failed to start persistent shell: {}", e);
                                                        // Fallback to old method
                                                        let tx = self.command_tx.clone();
                                                        let (new_input_tx, new_input_rx) = tokio::sync::mpsc::unbounded_channel();
                                                        let handle_windows_cmd_interactive = handle_windows_cmd_interactive(
                                                            txt, 
                                                            tx, 
                                                            new_input_rx
                                                        ).await;
                                                        log::info!("start_websocket_sender -> handle_windows_cmd_interactive: {handle_windows_cmd_interactive:?}");
                                                        self.interactive_input_tx = new_input_tx.clone();
                                                        self.persistent_shell = None;
                                                    } else {
                                                        log::error!("STARTED persistent_shell");
                                                        // Send the command to the persistent shell
                                                        if let Err(e) = shell.send_command(txt).await {
                                                            log::error!("Failed to send command to persistent shell: {}", e);
                                                        }
                                                    }
                                                }
                                            } else {
                                                log::error!("USING persistent_shell");
                                                // Use existing persistent shell
                                                if let Some(shell) = &mut self.persistent_shell {
                                                    if let Err(e) = shell.send_command(txt).await {
                                                        log::error!("Failed to send command to persistent shell: {}", e);
                                                        // Reset shell on error
                                                        self.persistent_shell = None;
                                                    }
                                                }
                                            }
                                        }
                                    },
                                    WsMessage::Binary(bin) => {
                                        if *ready {
                                            // Deserialize incoming TerminalEvent from egui and forward to rendering loop
                                            if let Ok(event) = serde_json::from_slice::<TerminalEvent>(&bin) {
                                                log::info!("Received TerminalEvent from egui: {:?}", event);
                                                if event_tx.send(event.into()).is_ok() {
                                                    log::info!("Forwarded TerminalEvent to rendering loop");
                                                } else {
                                                    log::warn!("Failed to forward TerminalEvent to rendering loop");
                                                }
                                            } else {
                                                self.handle_command(deserialize_command(&bin.clone()), &mut sender).await;
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            },
                        }
                    }
    
                    if let Ok(()) = shutdown_rx.try_recv() {
                        self.client.disconnect_client();
                        *ready = false;
                        self.persistent_shell = None;
                        return Ok(());
                    }

                    if socket_lost {
                        reconnect_attempts += 1;
                        log::info!("start_websocket_sender -> reconnecting (attempt {reconnect_attempts}/{MAX_RECONNECT_ATTEMPTS}) after {RECONNECT_DELAY:?}...");
                        tokio::time::sleep(RECONNECT_DELAY).await;
                        continue 'ws_session;
                    }
    
                    if *ready {
                        tokio::select! {
                            Some((frame_count, buffer)) = buffer_rx.recv() => {
                                log::debug!("Sending buffer, frame_count={}", frame_count);
                                let send_start = Instant::now();
                                let serialized = encode_buffer_with_timestamp(frame_count as u64, &buffer)?;
                                sender.send(WsMessage::Binary(serialized));
                                let send_duration = send_start.elapsed();
                                log::debug!("Buffer sent, frame_count={}, send_duration={:?}", frame_count, send_duration);
                            }
                            Some(cmd_output) = self.command_rx.recv() => {
                                sender.send(WsMessage::Binary(cmd_output));
                            }
                            Some(sysinfo) = self.sysinfo_rx.recv() => {
                                sender.send(WsMessage::Binary(sysinfo));
                            }
                            // Wake periodically so the event drain and keepalive run even with no outbound traffic.
                            _ = tokio::time::sleep(PING_INTERVAL) => {}
                        }
                    }

                    // Keepalive: pong replies refresh last_event_at; prolonged silence means the
                    // socket died without a Close/Error event (half-open TCP), so force a redial.
                    if last_ping_at.elapsed() >= PING_INTERVAL {
                        sender.send(WsMessage::Ping(Vec::new()));
                        last_ping_at = Instant::now();
                    }
                    if last_event_at.elapsed() >= LIVENESS_TIMEOUT {
                        reconnect_attempts += 1;
                        log::warn!("start_websocket_sender -> no socket events for {LIVENESS_TIMEOUT:?}; reconnecting (attempt {reconnect_attempts}/{MAX_RECONNECT_ATTEMPTS})");
                        let _ = connection_state_tx.send((false, "Connection silent — reconnecting".to_string()));
                        let _ = start_tx.send(false);
                        *ready = false;
                        self.persistent_shell = None;
                        tokio::time::sleep(RECONNECT_DELAY).await;
                        continue 'ws_session;
                    }

                    // Let other tasks run before looping again // THIS IS REQUIRED, or else the
                    // server will not actually receive an Open event, and will terminate the loop
                    // immediately. we need to give some CPU time to yield, allowing the websocket
                    // handshake to complete
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                }
                Err(e) => {
                    reconnect_attempts += 1;
                    log::error!("Failed to establish WebSocket connection (attempt {reconnect_attempts}/{MAX_RECONNECT_ATTEMPTS}): {e:?}");
                    let _ = connection_state_tx.send((false, format!("Connect failed: {e:?}")));
                    let _ = start_tx.send(false);
                    tokio::time::sleep(RECONNECT_DELAY).await;
                }
            }
        }
        #[allow(unreachable_code)]
        Ok(())
    }

    pub async fn handle_command(&mut self, cmd: Cmd, sender: &mut ClientTransport) {
        #[cfg(target_os = "windows")]
        match cmd {
            Cmd::FileSystemAction(FileSystemAction::RequestNewContents(new_path)) => {
                let path = if new_path == "current" {
                    let current_path = std::env::current_dir().unwrap_or_default();
                    log::info!("websockets -> Current_path: {current_path:?}");
                    current_path
                } else {
                    Path::new(&new_path).to_path_buf()
                };
                if path.is_dir() {
                    let paths = read_folder(&path, 1, false);
                    // info!("websockets -> Paths: {:?}", paths.clone());
                    if paths.len() > 0 {
                        // let node = self.explorer.build_virtual_file_system(path, paths);
                        // info!("websockets -> Node: {:?}", node);
    
                        // let payload = serialize(
                        //     &Cmd::FileSystemAction(FileSystemAction::GetNode(node))
                        // );
        
                        // match payload {
                        //     Ok(bytes) => self.ws_sender.send(WsMessage::Binary(bytes)),
                        //     Err(e) => error!("Error serializing paths: {e:?}"),
                        // }
                    }
                } else { sender.send(WsMessage::Text(format!("{new_path} is not a directory"))); }
            },
            Cmd::FileSystemAction(FileSystemAction::Execute(path)) => {
                let tx = self.bin_tx.clone();
                let p = path.clone();
                log::info!("websockets -> executing: {path:?}");
                let (new_input_tx, new_input_rx) = tokio::sync::mpsc::unbounded_channel();
                let handle_windows_cmd_interactive = handle_windows_cmd_interactive(
                    p, tx, 
                    new_input_rx
                ).await;

                self.interactive_input_tx = new_input_tx.clone();
                log::info!("websockets -> handle_windows_cmd_interactive: {handle_windows_cmd_interactive:?}");
            },
            Cmd::FileSystemAction(FileSystemAction::CopyFromClient(_path)) => {

            }
            Cmd::DesktopStreamStart { monitor, fps, quality, scale } => {
                log::info!("websockets -> DesktopStreamStart monitor={monitor} fps={fps} quality={quality} scale={scale}");
                crate::remote_desktop::start_desktop_stream(monitor, fps, quality, scale);
            }
            Cmd::DesktopStreamStop => {
                log::info!("websockets -> DesktopStreamStop");
                crate::remote_desktop::stop_desktop_stream();
            }
            Cmd::DesktopListMonitors => {
                let monitors = crate::remote_desktop::enumerate_monitors();
                let response = Cmd::DesktopMonitorList(monitors);
                if let Ok(payload) = encode_to_vec(&response, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }
            Cmd::FileSystemAction(FileSystemAction::CopyToClient(_minio_path)) => {
                // self.explorer
            } // self.explorer.previewed_file = Some(String::from_utf8(byte_vec.clone()));
            Cmd::FileSystemAction(FileSystemAction::Select((_, path))) => {
                match std::fs::read_to_string(path) {
                    Ok(file) => {
                        let payload = encode_to_vec(
                            &Cmd::FileSystemAction(FileSystemAction::PreviewedFile(file)),
                            standard()
                        );
        
                        match payload {
                            Ok(bytes) => sender.send(WsMessage::Binary(bytes)),
                            Err(e) => log::error!("Error serializing paths: {e:?}"),
                        };
                    },
                    Err(e) => {let _ = self.bin_tx.send(format!("Error with file preview: {e:?}").as_bytes().to_vec());},
                };
            }
            Cmd::FileSystemAction(FileSystemAction::Delete(path)) => {
                let tx = self.bin_tx.clone();
                log::info!("websockets -> deleting: {path:?}");
                let path = Path::new(&path);
                if !path.is_dir() {
                    let remove_dir = tokio::fs::remove_dir_all(path).await;
                    let _ = match remove_dir {
                        Ok(_) => tx.send("Removed Directory".as_bytes().to_vec()),
                        Err(e) => tx.send(format!("Error removing path: {e:?}").as_bytes().to_vec()),
                    };
                } else {
                    let remove_file = tokio::fs::remove_file(path).await;
                    let _ = match remove_file {
                        Ok(_) => tx.send("Removed Path".as_bytes().to_vec()),
                        Err(e) => tx.send(format!("Error removing path: {e:?}").as_bytes().to_vec()),
                    };
                }
            }
            Cmd::InteractiveInput(cmd) => {
                if cmd.ends_with("tron.bat") {
                    let path = Path::new(&cmd);
                    if path.exists() {
                        let whitelist = if cfg!(target_os="windows") {
                            path.join("tron\\resources\\stage_0_prep\\processkiller\\whitelist.txt")
                        } else { path.join("tron/resources/stage_0_prep/processkiller/whitelist.txt") };

                        if whitelist.exists() {

                        }
                    }
                } else {

                }

                // Send to persistent shell if available, otherwise use the old method
                if let Some(shell) = &mut self.persistent_shell {
                    let shell_cmd = cmd.clone();
                    let _shell_ptr = shell as *mut PersistentShell;
                    // Use a more direct approach to avoid lifetime issues
                    if let Err(e) = shell.send_command(shell_cmd).await {
                        log::error!("Failed to send interactive input to persistent shell: {}", e);
                        // Fallback to old method
                        let _ = self.interactive_input_tx.send(cmd);
                    }
                } else {
                    let _ = self.interactive_input_tx.send(cmd);
                }
            },
            Cmd::ReadEvents => {},
            Cmd::QuitInteractive => {
                if let Some(shell) = self.persistent_shell.take() {
                    let mut shell = shell;
                    tokio::spawn(async move {
                        if let Err(e) = shell.close().await {
                            log::error!("Failed to close persistent shell: {}", e);
                        }
                    });
                } else {
                    let _ = self.interactive_input_tx.send("quit".to_string());
                }
            },
            Cmd::LiveData => {
                // If already running, do nothing
                if self.join_handle.is_some() {
                    log::info!("websockets -> LiveData already running, ignoring request");
                    return;
                }
                log::info!("websockets -> Starting live stats task");
                let tx = self.sysinfo_tx.clone();
                let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
                self.live_stats_stop_tx = Some(stop_tx);
                self.join_handle = Some(tokio::spawn(async move {
                    let res = live_computer_stats(tx, stop_rx).await;
                    log::info!("live_computer_stats completed: {res:?}");
                }));
            }
            Cmd::TaskManager => todo!(),
            // Cmd::UninstallProgram(_) => todo!(),
            // Cmd::PullKeys(_) => todo!(),
            // Cmd::PullTicket(_) => todo!(),
            Cmd::Quit => {
                log::info!("websockets -> Received Cmd::Quit, stopping live stats");
                // Signal the live stats task to stop and await it
                if let Some(stop_tx) = self.live_stats_stop_tx.take() {
                    log::info!("websockets -> Sending stop signal to live stats task");
                    let _ = stop_tx.send(true);
                } else {
                    log::warn!("websockets -> No live stats stop channel found");
                }
                if let Some(handle) = self.join_handle.take() {
                    log::info!("websockets -> Waiting for live stats task to complete");
                    let _ = handle.await;
                    log::info!("websockets -> Live stats task completed");
                } else {
                    log::warn!("websockets -> No live stats join handle found");
                }
            }
            Cmd::KillProcess(pid) => {
                log::info!("websockets -> Killing process with PID: {}", pid);
                #[cfg(target_os = "windows")]
                {
                    let output = tokio::process::Command::new("taskkill")
                        .args(["/F", "/PID", &pid.to_string()])
                        .output()
                        .await;
                    
                    match output {
                        Ok(out) => {
                            if out.status.success() {
                                log::info!("Successfully killed process {}", pid);
                            } else {
                                let stderr = String::from_utf8_lossy(&out.stderr);
                                log::error!("Failed to kill process {}: {}", pid, stderr);
                            }
                        }
                        Err(e) => log::error!("Error executing taskkill: {}", e),
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    let output = tokio::process::Command::new("kill")
                        .args(["-9", &pid.to_string()])
                        .output()
                        .await;
                    
                    match output {
                        Ok(out) => {
                            if out.status.success() {
                                log::info!("Successfully killed process {}", pid);
                            } else {
                                let stderr = String::from_utf8_lossy(&out.stderr);
                                log::error!("Failed to kill process {}: {}", pid, stderr);
                            }
                        }
                        Err(e) => log::error!("Error executing kill: {}", e),
                    }
                }
            }
            Cmd::OpenProcessInExplorer(path) => {
                log::info!("websockets -> Opening path in explorer: {}", path);
                #[cfg(target_os = "windows")]
                {
                    // Get parent directory if path is a file
                    let target_path = Path::new(&path);
                    let dir_path = if target_path.is_file() {
                        target_path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| target_path.to_path_buf())
                    } else {
                        target_path.to_path_buf()
                    };
                    
                    // Use explorer.exe to open and select the file
                    if target_path.exists() {
                        let _ = tokio::process::Command::new("explorer.exe")
                            .args(["/select,", &path])
                            .spawn();
                    } else {
                        // If file doesn't exist, just open the directory
                        let _ = tokio::process::Command::new("explorer.exe")
                            .arg(dir_path)
                            .spawn();
                    }
                }
                #[cfg(target_os = "macos")]
                {
                    let target_path = Path::new(&path);
                    if target_path.exists() {
                        let _ = tokio::process::Command::new("open")
                            .args(["-R", &path])
                            .spawn();
                    }
                }
                #[cfg(target_os = "linux")]
                {
                    let target_path = Path::new(&path);
                    let dir_path = if target_path.is_file() {
                        target_path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| target_path.to_path_buf())
                    } else {
                        target_path.to_path_buf()
                    };
                    let _ = tokio::process::Command::new("xdg-open")
                        .arg(dir_path)
                        .spawn();
                }
            }
            Cmd::ListDirectory(path_str) => {
                log::info!("websockets -> Listing directory: {}", path_str);
                
                // Resolve special folder paths using Windows API or expand environment variables
                let expanded_path = resolve_special_path(&path_str);
                
                // Determine the actual path to list
                let target_path = if path_str == "current" {
                    std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf())
                } else {
                    Path::new(&expanded_path).to_path_buf()
                };
                
                let resolved_path = target_path.to_string_lossy().to_string();
                let mut entries: Vec<RemoteDirEntry> = Vec::new();
                
                if target_path.is_dir() {
                    match std::fs::read_dir(&target_path) {
                        Ok(dir_iter) => {
                            for entry in dir_iter.flatten() {
                                let path = entry.path();
                                let name = entry.file_name().to_string_lossy().to_string();
                                let is_directory = path.is_dir();
                                let size = if is_directory {
                                    None
                                } else {
                                    entry.metadata().ok().map(|m| m.len())
                                };
                                let modified = entry.metadata().ok()
                                    .and_then(|m| m.modified().ok())
                                    .map(|t| {
                                        let datetime: chrono::DateTime<chrono::Local> = t.into();
                                        datetime.to_rfc3339()
                                    });
                                
                                entries.push(RemoteDirEntry {
                                    name,
                                    path: path.to_string_lossy().to_string(),
                                    is_directory,
                                    size,
                                    modified,
                                });
                            }
                        }
                        Err(e) => {
                            log::error!("Error reading directory: {}", e);
                        }
                    }
                }
                
                // Send the directory listing back with resolved path
                let response = Cmd::DirectoryListing(entries, Some(resolved_path));
                let payload = encode_to_vec(&response, standard()).expect("Failed to serialize DirectoryListing");
                sender.send(WsMessage::Binary(payload));
            }
            Cmd::GetDrives => {
                log::info!("websockets -> Getting drives");
                use sysinfo::Disks;
                
                let disks = Disks::new_with_refreshed_list();
                let drives: Vec<String> = disks.iter()
                    .filter_map(|disk| disk.mount_point().to_str().map(|s| s.to_string()))
                    .collect();
                
                log::info!("websockets -> Found {} drives: {:?}", drives.len(), drives);
                
                let response = Cmd::DriveList(drives);
                let payload = encode_to_vec(&response, standard()).expect("Failed to serialize DriveList");
                sender.send(WsMessage::Binary(payload));
            }
            Cmd::DownloadRemoteFile(path_str) => {
                log::info!("websockets -> Download request for: {}", path_str);

                let path = Path::new(&path_str);
                if !path.is_file() {
                    log::warn!("Path is not a file: {}", path_str);
                    sender.send(WsMessage::Text("Error: Path is not a file".to_string()));
                    return;
                }

                if let Some(file_tx) = sender.file_sender() {
                    // Direct TCP path: stream from disk on a separate task so
                    // the session loop keeps echoing pongs, and the bounded
                    // channel paces us to the socket (no whole-file RAM load).
                    let path_owned = path_str.clone();
                    tokio::spawn(async move {
                        if let Err(e) = stream_file_download(&path_owned, file_tx).await {
                            log::error!("File stream for {path_owned} failed: {e}");
                        }
                    });
                } else {
                    // Relay path (no bounded file channel): read in chunks from
                    // disk to avoid a whole-file RAM load, sent inline.
                    match std::fs::read(path) {
                        Ok(data) => {
                            log::info!("File read successfully, {} bytes", data.len());
                            send_file_chunks(data, sender);
                        }
                        Err(e) => {
                            log::error!("Error reading file for download: {}", e);
                            sender.send(WsMessage::Text(format!("Error: {}", e)));
                        }
                    }
                }
            }
            Cmd::DownloadCrashDumps => {
                log::info!("websockets -> crash-dump bundle download requested");
                if let Some(file_tx) = sender.file_sender() {
                    // TCP path: build the zip on disk (streamed), then stream it
                    // down and delete it. Off the session loop so pongs flow.
                    tokio::spawn(async move {
                        match tokio::task::spawn_blocking(build_crash_dump_zip).await {
                            Ok(Ok(zip_path)) => {
                                let p = zip_path.to_string_lossy().to_string();
                                if let Err(e) = stream_file_download(&p, file_tx).await {
                                    log::error!("Crash-dump stream failed: {e}");
                                }
                                let _ = tokio::fs::remove_file(&zip_path).await;
                            }
                            Ok(Err(e)) => log::error!("Crash-dump zip failed: {e}"),
                            Err(e) => log::error!("Crash-dump zip task panicked: {e}"),
                        }
                    });
                } else {
                    // Relay path: build on disk, read + send inline, then delete.
                    match build_crash_dump_zip() {
                        Ok(zip_path) => {
                            match std::fs::read(&zip_path) {
                                Ok(data) => send_file_chunks(data, sender),
                                Err(e) => sender.send(WsMessage::Text(format!("Error: {e}"))),
                            }
                            let _ = std::fs::remove_file(&zip_path);
                        }
                        Err(e) => sender.send(WsMessage::Text(format!("Error: {e}"))),
                    }
                }
            }
            Cmd::DownloadRemoteDirectory(path_str) => {
                log::info!("websockets -> Download directory request for: {}", path_str);
                let path = Path::new(&path_str);
                match zip_directory(path) {
                    Ok(data) => {
                        log::info!(
                            "Zipped directory {} ({} bytes), sending chunks",
                            path_str,
                            data.len()
                        );
                        send_file_chunks(data, sender);
                    }
                    Err(e) => {
                        log::error!("Error zipping directory {}: {}", path_str, e);
                        sender.send(WsMessage::Text(format!("Error: {e}")));
                    }
                }
            }
            Cmd::ScanDirectorySize(path_str) => {
                log::info!("websockets -> Scan directory size for: {}", path_str);
                let path = Path::new(&path_str);
                let response = match scan_directory_size(path) {
                    Ok((total_bytes, file_count, dir_count)) => Cmd::DirectorySizeResult {
                        path: path_str,
                        total_bytes,
                        file_count,
                        dir_count,
                        error: None,
                    },
                    Err(e) => Cmd::DirectorySizeResult {
                        path: path_str,
                        total_bytes: 0,
                        file_count: 0,
                        dir_count: 0,
                        error: Some(e),
                    },
                };
                match encode_to_vec(&response, standard()) {
                    Ok(payload) => sender.send(WsMessage::Binary(payload)),
                    Err(e) => sender.send(WsMessage::Text(format!("Error: {e}"))),
                }
            }
            Cmd::ExecuteRemoteFile(path_str) => {
                log::info!("websockets -> Execute request for: {}", path_str);
                let path = Path::new(&path_str);
                
                if path.exists() {
                    #[cfg(target_os = "windows")]
                    {
                        // Use ShellExecuteW to open/execute the file
                        let _ = tokio::process::Command::new("cmd")
                            .args(["/c", "start", "", &path_str])
                            .spawn();
                        log::info!("Executed file: {}", path_str);
                    }
                    #[cfg(target_os = "macos")]
                    {
                        let _ = tokio::process::Command::new("open")
                            .arg(&path_str)
                            .spawn();
                    }
                    #[cfg(target_os = "linux")]
                    {
                        let _ = tokio::process::Command::new("xdg-open")
                            .arg(&path_str)
                            .spawn();
                    }
                } else {
                    log::warn!("File does not exist: {}", path_str);
                    sender.send(WsMessage::Text(format!("Error: File not found: {}", path_str)));
                }
            }
            Cmd::PreviewRemoteFile(path_str) => {
                log::info!("websockets -> Preview request for: {}", path_str);
                let path = Path::new(&path_str);
                
                if path.is_file() {
                    // Check file size - don't preview huge files
                    let max_preview_size: u64 = 100 * 1024 * 1024;
                    
                    if let Ok(metadata) = std::fs::metadata(path) {
                        if metadata.len() > max_preview_size {
                            sender.send(WsMessage::Text(format!("Error: File too large for preview ({} MB)", metadata.len() / 1024 / 1024)));
                            return;
                        }
                    }
                    
                    match std::fs::read_to_string(path) {
                        Ok(content) => {
                            let response = Cmd::FilePreviewContent(path_str, content);
                            match encode_to_vec(&response, standard()) {
                                Ok(payload) => {
                                    sender.send(WsMessage::Binary(payload));
                                    log::info!("Sent file preview content");
                                }
                                Err(e) => {
                                    log::error!("Failed to serialize preview content: {}", e);
                                    sender.send(WsMessage::Text(format!("Error: {}", e)));
                                }
                            }
                        }
                        Err(e) => {
                            // May be binary - try reading as lossy UTF-8
                            if let Ok(bytes) = std::fs::read(path) {
                                let content = String::from_utf8_lossy(&bytes).to_string();
                                let response = Cmd::FilePreviewContent(path_str, content);
                                if let Ok(payload) = encode_to_vec(&response, standard()) {
                                    sender.send(WsMessage::Binary(payload));
                                    return;
                                }
                            }
                            log::error!("Error reading file for preview: {}", e);
                            sender.send(WsMessage::Text(format!("Error reading file: {}", e)));
                        }
                    }
                } else {
                    sender.send(WsMessage::Text("Error: Path is not a file".to_string()));
                }
            }
            Cmd::UploadToClient(dest_path, data) => {
                log::info!("websockets -> Upload to client: {} ({} bytes)", dest_path, data.len());
                
                match std::fs::write(&dest_path, &data) {
                    Ok(_) => {
                        log::info!("Successfully wrote file to: {}", dest_path);
                        let response = Cmd::SaveResult(true, format!("File saved: {}", dest_path));
                        if let Ok(payload) = encode_to_vec(&response, standard()) {
                            sender.send(WsMessage::Binary(payload));
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to write file: {}", e);
                        let response = Cmd::SaveResult(false, format!("Failed to save: {}", e));
                        if let Ok(payload) = encode_to_vec(&response, standard()) {
                            sender.send(WsMessage::Binary(payload));
                        }
                    }
                }
            }
            Cmd::RequestThumbnail(path_str) => {
                log::info!("websockets -> Thumbnail request for: {}", path_str);
                
                #[cfg(target_os = "windows")]
                {
                    use std::os::windows::ffi::OsStrExt;
                    use windows::{
                        Win32::{
                            Foundation::SIZE,
                            System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, IBindCtx},
                            UI::Shell::{IShellItem, IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF},
                            Graphics::Gdi::*,
                        },
                        core::{Interface, PCWSTR},
                    };
                    
                    // Initialize COM
                    unsafe {
                        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                    }
                    
                    let path = Path::new(&path_str);
                    let result: Result<Vec<u8>, String> = (|| -> Result<Vec<u8>, String> {
                        unsafe {
                            let wide: Vec<u16> = path
                                .as_os_str()
                                .encode_wide()
                                .chain(std::iter::once(0))
                                .collect();
                            
                            let shell_item: IShellItem = SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None::<&IBindCtx>)
                                .map_err(|e| format!("SHCreateItemFromParsingName: {e}"))?;
                            let factory: IShellItemImageFactory = shell_item
                                .cast()
                                .map_err(|e| format!("cast IShellItemImageFactory: {e}"))?;
                            let hbmp: HBITMAP = factory
                                .GetImage(SIZE { cx: 256, cy: 256 }, SIIGBF(0))
                                .map_err(|e| format!("GetImage: {e}"))?;
                            
                            // Convert HBITMAP to PNG bytes
                            hbitmap_to_png_bytes(hbmp)
                        }
                    })();
                    
                    match result {
                        Ok(png_bytes) => {
                            let response = Cmd::ThumbnailResponse(path_str, png_bytes);
                            match encode_to_vec(&response, standard()) {
                                Ok(payload) => {
                                    sender.send(WsMessage::Binary(payload));
                                    log::info!("Sent thumbnail");
                                }
                                Err(e) => {
                                    log::error!("Failed to serialize thumbnail: {}", e);
                                    sender.send(WsMessage::Text(format!("Error: {}", e)));
                                }
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to generate thumbnail: {}", e);
                            // Empty response marks the failure so the admin's
                            // stream pump can move on instead of stalling.
                            let response = Cmd::ThumbnailResponse(path_str, Vec::new());
                            if let Ok(payload) = encode_to_vec(&response, standard()) {
                                sender.send(WsMessage::Binary(payload));
                            }
                        }
                    }
                }

                #[cfg(not(target_os = "windows"))]
                {
                    // Use image crate as fallback
                    let buf = image::open(&path_str)
                        .ok()
                        .and_then(|img| {
                            let mut buf = Vec::new();
                            img.thumbnail(256, 256)
                                .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
                                .ok()
                                .map(|_| buf)
                        })
                        .unwrap_or_default();
                    let response = Cmd::ThumbnailResponse(path_str, buf);
                    if let Ok(payload) = encode_to_vec(&response, standard()) {
                        sender.send(WsMessage::Binary(payload));
                    }
                }
            }
            Cmd::SaveRemoteFile(path_str, content) => {
                log::info!("websockets -> Save file request: {}", path_str);
                
                match std::fs::write(&path_str, &content) {
                    Ok(_) => {
                        log::info!("Successfully saved file: {}", path_str);
                        let response = Cmd::SaveResult(true, format!("File saved: {}", path_str));
                        if let Ok(payload) = encode_to_vec(&response, standard()) {
                            sender.send(WsMessage::Binary(payload));
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to save file: {}", e);
                        let response = Cmd::SaveResult(false, format!("Failed to save: {}", e));
                        if let Ok(payload) = encode_to_vec(&response, standard()) {
                            sender.send(WsMessage::Binary(payload));
                        }
                    }
                }
            }
            Cmd::RebootSystem { persist_mastertech, terminal_mode } => {
                log::info!("websockets -> Reboot system command received (persist={})", persist_mastertech);
                #[cfg(target_os = "windows")]
                {
                    if persist_mastertech {
                        // Logon task with working directory = exe dir, so data.enc resolves.
                        if let Err(e) =
                            crate::utilities::windows::reboot::schedule_mastertech_relaunch(terminal_mode).await
                        {
                            log::error!("Failed to create relaunch scheduled task: {e}");
                        }
                    }

                    if let Err(e) =
                        crate::utilities::windows::reboot::reboot_now("Mastertech remote reboot requested").await
                    {
                        log::error!("Failed to initiate reboot: {e}");
                    } else {
                        log::info!("Reboot initiated successfully");
                    }
                }
                #[cfg(target_os = "linux")]
                {
                    let output = tokio::process::Command::new("sudo")
                        .args(["shutdown", "-r", "+1", "Mastertech remote reboot"])
                        .output()
                        .await;
                    
                    match output {
                        Ok(out) => {
                            if !out.status.success() {
                                let stderr = String::from_utf8_lossy(&out.stderr);
                                log::error!("Failed to initiate reboot: {}", stderr);
                            }
                        }
                        Err(e) => log::error!("Error executing shutdown: {}", e),
                    }
                }
                #[cfg(target_os = "macos")]
                {
                    let output = tokio::process::Command::new("sudo")
                        .args(["shutdown", "-r", "+1"])
                        .output()
                        .await;
                    
                    match output {
                        Ok(out) => {
                            if !out.status.success() {
                                let stderr = String::from_utf8_lossy(&out.stderr);
                                log::error!("Failed to initiate reboot: {}", stderr);
                            }
                        }
                        Err(e) => log::error!("Error executing shutdown: {}", e),
                    }
                }
            }
            Cmd::LaunchTerminalMode => {
                log::info!("websockets -> LaunchTerminalMode: spawning terminal-mode process");
                match crate::utilities::app_restart::restart_in_terminal_mode() {
                    Ok(()) => log::info!("terminal mode process spawned"),
                    Err(e) => log::error!("LaunchTerminalMode failed: {e}"),
                }
            }
            Cmd::ShutdownSystem => {
                log::info!("websockets -> Shutdown system command received");
                #[cfg(target_os = "windows")]
                {
                    let output = tokio::process::Command::new("shutdown")
                        .args(["/s", "/t", "5", "/c", "Mastertech remote shutdown requested"])
                        .output()
                        .await;
                    
                    match output {
                        Ok(out) => {
                            if out.status.success() {
                                log::info!("Shutdown initiated successfully");
                            } else {
                                let stderr = String::from_utf8_lossy(&out.stderr);
                                log::error!("Failed to initiate shutdown: {}", stderr);
                            }
                        }
                        Err(e) => log::error!("Error executing shutdown command: {}", e),
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    let output = tokio::process::Command::new("sudo")
                        .args(["shutdown", "-h", "+1"])
                        .output()
                        .await;
                    
                    match output {
                        Ok(out) => {
                            if !out.status.success() {
                                let stderr = String::from_utf8_lossy(&out.stderr);
                                log::error!("Failed to initiate shutdown: {}", stderr);
                            }
                        }
                        Err(e) => log::error!("Error executing shutdown: {}", e),
                    }
                }
            }
            Cmd::LockWorkstation => {
                log::info!("websockets -> Lock workstation command received");
                #[cfg(target_os = "windows")]
                {
                    // Use rundll32 to call the LockWorkStation function
                    let output = tokio::process::Command::new("rundll32.exe")
                        .args(["user32.dll,LockWorkStation"])
                        .output()
                        .await;
                    
                    match output {
                        Ok(out) => {
                            if out.status.success() {
                                log::info!("Workstation locked successfully");
                            } else {
                                let stderr = String::from_utf8_lossy(&out.stderr);
                                log::error!("Failed to lock workstation: {}", stderr);
                            }
                        }
                        Err(e) => log::error!("Error locking workstation: {}", e),
                    }
                }
                #[cfg(target_os = "linux")]
                {
                    // Try common screen lockers
                    let lockers = ["loginctl lock-session", "gnome-screensaver-command -l", "xdg-screensaver lock"];
                    for locker in lockers {
                        let parts: Vec<&str> = locker.split_whitespace().collect();
                        if let Some((cmd, args)) = parts.split_first() {
                            if let Ok(output) = tokio::process::Command::new(cmd)
                                .args(args)
                                .output()
                                .await
                            {
                                if output.status.success() {
                                    log::info!("Workstation locked using: {}", locker);
                                    break;
                                }
                            }
                        }
                    }
                }
                #[cfg(target_os = "macos")]
                {
                    let _ = tokio::process::Command::new("pmset")
                        .args(["displaysleepnow"])
                        .output()
                        .await;
                }
            }
            Cmd::LogOffUser => {
                log::info!("websockets -> Log off user command received");
                #[cfg(target_os = "windows")]
                {
                    let output = tokio::process::Command::new("shutdown")
                        .args(["/l"])
                        .output()
                        .await;
                    
                    match output {
                        Ok(out) => {
                            if out.status.success() {
                                log::info!("User logged off successfully");
                            } else {
                                let stderr = String::from_utf8_lossy(&out.stderr);
                                log::error!("Failed to log off user: {}", stderr);
                            }
                        }
                        Err(e) => log::error!("Error logging off user: {}", e),
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    // On Linux/macOS, kill the user's session
                    let output = tokio::process::Command::new("pkill")
                        .args(["-KILL", "-u", &whoami::username().unwrap_or_default()])
                        .output()
                        .await;
                    
                    match output {
                        Ok(out) => {
                            if !out.status.success() {
                                let stderr = String::from_utf8_lossy(&out.stderr);
                                log::error!("Failed to log off user: {}", stderr);
                            }
                        }
                        Err(e) => log::error!("Error logging off user: {}", e),
                    }
                }
            }
            // --- Event Log ---
            Cmd::ReadEventLog { log_name, max_entries, level_filter } => {
                log::info!("websockets -> Reading event log: {} (max: {}, filter: {:?})", log_name, max_entries, level_filter);

                let level_clause = match level_filter.as_deref() {
                    Some("Critical") => " -Level 1",
                    Some("Error") => " -Level 2",
                    Some("Warning") => " -Level 3",
                    Some("Information") => " -Level 4",
                    Some("Verbose") => " -Level 5",
                    _ => "",
                };

                let ps_cmd = format!(
                    "Get-WinEvent -LogName '{}' -MaxEvents {}{} -ErrorAction SilentlyContinue | Select-Object LevelDisplayName,TimeCreated,ProviderName,Id,Message | ConvertTo-Json -Compress",
                    log_name, max_entries, level_clause
                );

                let output = tokio::process::Command::new("powershell")
                    .args(["-NoProfile", "-Command", &ps_cmd])
                    .output()
                    .await;

                let mut entries = Vec::new();
                if let Ok(out) = output {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    if let Ok(json_array) = serde_json::from_str::<Vec<serde_json::Value>>(&stdout) {
                        for obj in json_array {
                            entries.push(EventLogEntry {
                                level: obj.get("LevelDisplayName").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string(),
                                time: obj.get("TimeCreated").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                source: obj.get("ProviderName").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                event_id: obj.get("Id").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                                message: obj.get("Message").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            });
                        }
                    } else if let Ok(obj) = serde_json::from_str::<serde_json::Value>(&stdout) {
                        entries.push(EventLogEntry {
                            level: obj.get("LevelDisplayName").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string(),
                            time: obj.get("TimeCreated").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            source: obj.get("ProviderName").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            event_id: obj.get("Id").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                            message: obj.get("Message").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        });
                    }
                }

                log::info!("websockets -> Parsed {} event log entries", entries.len());
                let response = Cmd::EventLogResponse(entries);
                if let Ok(payload) = encode_to_vec(&response, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }

            // --- Windows Services ---
            Cmd::ListServices => {
                log::info!("websockets -> Listing services");

                let ps_cmd = "Get-CimInstance Win32_Service | Select-Object Name,DisplayName,State,StartMode,ProcessId | ConvertTo-Json -Compress";

                let output = tokio::process::Command::new("powershell")
                    .args(["-NoProfile", "-Command", ps_cmd])
                    .output()
                    .await;

                let mut services = Vec::new();
                if let Ok(out) = output {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    if let Ok(json_array) = serde_json::from_str::<Vec<serde_json::Value>>(&stdout) {
                        for obj in json_array {
                            services.push(WindowsService {
                                name: obj.get("Name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                display_name: obj.get("DisplayName").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                status: obj.get("State").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string(),
                                start_type: obj.get("StartMode").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string(),
                                pid: obj.get("ProcessId").and_then(|v| v.as_u64()).map(|p| p as u32),
                            });
                        }
                    }
                }

                log::info!("websockets -> Found {} services", services.len());
                let response = Cmd::ServiceListResponse(services);
                if let Ok(payload) = encode_to_vec(&response, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }

            Cmd::ControlService { name, action } => {
                log::info!("websockets -> Service control: {} - {:?}", name, action);

                let ps_cmd = match &action {
                    ServiceActionType::Start => format!("Start-Service -Name '{}' -ErrorAction Stop; 'OK'", name),
                    ServiceActionType::Stop => format!("Stop-Service -Name '{}' -Force -ErrorAction Stop; 'OK'", name),
                    ServiceActionType::Restart => format!("Restart-Service -Name '{}' -Force -ErrorAction Stop; 'OK'", name),
                    ServiceActionType::SetStartType(start_type) => format!("Set-Service -Name '{}' -StartupType '{}' -ErrorAction Stop; 'OK'", name, start_type),
                };

                let output = tokio::process::Command::new("powershell")
                    .args(["-NoProfile", "-Command", &ps_cmd])
                    .output()
                    .await;

                let (success, message) = match output {
                    Ok(out) => {
                        if out.status.success() {
                            (true, format!("Action completed: {:?}", action))
                        } else {
                            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                            (false, stderr)
                        }
                    }
                    Err(e) => (false, format!("Failed to execute: {}", e)),
                };

                let response = Cmd::ServiceActionResponse { name, success, message };
                if let Ok(payload) = encode_to_vec(&response, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }

            // --- Task Scheduler ---
            Cmd::ListScheduledTasks { folder } => {
                log::info!("websockets -> Listing scheduled tasks (folder: {:?})", folder);

                let folder_filter = folder.as_deref().unwrap_or("\\");
                let ps_cmd = format!(
                    r#"$tasks = Get-ScheduledTask -TaskPath '{}*' -ErrorAction SilentlyContinue; $results = @(); foreach($t in $tasks) {{ $info = $null; try {{ $info = Get-ScheduledTaskInfo -TaskName $t.TaskName -TaskPath $t.TaskPath -ErrorAction SilentlyContinue }} catch {{}}; $triggers = @(); foreach($tr in $t.Triggers) {{ $triggers += $tr.CimClass.CimClassName }}; $actions = @(); foreach($a in $t.Actions) {{ $actions += $a.Execute }}; $results += @{{ Name=$t.TaskName; Path=$t.TaskPath; State=$t.State.ToString(); LastRun=if($info){{$info.LastRunTime.ToString('o')}}else{{'Never'}}; NextRun=if($info){{$info.NextRunTime.ToString('o')}}else{{'N/A'}}; Description=$t.Description; Triggers=$triggers; Actions=$actions }} }}; $results | ConvertTo-Json -Compress -Depth 3"#,
                    folder_filter
                );

                let output = tokio::process::Command::new("powershell")
                    .args(["-NoProfile", "-Command", &ps_cmd])
                    .output()
                    .await;

                let mut tasks = Vec::new();
                if let Ok(out) = output {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    let parse_task = |obj: &serde_json::Value| -> ScheduledTask {
                        ScheduledTask {
                            name: obj.get("Name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            path: obj.get("Path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            state: obj.get("State").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string(),
                            last_run: obj.get("LastRun").and_then(|v| v.as_str()).map(|s| s.to_string()),
                            next_run: obj.get("NextRun").and_then(|v| v.as_str()).map(|s| s.to_string()),
                            description: obj.get("Description").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            triggers: obj.get("Triggers").and_then(|v| v.as_array()).map(|arr| {
                                arr.iter().filter_map(|t| t.as_str().map(|s| s.to_string())).collect()
                            }).unwrap_or_default(),
                            actions: obj.get("Actions").and_then(|v| v.as_array()).map(|arr| {
                                arr.iter().filter_map(|a| a.as_str().map(|s| s.to_string())).collect()
                            }).unwrap_or_default(),
                        }
                    };

                    if let Ok(json_array) = serde_json::from_str::<Vec<serde_json::Value>>(&stdout) {
                        for obj in &json_array {
                            tasks.push(parse_task(obj));
                        }
                    } else if let Ok(obj) = serde_json::from_str::<serde_json::Value>(&stdout) {
                        tasks.push(parse_task(&obj));
                    }
                }

                log::info!("websockets -> Found {} scheduled tasks", tasks.len());
                let response = Cmd::ScheduledTaskListResponse(tasks);
                if let Ok(payload) = encode_to_vec(&response, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }

            Cmd::ToggleScheduledTask { path, enable } => {
                log::info!("websockets -> {} task: {}", if enable { "Enable" } else { "Disable" }, path);

                let ps_cmd = if enable {
                    format!("Enable-ScheduledTask -TaskName '{}' -ErrorAction Stop; 'OK'", path)
                } else {
                    format!("Disable-ScheduledTask -TaskName '{}' -ErrorAction Stop; 'OK'", path)
                };

                let output = tokio::process::Command::new("powershell")
                    .args(["-NoProfile", "-Command", &ps_cmd])
                    .output()
                    .await;

                let (success, message) = match output {
                    Ok(out) => {
                        if out.status.success() {
                            (true, format!("Task {}", if enable { "enabled" } else { "disabled" }))
                        } else {
                            (false, String::from_utf8_lossy(&out.stderr).to_string())
                        }
                    }
                    Err(e) => (false, format!("Failed: {}", e)),
                };

                let response = Cmd::ScheduledTaskActionResponse { success, message };
                if let Ok(payload) = encode_to_vec(&response, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }

            Cmd::RunScheduledTask(path) => {
                log::info!("websockets -> Running task: {}", path);

                let ps_cmd = format!("Start-ScheduledTask -TaskName '{}' -ErrorAction Stop; 'OK'", path);
                let output = tokio::process::Command::new("powershell")
                    .args(["-NoProfile", "-Command", &ps_cmd])
                    .output()
                    .await;

                let (success, message) = match output {
                    Ok(out) => {
                        if out.status.success() {
                            (true, "Task started".to_string())
                        } else {
                            (false, String::from_utf8_lossy(&out.stderr).to_string())
                        }
                    }
                    Err(e) => (false, format!("Failed: {}", e)),
                };

                let response = Cmd::ScheduledTaskActionResponse { success, message };
                if let Ok(payload) = encode_to_vec(&response, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }

            // --- Registry ---
            Cmd::ListRegistryKeys(path) => {
                log::info!("websockets -> Listing registry keys: {}", path);

                let ps_cmd = format!(
                    r#"$subkeys = @(); $values = @(); try {{ Get-ChildItem -Path 'Registry::{path}' -ErrorAction Stop | ForEach-Object {{ $subkeys += @{{ Name=$_.PSChildName; Path=$_.Name; SubkeyCount=(Get-ChildItem -Path $_.PSPath -ErrorAction SilentlyContinue | Measure-Object).Count; ValueCount=(Get-ItemProperty -Path $_.PSPath -ErrorAction SilentlyContinue | Get-Member -MemberType NoteProperty | Where-Object {{ $_.Name -notmatch '^PS' }} | Measure-Object).Count }} }}; $props = Get-ItemProperty -Path 'Registry::{path}' -ErrorAction SilentlyContinue; if($props) {{ $props | Get-Member -MemberType NoteProperty | Where-Object {{ $_.Name -notmatch '^PS' }} | ForEach-Object {{ $n = $_.Name; $v = $props.$n; $kind = (Get-Item -Path 'Registry::{path}' -ErrorAction SilentlyContinue).GetValueKind($n); $values += @{{ Name=$n; Kind=$kind.ToString(); Data=[string]$v }} }} }} }} catch {{ }}; @{{ Subkeys=$subkeys; Values=$values }} | ConvertTo-Json -Compress -Depth 4"#
                );

                let output = tokio::process::Command::new("powershell")
                    .args(["-NoProfile", "-Command", &ps_cmd])
                    .output()
                    .await;

                let mut subkeys = Vec::new();
                let mut values = Vec::new();

                if let Ok(out) = output {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(&stdout) {
                        if let Some(sk_arr) = obj.get("Subkeys").and_then(|v| v.as_array()) {
                            for sk in sk_arr {
                                subkeys.push(RegistryKeyInfo {
                                    name: sk.get("Name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                    path: sk.get("Path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                    subkey_count: sk.get("SubkeyCount").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                                    value_count: sk.get("ValueCount").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                                });
                            }
                        }
                        if let Some(val_arr) = obj.get("Values").and_then(|v| v.as_array()) {
                            for val in val_arr {
                                values.push(RegistryValueEntry {
                                    name: val.get("Name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                    kind: val.get("Kind").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string(),
                                    data: val.get("Data").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                });
                            }
                        }
                    }
                }

                log::info!("websockets -> Registry: {} subkeys, {} values", subkeys.len(), values.len());
                let response = Cmd::RegistryKeyResponse { path, subkeys, values };
                if let Ok(payload) = encode_to_vec(&response, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }

            Cmd::BackupRegistryKey(path) => {
                log::info!("websockets -> Backing up registry key: {}", path);

                let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
                let backup_filename = format!("reg_backup_{}_{}.reg",
                    path.replace('\\', "_").replace('/', "_"),
                    timestamp
                );
                let backup_dir = std::env::temp_dir().join("mastertech_reg_backups");
                let _ = std::fs::create_dir_all(&backup_dir);
                let backup_path = backup_dir.join(&backup_filename);
                let backup_path_str = backup_path.to_string_lossy().to_string();

                let output = tokio::process::Command::new("reg")
                    .args(["export", &path, &backup_path_str, "/y"])
                    .output()
                    .await;

                let (success, message) = match output {
                    Ok(out) => {
                        if out.status.success() {
                            (true, format!("Backup saved to {}", backup_path_str))
                        } else {
                            (false, String::from_utf8_lossy(&out.stderr).to_string())
                        }
                    }
                    Err(e) => (false, format!("Failed to backup: {}", e)),
                };

                let response = Cmd::RegistryBackupResponse {
                    success,
                    backup_path: backup_path_str,
                    message,
                };
                if let Ok(payload) = encode_to_vec(&response, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }

            Cmd::CommitRegistryEdits(edits) => {
                log::info!("websockets -> Committing {} registry edits", edits.len());

                let mut all_success = true;
                let mut messages = Vec::new();

                for edit in &edits {
                    let ps_cmd = match edit {
                        RegistryEdit::SetValue { path, name, kind, data } => {
                            let reg_type = match kind.as_str() {
                                "REG_DWORD" | "DWord" => "DWord",
                                "REG_QWORD" | "QWord" => "QWord",
                                "REG_BINARY" | "Binary" => "Binary",
                                "REG_MULTI_SZ" | "MultiString" => "MultiString",
                                "REG_EXPAND_SZ" | "ExpandString" => "ExpandString",
                                _ => "String",
                            };
                            format!(
                                "Set-ItemProperty -Path 'Registry::{}' -Name '{}' -Value '{}' -Type {} -ErrorAction Stop; 'OK'",
                                path, name, data, reg_type
                            )
                        }
                        RegistryEdit::DeleteValue { path, name } => {
                            format!(
                                "Remove-ItemProperty -Path 'Registry::{}' -Name '{}' -ErrorAction Stop; 'OK'",
                                path, name
                            )
                        }
                        RegistryEdit::CreateKey { path } => {
                            format!(
                                "New-Item -Path 'Registry::{}' -Force -ErrorAction Stop | Out-Null; 'OK'",
                                path
                            )
                        }
                        RegistryEdit::DeleteKey { path } => {
                            format!(
                                "Remove-Item -Path 'Registry::{}' -Recurse -Force -ErrorAction Stop; 'OK'",
                                path
                            )
                        }
                    };

                    let output = tokio::process::Command::new("powershell")
                        .args(["-NoProfile", "-Command", &ps_cmd])
                        .output()
                        .await;

                    match output {
                        Ok(out) => {
                            if !out.status.success() {
                                all_success = false;
                                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                                messages.push(format!("Failed: {}", stderr));
                            }
                        }
                        Err(e) => {
                            all_success = false;
                            messages.push(format!("Error: {}", e));
                        }
                    }
                }

                let message = if all_success {
                    format!("All {} edit(s) applied successfully", edits.len())
                } else {
                    messages.join("; ")
                };

                let response = Cmd::RegistryEditResponse { success: all_success, message };
                if let Ok(payload) = encode_to_vec(&response, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }

            Cmd::ListStartupApps => {
                log::info!("websockets -> ListStartupApps");

                let ps_cmd = r#"
$paths = @(
    "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run",
    "HKLM:\Software\Microsoft\Windows\CurrentVersion\Run",
    "HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run",
    "HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run",
    "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
)
$results = @()
foreach ($path in $paths) {
    if (Test-Path $path) {
        $isApproved = $path -like "*StartupApproved*"
        $props = Get-ItemProperty -Path $path -ErrorAction SilentlyContinue
        if ($props) {
            $memberNames = ($props | Get-Member -MemberType NoteProperty | Select-Object -ExpandProperty Name) | Where-Object { $_ -notin @('PSPath','PSParentPath','PSChildName','PSProvider','PSDrive') }
            foreach ($name in $memberNames) {
                $value = $props.$name
                $state = "Unknown"
                $cmd = ""
                if ($isApproved) {
                    if ($value -is [byte[]] -and $value.Length -ge 1) {
                        switch ($value[0]) {
                            0x02 { $state = "Enabled" }
                            0x03 { $state = "Disabled" }
                            0x06 { $state = "DisabledByUser" }
                            default { $state = "Unknown" }
                        }
                    }
                    $runPath = $path -replace 'StartupApproved\\Run','Run'
                    if (Test-Path $runPath) {
                        $runProps = Get-ItemProperty -Path $runPath -ErrorAction SilentlyContinue
                        if ($runProps -and ($runProps | Get-Member -Name $name -ErrorAction SilentlyContinue)) {
                            $cmd = [string]$runProps.$name
                        }
                    }
                } else {
                    $cmd = [string]$value
                    $approvedPath = $path -replace '\\Run$','\Explorer\StartupApproved\Run'
                    if (Test-Path $approvedPath) {
                        $approvedProps = Get-ItemProperty -Path $approvedPath -ErrorAction SilentlyContinue
                        if ($approvedProps -and ($approvedProps | Get-Member -Name $name -ErrorAction SilentlyContinue)) {
                            $aVal = $approvedProps.$name
                            if ($aVal -is [byte[]] -and $aVal.Length -ge 1) {
                                switch ($aVal[0]) {
                                    0x02 { $state = "Enabled" }
                                    0x03 { $state = "Disabled" }
                                    0x06 { $state = "DisabledByUser" }
                                    default { $state = "Unknown" }
                                }
                            }
                        } else { $state = "Enabled" }
                    } else { $state = "Enabled" }
                }

                $source = if ($path -like "HKLM:*") { "HKLM" } else { "HKCU" }
                if ($path -like "*WOW6432Node*") { $source = "HKLM (32-bit)" }
                if ($isApproved) { $source += " (Approved)" }

                if (-not $isApproved -or $cmd -ne "") {
                    $results += [pscustomobject]@{
                        name = $name
                        command = $cmd
                        registry_path = $path
                        state = $state
                        source = $source
                    }
                }
            }
        }
    }
}
$results | Sort-Object -Property name -Unique | ConvertTo-Json -Depth 3
"#;

                let output = tokio::process::Command::new("powershell")
                    .args(["-NoProfile", "-Command", ps_cmd])
                    .output()
                    .await;

                let apps: Vec<StartupApp> = match output {
                    Ok(out) if out.status.success() => {
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        let trimmed = stdout.trim();
                        if trimmed.is_empty() || trimmed == "null" {
                            Vec::new()
                        } else {
                            serde_json::from_str::<Vec<StartupApp>>(trimmed)
                                .or_else(|_| serde_json::from_str::<StartupApp>(trimmed).map(|s| vec![s]))
                                .unwrap_or_default()
                        }
                    }
                    Ok(out) => {
                        log::error!("ListStartupApps failed: {}", String::from_utf8_lossy(&out.stderr));
                        Vec::new()
                    }
                    Err(e) => {
                        log::error!("ListStartupApps error: {e}");
                        Vec::new()
                    }
                };

                let response = Cmd::StartupAppsResponse(apps);
                if let Ok(payload) = encode_to_vec(&response, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }

            Cmd::ToggleStartupApp { name, registry_path, enable } => {
                log::info!("websockets -> ToggleStartupApp: {} -> enable={}", name, enable);

                // Determine the StartupApproved path from the registry_path
                let approved_path = if registry_path.contains("StartupApproved") {
                    registry_path.clone()
                } else {
                    registry_path.replace("\\Run", "\\Explorer\\StartupApproved\\Run")
                };

                let byte_val = if enable { "0x02" } else { "0x03" };

                let ps_cmd = format!(
                    r#"
$path = '{approved_path}'
$name = '{name}'
if (Test-Path $path) {{
    $props = Get-ItemProperty -Path $path -ErrorAction SilentlyContinue
    if ($props -and ($props | Get-Member -Name $name -ErrorAction SilentlyContinue)) {{
        $current = $props.$name
        if ($current -is [byte[]]) {{
            $current[0] = {byte_val}
            Set-ItemProperty -Path $path -Name $name -Value ([byte[]]$current) -ErrorAction Stop
            'OK'
        }} else {{
            $newVal = [byte[]]@({byte_val},0,0,0,0,0,0,0,0,0,0,0)
            Set-ItemProperty -Path $path -Name $name -Value $newVal -ErrorAction Stop
            'OK'
        }}
    }} else {{
        $newVal = [byte[]]@({byte_val},0,0,0,0,0,0,0,0,0,0,0)
        New-ItemProperty -Path $path -Name $name -Value $newVal -PropertyType Binary -ErrorAction Stop | Out-Null
        'OK'
    }}
}} else {{
    Write-Error "Registry path not found: $path"
}}
"#
                );

                let output = tokio::process::Command::new("powershell")
                    .args(["-NoProfile", "-Command", &ps_cmd])
                    .output()
                    .await;

                let (success, message) = match output {
                    Ok(out) if out.status.success() => {
                        let action = if enable { "enabled" } else { "disabled" };
                        (true, format!("'{}' {}", name, action))
                    }
                    Ok(out) => {
                        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                        (false, format!("Failed to toggle '{}': {}", name, stderr))
                    }
                    Err(e) => (false, format!("Error: {}", e)),
                };

                let response = Cmd::StartupAppActionResponse { success, message };
                if let Ok(payload) = encode_to_vec(&response, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }

            Cmd::GetRemoteScriptList => {
                log::info!("websockets -> GetRemoteScriptList");
                let all = displays::scripts::get_all_categories();
                let categories: Vec<(String, Vec<RemoteScriptItem>)> = displays::scripts::CATEGORY_ORDER
                    .iter()
                    .filter_map(|cat| {
                        let cat_label = format!("{}", cat);
                        let items = all.get(cat)?;
                        let entries = items
                            .iter()
                            .map(|s| RemoteScriptItem {
                                name: s.name.clone(),
                                category: cat_label.clone(),
                                content: None,
                            })
                            .collect();
                        Some((cat_label, entries))
                    })
                    .collect();
                let response = Cmd::RemoteScriptListResponse { categories };
                if let Ok(payload) = encode_to_vec(&response, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }

            Cmd::RunRemoteScripts { scripts, service_number, customer_email, diagnostic_session_id } => {
                log::info!("websockets -> RunRemoteScripts: {} scripts, SO={}", scripts.len(), service_number);

                // Spawn the script loop so the TCP session loop can continue
                // processing Ping/AppPing frames while long-running scripts
                // (CPS key fetch, antivirus installs) execute in the background.
                let tx = self.command_tx.clone();
                tokio::spawn(async move {

                let task_epoch = Instant::now();
                // Millis from task_epoch when the current script started; AtomicU64 keeps the future Send.
                let script_started_ms = std::sync::atomic::AtomicU64::new(0);
                // Set when a script in this batch wants a reboot; reported once after the batch.
                let batch_reboot_recommended = std::sync::atomic::AtomicBool::new(false);

                let send_log = |tx: &tokio::sync::mpsc::UnboundedSender<Vec<u8>>, msg: String| {
                    let cmd = Cmd::RemoteScriptLog(msg);
                    if let Ok(payload) = encode_to_vec(&cmd, standard()) {
                        let _ = tx.send(payload);
                    }
                };

                let send_result = |tx: &tokio::sync::mpsc::UnboundedSender<Vec<u8>>, name: &str, status: RemoteScriptStatus| {
                    let verdict = match status {
                        RemoteScriptStatus::Success => Some("PASSED"),
                        RemoteScriptStatus::Failed => Some("FAILED"),
                        _ => None,
                    };
                    let cmd = Cmd::RemoteScriptResult { name: name.to_string(), status };
                    if let Ok(payload) = encode_to_vec(&cmd, standard()) {
                        let _ = tx.send(payload);
                    }
                    // `<name> PASSED/FAILED in <secs>s` marker clears the admin home-page active-run card.
                    if let Some(verdict) = verdict {
                        let elapsed_s = (task_epoch.elapsed().as_millis() as u64)
                            .saturating_sub(script_started_ms.load(std::sync::atomic::Ordering::SeqCst))
                            / 1000;
                        let marker = Cmd::RemoteScriptLog(format!(
                            "{name} {verdict} in {elapsed_s}s — remote script finished"
                        ));
                        if let Ok(payload) = encode_to_vec(&marker, standard()) {
                            let _ = tx.send(payload);
                        }
                    }
                };

                for script in &scripts {
                    script_started_ms.store(
                        task_epoch.elapsed().as_millis() as u64,
                        std::sync::atomic::Ordering::SeqCst,
                    );
                    send_log(&tx, format!("Starting: {}", script.name));

                    let allowed_secs =
                        displays::scripts::default_remote_script_timeout_secs(&script.name);
                    let script_fut = async {
                    match script.name.as_str() {
                        "Disable Sleep / Hibernation" => {
                            match crate::terminal_mode::tabs::script_categories::disable_hibernation_and_sleep() {
                                Ok(_) => {
                                    send_log(&tx, "Disabled Sleep / Hibernation".into());
                                    send_result(&tx, &script.name, RemoteScriptStatus::Success);
                                }
                                Err(e) => {
                                    send_log(&tx, format!("Error: {e}"));
                                    send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                }
                            }
                        }

                        "Activate Webroot" => {
                            if service_number.is_empty() {
                                send_log(&tx, "Webroot activation requires SO number".into());
                                send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                return;
                            }
                            send_log(&tx, "Fetching CPS keys...".into());
                            let so = service_number.clone();
                            let client = reqwest::Client::new();
                            let (progress_tx, _) = crossbeam::channel::unbounded();
                            match crate::tabs::tur_sheet::get_ticket::SendRequest::get_cps(so, client.clone()).await {
                                Ok(keys) => {
                                    let key = keys.get(0).cloned().unwrap_or_default();
                                    send_log(&tx, format!("Webroot key: {}", key.webroot_key));
                                    match crate::utilities::scripts::antivirus::install_webroot(key.webroot_key, client, progress_tx).await {
                                        Ok(rekeyed) => {
                                            send_log(&tx, "Webroot installed successfully".into());
                                            if rekeyed {
                                                batch_reboot_recommended.store(true, std::sync::atomic::Ordering::SeqCst);
                                            }
                                            send_result(&tx, &script.name, RemoteScriptStatus::Success);
                                        }
                                        Err(e) => {
                                            send_log(&tx, format!("Webroot install error: {e}"));
                                            send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                        }
                                    }
                                }
                                Err(e) => {
                                    send_log(&tx, format!("Failed to get CPS keys: {e}"));
                                    send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                }
                            }
                        }

                        "Activate SuperAnti" => {
                            if service_number.is_empty() {
                                send_log(&tx, "SuperAnti activation requires SO number".into());
                                send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                return;
                            }
                            let killed = crate::utilities::scripts::antivirus::kill_sas_processes();
                            send_log(&tx, format!("Killed {killed} SAS processes"));
                            let so = service_number.clone();
                            let client = reqwest::Client::new();
                            let (progress_tx, _) = crossbeam::channel::unbounded();
                            match crate::tabs::tur_sheet::get_ticket::SendRequest::get_cps(so, client.clone()).await {
                                Ok(keys) => {
                                    let key = keys.get(0).cloned().unwrap_or_default();
                                    send_log(&tx, format!("SuperAnti key: {}", key.superanti_key));
                                    // install_sas activates via /REGCODE during silent install
                                    // (fresh) or /autoregister:KEY against the existing exe.
                                    match crate::utilities::scripts::antivirus::install_sas(key.superanti_key, client, progress_tx).await {
                                        Ok(_) => {
                                            send_log(&tx, "SAS installed and activated".into());
                                            send_result(&tx, &script.name, RemoteScriptStatus::Success);
                                        }
                                        Err(e) => {
                                            send_log(&tx, format!("SAS install error: {e}"));
                                            send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                        }
                                    }
                                }
                                Err(e) => {
                                    send_log(&tx, format!("Failed to get CPS keys: {e}"));
                                    send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                }
                            }
                        }

                        "Activate SEB" => {
                            if service_number.is_empty() || customer_email.is_empty() {
                                send_log(&tx, "SEB activation requires SO number and email".into());
                                send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                return;
                            }
                            let client = reqwest::Client::new();
                            let (progress_tx, _) = crossbeam::channel::unbounded();
                            match crate::utilities::scripts::antivirus::install_supereasybackup(customer_email.clone(), client, progress_tx).await {
                                Ok(_) => {
                                    send_log(&tx, "SEB installed successfully".into());
                                    send_result(&tx, &script.name, RemoteScriptStatus::Success);
                                }
                                Err(e) => {
                                    send_log(&tx, format!("SEB install error: {e}"));
                                    send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                }
                            }
                        }

                        "Install Windows Updates" => {
                            send_log(&tx, "Checking internet before Windows Updates...".into());
                            match crate::utilities::windows::net_adapter::ensure_internet_connected().await {
                                Ok(_) => send_log(&tx, "Internet confirmed".into()),
                                Err(e) => {
                                    send_log(&tx, format!("No internet: {e}"));
                                    send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                    return;
                                }
                            }
                            send_log(&tx, "Starting Windows Updates (search + install)...".into());

                            let (update_tx, update_rx) = crossbeam::channel::unbounded();
                            let handle = std::thread::spawn(move || {
                                crate::utilities::windows::windows_update::install_windows_updates(
                                    update_tx, true, true,
                                )
                            });

                            loop {
                                use crate::utilities::windows::windows_update::WindowsUpdateEvent;
                                while let Ok(event) = update_rx.try_recv() {
                                    match event {
                                        WindowsUpdateEvent::UpdateLogs(msg) => send_log(&tx, msg),
                                        WindowsUpdateEvent::DownloadPercentage(pct) => {
                                            send_log(&tx, format!("Download: {pct}%"));
                                        }
                                        WindowsUpdateEvent::InstallPercentage(pct) => {
                                            send_log(&tx, format!("Install: {pct}%"));
                                        }
                                        WindowsUpdateEvent::ReturnedUpdates(updates) => {
                                            send_log(&tx, format!("{} updates processed", updates.updates.len()));
                                            for u in &updates.updates {
                                                send_log(&tx, format!("  {} (installed: {})", u.title, u.is_installed));
                                            }
                                        }
                                    }
                                }
                                if handle.is_finished() {
                                    break;
                                }
                                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                            }

                            match handle.join() {
                                Ok(Ok(_)) => {
                                    send_log(&tx, "Windows Updates completed successfully".into());
                                    send_result(&tx, &script.name, RemoteScriptStatus::Success);
                                }
                                Ok(Err(e)) => {
                                    send_log(&tx, format!("Windows Updates error: {e:?}"));
                                    send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                }
                                Err(_) => {
                                    send_log(&tx, "Windows Updates thread panicked".into());
                                    send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                }
                            }
                        }

                        "Install LibreOffice" => {
                            let client = reqwest::Client::new();
                            let (progress_tx, _) = crossbeam::channel::unbounded();
                            match crate::utilities::scripts::programs::install_program(
                                "https://ninite.com/libreoffice/ninite.exe".into(), client, progress_tx
                            ).await {
                                Ok(_) => {
                                    send_log(&tx, "LibreOffice installed".into());
                                    send_result(&tx, &script.name, RemoteScriptStatus::Success);
                                }
                                Err(e) => {
                                    send_log(&tx, format!("LibreOffice install error: {e}"));
                                    send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                }
                            }
                        }
                        
                        "Disable Notifications" => if cfg!(target_os = "windows") {
                            let mut msgs = Vec::new();
                            let mut ok = true;
                            macro_rules! try_reg {
                                ($fn:expr, $label:expr) => {
                                    match $fn() {
                                        Ok(r) => msgs.push(format!("{}: {:?}", $label, r)),
                                        Err(e) => { msgs.push(format!("{}: {e}", $label)); ok = false; }
                                    }
                                }
                            }
                            use crate::utilities::windows::registry::*;
                            try_reg!(disable_notifications, "notifications");
                            try_reg!(disable_lockscreen_notifications, "lockscreen_notifications");
                            try_reg!(disable_content_delivery_allowed, "content_delivery");
                            try_reg!(disable_silent_installed_apps_enabled, "silent_apps");
                            try_reg!(disable_subscribed_content_enabled, "subscribed_content");
                            try_reg!(disable_system_pane_suggestions_enabled, "system_suggestions");
                            try_reg!(disable_account_notifications, "account_notifications");
                            try_reg!(enable_more_pins_layout, "more_pins_layout");
                            try_reg!(disable_start_account_notifications, "start_account_notifications");
                            try_reg!(disable_recent_items_tracking, "recent_items");
                            try_reg!(remove_chat_from_taskbar, "chat_taskbar");
                            for m in &msgs { send_log(&tx, m.clone()); }
                            send_result(&tx, &script.name, if ok { RemoteScriptStatus::Success } else { RemoteScriptStatus::Failed });
                        }

                        "Unpin Copilot" => {
                            let mut ok = true;
                            match crate::utilities::windows::registry::disable_copilot() {
                                Ok(results) => for r in &results { send_log(&tx, r.clone()); },
                                Err(e) => { ok = false; send_log(&tx, format!("Error: {e}")); }
                            }
                            match crate::utilities::scripts::remove_copilot_appx() {
                                Ok(msgs) => for m in msgs { send_log(&tx, m); },
                                Err(e) => { ok = false; send_log(&tx, format!("Copilot app removal: {e}")); }
                            }
                            send_result(&tx, &script.name, if ok { RemoteScriptStatus::Success } else { RemoteScriptStatus::Failed });
                        }

                        "Align Taskbar to left" => {
                            match crate::utilities::windows::registry::align_taskbar_left() {
                                Ok(msgs) => {
                                    for m in &msgs { send_log(&tx, m.trim().to_string()); }
                                    send_result(&tx, &script.name, RemoteScriptStatus::Success);
                                }
                                Err(e) => {
                                    send_log(&tx, format!("Error: {e}"));
                                    send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                }
                            }
                        }

                        "Change SuperAntiSpyware settings" => {
                            let sas_exe = std::path::Path::new(r"C:\Program Files\SUPERAntiSpyware\SUPERAntiSpyware.exe");
                            if !sas_exe.exists() {
                                send_log(&tx, "SAS not installed".into());
                                send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                return;
                            }
                            let killed = crate::utilities::scripts::antivirus::kill_sas_processes();
                            send_log(&tx, format!("Killed {killed} SAS processes"));
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                            match crate::utilities::scripts::antivirus::sas_tasks::configure_sas_scheduled_tasks() {
                                Ok((update_guid, scan_guid)) => {
                                    send_log(&tx, format!("SAS update task: {update_guid}"));
                                    send_log(&tx, format!("SAS scan task: {scan_guid}"));
                                    send_result(&tx, &script.name, RemoteScriptStatus::Success);
                                }
                                Err(e) => {
                                    send_log(&tx, format!("Error: {e}"));
                                    send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                }
                            }
                        }

                        "Is Windows Activated?" => {
                            match crate::terminal_mode::tabs::script_categories::check_windows_activation() {
                                Ok(status) => {
                                    let msg = if status.license_status == 1 { "Windows is activated" } else { "Windows is NOT activated" };
                                    send_log(&tx, msg.into());
                                    send_result(&tx, &script.name, RemoteScriptStatus::Success);
                                }
                                Err(e) => {
                                    send_log(&tx, format!("Error: {e}"));
                                    send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                }
                            }
                        }

                        "Windows Version" => {
                            let ver = sysinfo::System::long_os_version().unwrap_or_default();
                            send_log(&tx, format!("Windows Version: {ver}"));
                            send_result(&tx, &script.name, RemoteScriptStatus::Success);
                        }

                        "Is SuperEasyBackup installed?" | "Is Webroot installed?" | "Is SuperAntiSpyware installed?" => {
                            let search_term = match script.name.as_str() {
                                "Is SuperEasyBackup installed?" => "supereasybackup",
                                "Is Webroot installed?" => "webroot",
                                "Is SuperAntiSpyware installed?" => "superantispyware",
                                _ => "",
                            };
                            match crate::utilities::scripts::programs::InstalledProgram::get_installed_programs() {
                                Ok(programs) => {
                                    let found = programs.iter().any(|p| {
                                        let dn = p.display_name.clone().unwrap_or_default().to_lowercase();
                                        let pub_ = p.publisher.clone().unwrap_or_default().to_lowercase();
                                        dn.contains(search_term) || pub_.contains(search_term)
                                    });
                                    if found {
                                        send_log(&tx, format!("{} found", script.name.trim_end_matches('?')));
                                    } else {
                                        send_log(&tx, format!("{} NOT found", script.name.trim_end_matches('?')));
                                    }
                                    send_result(&tx, &script.name, RemoteScriptStatus::Success);
                                }
                                Err(e) => {
                                    send_log(&tx, format!("Error querying programs: {e}"));
                                    send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                }
                            }
                        }

                        "Any Recent Blue Screens?" => {
                            let ps_cmd = r#"
$days = 30
$start = (Get-Date).AddDays(-$days)
$evts = New-Object System.Collections.Generic.List[Object]
$filters = @(
    @{ LogName = 'System';      ProviderName = 'Microsoft-Windows-WER-SystemErrorReporting'; StartTime = $start },
    @{ LogName = 'System';      Id = 1001; StartTime = $start },
    @{ LogName = 'System';      Id = 1003; StartTime = $start },
    @{ LogName = 'System';      Id = 41;   StartTime = $start },
    @{ LogName = 'System';      Id = 6008; StartTime = $start },
    @{ LogName = 'System';      Id = 4101; StartTime = $start },
    @{ LogName = 'Application'; ProviderName = 'Windows Error Reporting'; Id = 1001; StartTime = $start }
)
foreach ($f in $filters) {
    try {
        Get-WinEvent -FilterHashtable $f -ErrorAction SilentlyContinue |
            ForEach-Object { $null = $evts.Add($_) }
    } catch {}
}
if ($evts.Count -eq 0) {
    Write-Output ("No BugCheck / WER / Kernel-Power / TDR (nvlddmkm 4101) events in the last {0} days." -f $days)
} else {
    Write-Output ("Found {0} BSOD/TDR-related events in the last {1} days (most recent 25):" -f $evts.Count, $days)
    $evts | Sort-Object TimeCreated -Descending | Select-Object -First 25 | ForEach-Object {
        $first = ($_.Message -split "`r?`n") | Where-Object { $_.Trim() -ne '' } | Select-Object -First 1
        Write-Output ("[{0}] id={1} provider={2} level={3} :: {4}" -f $_.TimeCreated, $_.Id, $_.ProviderName, $_.LevelDisplayName, $first)
    }
}
$mini = Get-ChildItem -Path "$env:SystemRoot\Minidump" -Filter '*.dmp' -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime -Descending | Select-Object -First 10
if ($mini) {
    Write-Output "Recent minidumps:"
    foreach ($d in $mini) { Write-Output ("  {0}  {1} bytes  {2}" -f $d.LastWriteTime, $d.Length, $d.FullName) }
} else {
    Write-Output "No minidumps in $env:SystemRoot\Minidump"
}
$memdmp = Join-Path $env:SystemRoot 'MEMORY.DMP'
if (Test-Path $memdmp) {
    $f = Get-Item $memdmp
    Write-Output ("MEMORY.DMP present: {0} bytes, last written {1}" -f $f.Length, $f.LastWriteTime)
}
"#;
                            let output = tokio::process::Command::new("powershell")
                                .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", ps_cmd])
                                .output()
                                .await;
                            match output {
                                Ok(out) => {
                                    let stdout = String::from_utf8_lossy(&out.stdout);
                                    let stderr = String::from_utf8_lossy(&out.stderr);
                                    for line in stdout.lines() {
                                        if !line.trim().is_empty() { send_log(&tx, line.to_string()); }
                                    }
                                    for line in stderr.lines() {
                                        if !line.trim().is_empty() { send_log(&tx, format!("[stderr] {line}")); }
                                    }
                                    if out.status.success() {
                                        send_result(&tx, &script.name, RemoteScriptStatus::Success);
                                    } else {
                                        send_log(&tx, format!("Exit code: {:?}", out.status.code()));
                                        send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                    }
                                }
                                Err(e) => {
                                    send_log(&tx, format!("Error running BSOD check: {e}"));
                                    send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                }
                            }
                        }

                        "Is Hibernation/Sleep enabled?" => {
                            let ps_cmd = r#"
Write-Output ((powercfg /getactivescheme) -join '')
$settings = @(
    @{ Name='Sleep after';      Sub='238c9fa8-0aad-41ed-83f4-97be242c8f20'; Guid='29f6c1db-86da-48c5-9fdb-f2b67b1f44da'; Units='Seconds' },
    @{ Name='Hibernate after';  Sub='238c9fa8-0aad-41ed-83f4-97be242c8f20'; Guid='9d7815a6-7ee4-497e-8888-515a05f02364'; Units='Seconds' },
    @{ Name='Hybrid sleep';     Sub='238c9fa8-0aad-41ed-83f4-97be242c8f20'; Guid='94ac6d29-73ce-41a6-809f-6363ba21b47e'; Units='OnOff'   },
    @{ Name='Turn off display'; Sub='7516b95f-f776-4464-8c53-06167f40cc99'; Guid='3c0bc021-c8a8-4e07-a973-6b14cbcb2b7e'; Units='Seconds' }
)
$anyEnabled = $false
foreach ($s in $settings) {
    $out = powercfg /query SCHEME_CURRENT $s.Sub $s.Guid 2>$null
    $acMatch = $out | Select-String 'Current AC Power Setting Index:\s*(0x[0-9a-fA-F]+)'
    $dcMatch = $out | Select-String 'Current DC Power Setting Index:\s*(0x[0-9a-fA-F]+)'
    $ac = if ($acMatch) { $acMatch.Matches[0].Groups[1].Value } else { '0x00000000' }
    $dc = if ($dcMatch) { $dcMatch.Matches[0].Groups[1].Value } else { '0x00000000' }
    if ($s.Units -eq 'Seconds') {
        $acVal = [uint32]$ac
        $dcVal = [uint32]$dc
        Write-Output ("{0}: AC={1}s  DC={2}s" -f $s.Name, $acVal, $dcVal)
        if ($acVal -gt 0 -or $dcVal -gt 0) { $anyEnabled = $true }
    } else {
        $acOn = if ($ac -ne '0x00000000') { 'On' } else { 'Off' }
        $dcOn = if ($dc -ne '0x00000000') { 'On' } else { 'Off' }
        Write-Output ("{0}: AC={1}  DC={2}" -f $s.Name, $acOn, $dcOn)
        if ($acOn -eq 'On' -or $dcOn -eq 'On') { $anyEnabled = $true }
    }
}
$states = (powercfg /availablesleepstates 2>&1) -join "`n"
if ($states -match 'Hibernate') { Write-Output 'Hibernation available: YES' } else { Write-Output 'Hibernation available: NO' }
if ($anyEnabled) { Write-Output 'Sleep/Hibernation: ENABLED on at least one setting' } else { Write-Output 'Sleep/Hibernation: all timeouts at 0 (disabled)' }
"#;
                            let output = tokio::process::Command::new("powershell")
                                .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", ps_cmd])
                                .output()
                                .await;
                            match output {
                                Ok(out) => {
                                    let stdout = String::from_utf8_lossy(&out.stdout);
                                    let stderr = String::from_utf8_lossy(&out.stderr);
                                    for line in stdout.lines() {
                                        if !line.trim().is_empty() { send_log(&tx, line.to_string()); }
                                    }
                                    for line in stderr.lines() {
                                        if !line.trim().is_empty() { send_log(&tx, format!("[stderr] {line}")); }
                                    }
                                    if out.status.success() {
                                        send_result(&tx, &script.name, RemoteScriptStatus::Success);
                                    } else {
                                        send_log(&tx, format!("Exit code: {:?}", out.status.code()));
                                        send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                    }
                                }
                                Err(e) => {
                                    send_log(&tx, format!("Error querying power options: {e}"));
                                    send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                }
                            }
                        }

                        "Check Updates" => {
                            send_log(&tx, "Checking internet before Windows Update search...".into());
                            match crate::utilities::windows::net_adapter::ensure_internet_connected().await {
                                Ok(_) => send_log(&tx, "Internet confirmed".into()),
                                Err(e) => {
                                    send_log(&tx, format!("No internet: {e}"));
                                    send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                    return;
                                }
                            }
                            send_log(&tx, "Searching for available Windows updates (no install)...".into());

                            let (update_tx, update_rx) = crossbeam::channel::unbounded();
                            let handle = std::thread::spawn(move || {
                                crate::utilities::windows::windows_update::install_windows_updates(
                                    update_tx, false, false,
                                )
                            });

                            loop {
                                use crate::utilities::windows::windows_update::WindowsUpdateEvent;
                                while let Ok(event) = update_rx.try_recv() {
                                    match event {
                                        WindowsUpdateEvent::UpdateLogs(msg) => send_log(&tx, msg),
                                        WindowsUpdateEvent::DownloadPercentage(_) | WindowsUpdateEvent::InstallPercentage(_) => {}
                                        WindowsUpdateEvent::ReturnedUpdates(updates) => {
                                            let pending: Vec<_> = updates.updates.iter().filter(|u| !u.is_installed).collect();
                                            send_log(&tx, format!(
                                                "{} updates returned ({} pending, {} already installed)",
                                                updates.updates.len(),
                                                pending.len(),
                                                updates.updates.len() - pending.len()
                                            ));
                                            for u in &pending {
                                                send_log(&tx, format!("  [pending] {}", u.title));
                                            }
                                        }
                                    }
                                }
                                if handle.is_finished() {
                                    break;
                                }
                                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                            }

                            match handle.join() {
                                Ok(Ok(_)) => {
                                    send_log(&tx, "Windows update check finished".into());
                                    send_result(&tx, &script.name, RemoteScriptStatus::Success);
                                }
                                Ok(Err(e)) => {
                                    send_log(&tx, format!("Windows update check error: {e:?}"));
                                    send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                }
                                Err(_) => {
                                    send_log(&tx, "Windows update check thread panicked".into());
                                    send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                }
                            }
                        }

                        "Run Prechecks" => {
                            let mut all_ok = true;
                            match crate::utilities::windows::registry::disable_notifications() {
                                Ok(results) => for r in &results { send_log(&tx, format!("notifications => {r}")); }
                                Err(e) => { send_log(&tx, format!("notifications error: {e}")); all_ok = false; }
                            }
                            match crate::utilities::windows::registry::align_taskbar_left() {
                                Ok(results) => for r in &results { send_log(&tx, format!("taskbar => {}", r.trim())); }
                                Err(e) => { send_log(&tx, format!("taskbar error: {e}")); all_ok = false; }
                            }
                            match crate::utilities::windows::net_adapter::scan_wifi_networks() {
                                Ok(networks) => send_log(&tx, format!("wifi networks visible: {}", networks.len())),
                                Err(e) => { send_log(&tx, format!("wifi scan error: {e}")); all_ok = false; }
                            }
                            match crate::utilities::windows::net_adapter::get_wlan_status() {
                                Ok(_) => send_log(&tx, "wlan status: OK".into()),
                                Err(e) => send_log(&tx, format!("wlan status: {e:?}")),
                            }
                            match crate::utilities::windows::net_adapter::check_network_adapters() {
                                Ok(adapters) => send_log(&tx, format!("network adapters: {adapters:?}")),
                                Err(e) => { send_log(&tx, format!("adapter check error: {e}")); all_ok = false; }
                            }
                            send_result(&tx, &script.name, if all_ok { RemoteScriptStatus::Success } else { RemoteScriptStatus::Failed });
                        }

                        "Run SuperAntiSpyware Scan" => {
                            let (sas_tx, sas_rx) = tokio::sync::oneshot::channel();
                            std::thread::spawn(move || {
                                let _ = sas_tx.send(crate::utilities::scripts::antivirus::run_sas_quick_scan());
                            });
                            match sas_rx.await {
                                Ok(Ok(msgs)) => {
                                    for m in msgs { send_log(&tx, m); }
                                    send_result(&tx, &script.name, RemoteScriptStatus::Success);
                                }
                                Ok(Err(e)) => {
                                    send_log(&tx, format!("SAS scan: {e}"));
                                    send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                }
                                Err(_) => {
                                    send_log(&tx, "SAS scan worker dropped".into());
                                    send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                }
                            }
                        }

                        "Run Webroot Scan" => {
                            match crate::utilities::scripts::antivirus::start_webroot_scan() {
                                Ok(msg) => {
                                    send_log(&tx, msg);
                                    send_result(&tx, &script.name, RemoteScriptStatus::Success);
                                }
                                Err(e) => {
                                    send_log(&tx, format!("Webroot scan: {e}"));
                                    send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                }
                            }
                        }

                        "Disable Startup Apps" => {
                            let mut ok = true;
                            match crate::utilities::scripts::disable_hkcu_startup_entries("msedge") {
                                Ok(msgs) => for m in msgs { send_log(&tx, format!("Edge: {m}")); },
                                Err(e) => { ok = false; send_log(&tx, format!("Edge startup: {e}")); }
                            }
                            if crate::utilities::scripts::onedrive_in_use() {
                                send_log(&tx, "OneDrive has a signed-in account; leaving its startup entry enabled.".into());
                            } else {
                                match crate::utilities::scripts::disable_hkcu_startup_entries("onedrive") {
                                    Ok(msgs) => for m in msgs { send_log(&tx, format!("OneDrive: {m}")); },
                                    Err(e) => { ok = false; send_log(&tx, format!("OneDrive startup: {e}")); }
                                }
                                use std::os::windows::process::CommandExt;
                                let _ = std::process::Command::new("taskkill")
                                    .args(["/F", "/IM", "OneDrive.exe"])
                                    .creation_flags(0x08000000)
                                    .output();
                                send_log(&tx, "OneDrive not signed in: killed OneDrive.exe".into());
                            }
                            send_result(&tx, &script.name, if ok { RemoteScriptStatus::Success } else { RemoteScriptStatus::Failed });
                        }

                        "Disable proxy settings" | "Data Transfer"
                        | "When Was The Last Service Date?"
                        | "Are there scheduled tasks for it?"
                        | "Run Junkware Category" => {
                            send_log(&tx, format!("'{}' not yet implemented for remote execution", script.name));
                            send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                        }

                        name if stress_runner::is_benchmark_script(name) => {
                            use std::sync::Arc;
                            use stress_kit::telemetry::TelemetryAgent;

                            send_log(&tx, format!("{name}: running scored benchmark(s) via stress-runner (persisted)"));

                            enum BenchMsg {
                                Log(String),
                                Done(bool),
                            }

                            let script_name = script.name.clone();
                            let (bench_tx, bench_rx) = crossbeam::channel::unbounded::<BenchMsg>();

                            std::thread::spawn(move || {
                                let client = crate::filesystem::get_client_hash();
                                let computer = match client.computer.clone() {
                                    Some(c) => c,
                                    None => {
                                        let _ = bench_tx.send(BenchMsg::Log(
                                            "get_client_hash returned no computer record".into(),
                                        ));
                                        let _ = bench_tx.send(BenchMsg::Done(false));
                                        return;
                                    }
                                };
                                let telemetry = Arc::new(TelemetryAgent::start(1000));
                                std::thread::sleep(std::time::Duration::from_millis(1500));
                                let include_gpu = !telemetry.snapshot().gpus.is_empty();
                                let secs = stress_runner::DEFAULT_BENCH_SECS;
                                let _ = bench_tx.send(BenchMsg::Log(format!(
                                    "{secs}s per benchmark, gpu kinds {}",
                                    if include_gpu { "included" } else { "skipped (no GPU)" }
                                )));

                                let Some(outcomes) = stress_runner::run_benchmark_script(
                                    &script_name,
                                    computer,
                                    telemetry,
                                    secs,
                                    include_gpu,
                                ) else {
                                    let _ = bench_tx.send(BenchMsg::Log(format!(
                                        "Unknown benchmark script '{script_name}'"
                                    )));
                                    let _ = bench_tx.send(BenchMsg::Done(false));
                                    return;
                                };

                                let mut success = true;
                                let mut no_sample_kinds: Vec<String> = Vec::new();
                                for o in &outcomes {
                                    if o.errors > 0 || o.error.is_some() {
                                        success = false;
                                    }
                                    if o.status == stress_runner::BenchmarkStatus::NoSamples {
                                        no_sample_kinds.push(o.kind.clone());
                                    }
                                    let _ = bench_tx.send(BenchMsg::Log(format!(
                                        "{}: {:.1} {} (peak {:.1}) errors={}{}{}{}",
                                        o.kind,
                                        o.score,
                                        o.unit,
                                        o.peak.unwrap_or(o.score),
                                        o.errors,
                                        if o.status == stress_runner::BenchmarkStatus::NoSamples {
                                            " — status: no_samples (not scored)"
                                        } else {
                                            ""
                                        },
                                        o.result_id
                                            .as_deref()
                                            .map(|id| format!(" [{id}]"))
                                            .unwrap_or_default(),
                                        o.error
                                            .as_deref()
                                            .map(|e| format!(" — {e}"))
                                            .unwrap_or_default(),
                                    )));
                                }
                                let summary = if !success {
                                    "errors detected".to_string()
                                } else if no_sample_kinds.is_empty() {
                                    "all clean".to_string()
                                } else {
                                    format!("clean, no samples from: {}", no_sample_kinds.join(", "))
                                };
                                let _ = bench_tx.send(BenchMsg::Log(format!(
                                    "{} benchmark(s) complete, {summary}",
                                    outcomes.len(),
                                )));
                                let _ = bench_tx.send(BenchMsg::Done(success));
                            });

                            let mut final_success: Option<bool> = None;
                            while final_success.is_none() {
                                while let Ok(msg) = bench_rx.try_recv() {
                                    match msg {
                                        BenchMsg::Log(line) => send_log(&tx, line),
                                        BenchMsg::Done(ok) => final_success = Some(ok),
                                    }
                                }
                                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                            }
                            send_result(
                                &tx,
                                &script.name,
                                if final_success.unwrap_or(false) { RemoteScriptStatus::Success } else { RemoteScriptStatus::Failed },
                            );
                        }

                        name if stress_runner::is_stress_script(name) => {
                            use std::sync::Arc;
                            use stress_kit::telemetry::TelemetryAgent;
                            use stress_runner::{build_stress_script_spec, drive_blocking, RunResult, RunUpdate};

                            if service_number.trim().is_empty() {
                                send_log(
                                    &tx,
                                    format!(
                                        "{name}: service_number is required so stress_test_run carries service_order / customer / computer linkage — aborting."
                                    ),
                                );
                                send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                return;
                            }

                            send_log(&tx, format!("{name}: running via stress-runner (persisted)"));

                            enum ProbeMsg {
                                Log(String),
                                Done(bool),
                            }

                            let service_number_probe = service_number.clone();
                            let diag_session_probe = diagnostic_session_id.clone();
                            let script_name = script.name.clone();
                            let label = script.name.clone();
                            let (probe_tx, probe_rx) = crossbeam::channel::unbounded::<ProbeMsg>();

                            std::thread::spawn(move || {
                                let client = crate::filesystem::get_client_hash();
                                let computer = match client.computer.clone() {
                                    Some(c) => c,
                                    None => {
                                        let _ = probe_tx.send(ProbeMsg::Log(
                                            "get_client_hash returned no computer record".into(),
                                        ));
                                        let _ = probe_tx.send(ProbeMsg::Done(false));
                                        return;
                                    }
                                };
                                let mut spec = match build_stress_script_spec(&script_name, computer, 60) {
                                    Some(s) => s,
                                    None => {
                                        let _ = probe_tx.send(ProbeMsg::Log(format!(
                                            "Unknown stress script '{}'", script_name
                                        )));
                                        let _ = probe_tx.send(ProbeMsg::Done(false));
                                        return;
                                    }
                                };
                                let telemetry = Arc::new(TelemetryAgent::start(1000));
                                spec.tags.push("origin:remote_scripts".into());
                                spec.hostname = std::env::var("COMPUTERNAME")
                                    .or_else(|_| std::env::var("HOSTNAME"))
                                    .ok();
                                spec.machine_id = Some(client.client_hash.clone());
                                if !service_number_probe.is_empty() {
                                    spec.service_order = Some(database::schema::RecordId::new(
                                        database::schema::TICKET_TABLE,
                                        service_number_probe,
                                    ));
                                }
                                if !diag_session_probe.is_empty() {
                                    spec.session_ref = Some(
                                        database::schema::entity_link::parse_record_id(
                                            &diag_session_probe,
                                            database::schema::DIAGNOSTIC_SESSION_TABLE,
                                        ),
                                    );
                                }

                                let mut success = false;
                                drive_blocking(spec, telemetry, |update| match update {
                                    RunUpdate::Started { run_id } => {
                                        use database::schema::RecordIdExt;
                                        let _ = probe_tx.send(ProbeMsg::Log(format!(
                                            "stress_test_run id: {}",
                                            run_id.key_string()
                                        )));
                                    }
                                    RunUpdate::StageStarted { index, label: stage_label, stage_count } => {
                                        if stage_count > 1 {
                                            let _ = probe_tx.send(ProbeMsg::Log(format!(
                                                "Stage {}/{}: {stage_label}", index + 1, stage_count
                                            )));
                                        }
                                    }
                                    RunUpdate::Tick { metrics, stage_label, .. } => {
                                        if let Some(err) = metrics.last_error.as_ref() {
                                            let stage = stage_label.unwrap_or_else(|| "single".into());
                                            let _ = probe_tx.send(ProbeMsg::Log(format!("{stage}: {err}")));
                                        }
                                    }
                                    RunUpdate::StageFinished { .. } => {}
                                    RunUpdate::StageVerdict { index, label: stage_label, pass, violations, .. } => {
                                        let _ = probe_tx.send(ProbeMsg::Log(format!(
                                            "{label} stage {} '{stage_label}': {}",
                                            index + 1,
                                            if pass { "PASS" } else { "FAIL" }
                                        )));
                                        for violation in violations {
                                            let _ = probe_tx.send(ProbeMsg::Log(format!(
                                                "{label} stage {} violation: {violation}",
                                                index + 1
                                            )));
                                        }
                                    }
                                    RunUpdate::Finished(v) => {
                                        success = v.result == RunResult::Pass;
                                        let result_str = match v.result {
                                            RunResult::Pass => "PASSED",
                                            RunResult::Fail => "FAILED",
                                            RunResult::Aborted => "ABORTED",
                                            RunResult::Inconclusive => "INCONCLUSIVE",
                                            RunResult::InProgress => "IN_PROGRESS",
                                        };
                                        let _ = probe_tx.send(ProbeMsg::Log(format!(
                                            "{label} {result_str} in {:.1}s (run persisted)",
                                            v.duration_secs
                                        )));
                                    }
                                    RunUpdate::Warning { message } => {
                                        let _ = probe_tx.send(ProbeMsg::Log(format!("{label} warning: {message}")));
                                    }
                                    RunUpdate::Error { message } => {
                                        let _ = probe_tx.send(ProbeMsg::Log(format!("{label} error: {message}")));
                                    }
                                });
                                let _ = probe_tx.send(ProbeMsg::Done(success));
                            });

                            let mut final_success: Option<bool> = None;
                            while final_success.is_none() {
                                while let Ok(msg) = probe_rx.try_recv() {
                                    match msg {
                                        ProbeMsg::Log(line) => send_log(&tx, line),
                                        ProbeMsg::Done(ok) => final_success = Some(ok),
                                    }
                                }
                                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                            }
                            send_result(
                                &tx,
                                &script.name,
                                if final_success.unwrap_or(false) { RemoteScriptStatus::Success } else { RemoteScriptStatus::Failed },
                            );
                        }

                        _ => {
                            if let Some(content) = &script.content {
                                send_log(&tx, format!("Running custom script: {}", script.name));
                                let ext = if script.name.ends_with(".bat") || script.name.ends_with(".cmd") {
                                    "bat"
                                } else {
                                    "ps1"
                                };
                                let temp_dir = std::env::temp_dir();
                                let script_file = temp_dir.join(format!("mastertech_custom_{}.{}", uuid::Uuid::new_v4(), ext));
                                if let Err(e) = std::fs::write(&script_file, content) {
                                    send_log(&tx, format!("Failed to write script: {e}"));
                                    send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                    return;
                                }

                                let output = if ext == "ps1" {
                                    tokio::process::Command::new("powershell")
                                        .args(["-ExecutionPolicy", "Bypass", "-File", &script_file.to_string_lossy()])
                                        .output()
                                        .await
                                } else {
                                    tokio::process::Command::new("cmd")
                                        .args(["/C", &script_file.to_string_lossy()])
                                        .output()
                                        .await
                                };

                                let _ = std::fs::remove_file(&script_file);

                                match output {
                                    Ok(out) => {
                                        let stdout = String::from_utf8_lossy(&out.stdout);
                                        let stderr = String::from_utf8_lossy(&out.stderr);
                                        if !stdout.is_empty() {
                                            for line in stdout.lines() {
                                                send_log(&tx, line.to_string());
                                            }
                                        }
                                        if !stderr.is_empty() {
                                            for line in stderr.lines() {
                                                send_log(&tx, format!("[stderr] {}", line));
                                            }
                                        }
                                        if out.status.success() {
                                            send_result(&tx, &script.name, RemoteScriptStatus::Success);
                                        } else {
                                            send_log(&tx, format!("Exit code: {:?}", out.status.code()));
                                            send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                        }
                                    }
                                    Err(e) => {
                                        send_log(&tx, format!("Execution error: {e}"));
                                        send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                    }
                                }
                            } else if script.category == "Junkware Removal" {
                                send_log(&tx, format!("Attempting to uninstall: {}", script.name));
                                match crate::utilities::scripts::programs::InstalledProgram::get_by_name(&script.name) {
                                    Ok(Some(program)) => {
                                        let _ = program.uninstall();
                                        send_log(&tx, format!("Uninstall initiated for {}", script.name));
                                        send_result(&tx, &script.name, RemoteScriptStatus::Success);
                                    }
                                    Ok(None) => {
                                        send_log(&tx, format!("{} not found / already removed", script.name));
                                        send_result(&tx, &script.name, RemoteScriptStatus::Success);
                                    }
                                    Err(e) => {
                                        send_log(&tx, format!("Error: {e}"));
                                        send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                                    }
                                }
                            } else {
                                send_log(&tx, format!("Unknown script: {}", script.name));
                                send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                            }
                        }
                    }
                    };
                    // Per-script budget so one hung script can't stall the batch forever.
                    if tokio::time::timeout(Duration::from_secs(allowed_secs), script_fut)
                        .await
                        .is_err()
                    {
                        send_log(
                            &tx,
                            format!(
                                "{}: exceeded {allowed_secs}s planned timeout — abandoning script",
                                script.name
                            ),
                        );
                        send_result(&tx, &script.name, RemoteScriptStatus::Failed);
                    }
                }

                if batch_reboot_recommended.load(std::sync::atomic::Ordering::SeqCst) {
                    send_log(&tx, format!(
                        "{} Webroot was re-keyed over an existing install — reboot the client to finalize activation (Power ▸ Reboot relaunches MasterTech)",
                        displays::scripts::REBOOT_RECOMMENDED_MARKER
                    ));
                }

                let complete = Cmd::RemoteScriptsComplete;
                if let Ok(payload) = encode_to_vec(&complete, standard()) {
                    let _ = tx.send(payload);
                }
                }); // end tokio::spawn — returns immediately so the TCP session loop
                    // can continue processing Ping/AppPing while scripts run.
            }

            Cmd::RunRemoteScenario {
                stages,
                total_wall_secs,
                repeat_until_total,
                service_number,
                diagnostic_session_id,
                preset_label,
                notes,
            } => {
                run_remote_stress_plan(
                    self.command_tx.clone(),
                    RemoteStressPlanRequest::Scenario {
                        stages,
                        total_wall_secs,
                        repeat_until_total,
                    },
                    service_number,
                    diagnostic_session_id,
                    preset_label,
                    notes,
                );
            }

            Cmd::RunRemoteConcurrent {
                lanes,
                duration_secs,
                service_number,
                diagnostic_session_id,
                preset_label,
                notes,
            } => {
                run_remote_stress_plan(
                    self.command_tx.clone(),
                    RemoteStressPlanRequest::Concurrent {
                        lanes,
                        duration_secs,
                    },
                    service_number,
                    diagnostic_session_id,
                    preset_label,
                    notes,
                );
            }

            Cmd::RunScriptContent { filename, content } => {
                log::info!("RunScriptContent: filename={filename}");
                let ext = filename.rsplit('.').next().unwrap_or("").to_lowercase();

                let send_log = |sender: &mut ClientTransport, msg: String| {
                    if let Ok(payload) = encode_to_vec(&Cmd::RemoteScriptLog(msg), standard()) {
                        sender.send(WsMessage::Binary(payload));
                    }
                };

                send_log(sender, format!("Running script: {filename}"));

                if !matches!(ext.as_str(), "ps1" | "bat" | "cmd") {
                    send_log(sender, format!("Unsupported script type: .{ext}"));
                    return;
                }
                // Run the interpreter on a blocking thread.
                let ext_for_run = ext.clone();
                let join = tokio::task::spawn_blocking(move || match ext_for_run.as_str() {
                    "ps1" => std::process::Command::new("powershell")
                        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &content])
                        .output(),
                    _ => std::process::Command::new("cmd").args(["/C", &content]).output(),
                })
                .await;

                match join {
                    Ok(Ok(out)) => {
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        let stderr = String::from_utf8_lossy(&out.stderr);
                        if !stdout.is_empty() {
                            send_log(sender, stdout.to_string());
                        }
                        if !stderr.is_empty() {
                            send_log(sender, format!("[stderr] {stderr}"));
                        }
                        send_log(sender, format!("Script {filename} exited with code: {}", out.status));
                    }
                    Ok(Err(e)) => {
                        send_log(sender, format!("Failed to run script {filename}: {e}"));
                    }
                    Err(e) => {
                        send_log(sender, format!("Script {filename} task panicked: {e}"));
                    }
                }
            }

            Cmd::LoadWasmPlugin { plugin_id, wasm_bytes } => {
                let size = wasm_bytes.len();
                log::info!("Received remote WASM plugin '{plugin_id}' ({size} bytes)");
                let tx = displays::plugins::wasm_load_sender();
                let result_cmd = if tx.try_send((plugin_id.clone(), wasm_bytes)).is_ok() {
                    Cmd::LoadWasmPluginResult {
                        plugin_id,
                        success: true,
                        message: format!("Plugin queued for loading ({size} bytes)"),
                    }
                } else {
                    Cmd::LoadWasmPluginResult {
                        plugin_id,
                        success: false,
                        message: "WASM load channel full or disconnected".to_string(),
                    }
                };
                if let Ok(payload) = encode_to_vec(&result_cmd, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }

            Cmd::SetFrameCapture { enabled } => {
                log::info!("SetFrameCapture received: enabled={enabled}");
                let tx = displays::plugins::frame_capture_sender();
                let _ = tx.try_send(enabled);
            }

            Cmd::CallRemotePluginTool { request_id, plugin_id, tool_name, args_json } => {
                log::info!("CallRemotePluginTool: {plugin_id}::{tool_name} req={request_id}");
                let call_tx = displays::plugins::remote_tool_call_sender();
                let _ = call_tx.try_send((request_id.clone(), plugin_id.clone(), tool_name.clone(), args_json));
                let result_rx = displays::plugins::remote_tool_result_receiver();
                let mut result: Option<(bool, String)> = None;
                // Poll up to 90s (9000 * 10ms) for the background plugin result.
                for _ in 0..9000 {
                    if let Ok((rid, success, rjson)) = result_rx.try_recv() {
                        if rid == request_id {
                            result = Some((success, rjson));
                            break;
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                let (success, result_json) = result.unwrap_or((
                    false,
                    "PluginManager did not return a result within 90 seconds".to_string(),
                ));
                let result_cmd = Cmd::RemotePluginToolResult {
                    request_id,
                    plugin_id,
                    tool_name,
                    success,
                    result_json,
                };
                if let Ok(payload) = encode_to_vec(&result_cmd, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }

            Cmd::AnalyzeCrashDumps { request_id, paths } => {
                log::info!(
                    "AnalyzeCrashDumps: req={request_id} targets={}",
                    paths.as_ref().map(|p| p.len()).unwrap_or(0)
                );
                let result_json = tokio::task::spawn_blocking(move || {
                    let files: Vec<std::path::PathBuf> = match paths {
                        Some(ps) if !ps.is_empty() => {
                            ps.into_iter().map(std::path::PathBuf::from).collect()
                        }
                        _ => enumerate_crash_dumps(),
                    };
                    let mut dumps: Vec<serde_json::Value> = Vec::with_capacity(files.len());
                    let mut triages: Vec<dump_triage::KernelDumpTriage> = Vec::new();
                    for p in &files {
                        let dump_name =
                            p.file_name().map(|f| f.to_string_lossy().to_string());
                        match dump_triage::analyze_file(p) {
                            Ok(triage) => {
                                triages.push(triage.clone());
                                dumps.push(serde_json::json!({
                                    "dump_name": dump_name,
                                    "path": p.to_string_lossy(),
                                    "triage": triage,
                                }));
                            }
                            Err(e) => dumps.push(serde_json::json!({
                                "dump_name": dump_name,
                                "path": p.to_string_lossy(),
                                "error": e,
                            })),
                        }
                    }
                    let cross = dump_triage::diff::baseline_diffs(&triages);
                    serde_json::json!({ "count": dumps.len(), "dumps": dumps, "cross_dump": cross }).to_string()
                })
                .await
                .unwrap_or_else(|e| {
                    serde_json::json!({ "error": format!("analysis task panicked: {e}") })
                        .to_string()
                });

                let result_cmd = Cmd::RemotePluginToolResult {
                    request_id,
                    plugin_id: "native.crash-analysis".to_string(),
                    tool_name: "analyze_crash_dumps".to_string(),
                    success: true,
                    result_json,
                };
                if let Ok(payload) = encode_to_vec(&result_cmd, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }

            Cmd::DirectFileTransfer { filename, chunk_index, total_chunks, data } => {
                log::info!("DirectFileTransfer: {filename} chunk {chunk_index}/{total_chunks} ({} bytes)", data.len());
                let entry = self.file_transfer_buffers
                    .entry(filename.clone())
                    .or_insert_with(|| (total_chunks, Vec::new()));
                entry.1.push((chunk_index, data));

                if entry.1.len() as u32 == total_chunks {
                    let (_, mut chunks) = self.file_transfer_buffers.remove(&filename).unwrap();
                    chunks.sort_by_key(|(idx, _)| *idx);
                    let full_data: Vec<u8> = chunks.into_iter().flat_map(|(_, d)| d).collect();
                    let size = full_data.len();

                    let transfer_dir = std::env::var("USERPROFILE")
                        .map(|p| std::path::PathBuf::from(p).join("Desktop"))
                        .unwrap_or_else(|_| std::env::temp_dir());
                    let _ = std::fs::create_dir_all(&transfer_dir);
                    let save_path = transfer_dir.join(&filename);
                    let result_cmd = match std::fs::write(&save_path, &full_data) {
                        Ok(()) => {
                            log::info!("File saved: {} ({size} bytes)", save_path.display());
                            Cmd::DirectFileTransferResult {
                                filename,
                                success: true,
                                message: format!("Saved to {} ({size} bytes)", save_path.display()),
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to save file {}: {e}", save_path.display());
                            Cmd::DirectFileTransferResult {
                                filename,
                                success: false,
                                message: format!("Write failed: {e}"),
                            }
                        }
                    };
                    if let Ok(payload) = encode_to_vec(&result_cmd, standard()) {
                        sender.send(WsMessage::Binary(payload));
                    }
                }
            }

            // ── Open-service-order auto-link (Stage 2) ────────────────────
            //
            // Pull-based: the admin asks for the cached PrestaShop
            // candidates the client built at startup (and the live
            // ComputerData snapshot it has on hand), the client replies
            // with the bundle.  `refresh = true` forces a fresh
            // PrestaShop fetch (the admin's "Refresh suggestions"
            // button); otherwise we serve from the in-memory cache.
            Cmd::RequestOpenServiceCandidates { refresh } => {
                use crate::filesystem::customer_lookup::{
                    get_open_service_cache, lookup_customer_and_open_orders,
                    set_open_service_cache, CachedOpenServiceLookup,
                };
                use crate::filesystem::oa_serial::{
                    get_oa_style_serial, to_oa3_13digit,
                };

                if refresh {
                    log::info!(
                        "Cmd::RequestOpenServiceCandidates: refresh=true \
                         — re-running PrestaShop lookup"
                    );
                    let serial13 = match get_oa_style_serial()
                        .and_then(|raw| to_oa3_13digit(&raw))
                    {
                        Ok(s) => s,
                        Err(e) => {
                            log::warn!(
                                "Cmd::RequestOpenServiceCandidates: \
                                 OA serial unavailable: {e:?}"
                            );
                            String::new()
                        }
                    };
                    if !serial13.is_empty() {
                        match lookup_customer_and_open_orders(&serial13).await {
                            Ok((match_, candidates)) => {
                                set_open_service_cache(CachedOpenServiceLookup {
                                    match_: Some(match_),
                                    candidates,
                                    resolved_at: std::time::SystemTime::now(),
                                });
                            }
                            Err(e) => {
                                log::warn!(
                                    "Cmd::RequestOpenServiceCandidates: \
                                     refresh lookup failed: {e:?}"
                                );
                            }
                        }
                    }
                }

                let cached = get_open_service_cache();
                // `get_sysinfo()` (vs `_no_gpu`) so live_specs.gpu_info
                // is populated — the admin's entity-link modal maps
                // it through to the form's GPU field. This handler
                // only fires on session open / explicit refresh, so
                // the ~hundreds-of-ms GPU enumeration is acceptable
                // here (unlike the 400ms live-stats loop below, which
                // still uses `_no_gpu`).
                let live_specs = get_sysinfo().await.ok();
                let response = Cmd::OpenServiceCandidatesResponse {
                    match_: cached.as_ref().and_then(|c| c.match_.clone()),
                    candidates: cached
                        .as_ref()
                        .map(|c| c.candidates.clone())
                        .unwrap_or_default(),
                    live_specs,
                };
                if let Ok(payload) = encode_to_vec(&response, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }

            // Client → admin response is sender-side only; if the admin
            // somehow echoes one back, drop it on the floor.
            Cmd::OpenServiceCandidatesResponse { .. } => {}

            // ── Application-layer heartbeat ──────────────────────────────
            //
            // Admin sends `AppPing` on a timer (~every 15 s) so it can
            // detect plugin-host wedges that leave the kernel TCP socket
            // technically alive.  We echo back `AppPong` with the same
            // nonce + send time; the admin compares and times any drift.
            // The whole round-trip flows through `handle_command` on
            // both ends, so if the wasm PluginManager or the egui
            // dispatcher is hung the pong stops arriving even though
            // TCP keepalive still says "fine."
            Cmd::AppPing { nonce, sent_at_ms } => {
                let response = Cmd::AppPong { nonce, sent_at_ms };
                if let Ok(payload) = encode_to_vec(&response, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }
            // Client never sends AppPing in the current shape; if one
            // arrives (admin echoing its own pong, future bidirectional
            // heartbeat), just drop.
            Cmd::AppPong { .. } => {}

            Cmd::None => {},
            // ── Remote self-update (terminal-mode path) ──────────────────
            Cmd::MastertechSelfUpdateChunk { chunk_index, total_chunks, data } => {
                log::info!(
                    "[self-update] chunk {}/{} ({} bytes) via terminal WS",
                    chunk_index + 1,
                    total_chunks,
                    data.len(),
                );
                if let Some(bytes) = self.self_update_buffer.push(chunk_index, total_chunks, data) {
                    log::info!("[self-update] all {} chunks received — applying…", total_chunks);
                    const RECONNECT_HINT_SECS: u32 = 15;
                    let relaunch_cmd = displays::Cmd::MastertechSelfUpdateRelaunching {
                        reconnect_hint_secs: RECONNECT_HINT_SECS,
                    };
                    if let Ok(payload) = encode_to_vec(&relaunch_cmd, standard()) {
                        sender.send(WsMessage::Binary(payload));
                    }
                    let apply_result = tokio::task::spawn_blocking(move || {
                        crate::remote_self_update::apply_and_relaunch(bytes)
                    })
                    .await;
                    let (success, message) = match apply_result {
                        Ok(pair) => pair,
                        Err(e) => (false, format!("apply task failed: {e}")),
                    };
                    log::info!("[self-update] result: success={success} message={message}");
                    let result_cmd = displays::Cmd::MastertechSelfUpdateResult { success, message };
                    if let Ok(payload) = encode_to_vec(&result_cmd, standard()) {
                        sender.send(WsMessage::Binary(payload));
                    }
                    if success {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        std::process::exit(0);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Convert HBITMAP to PNG bytes (Windows only)
#[cfg(target_os = "windows")]
pub fn hbitmap_to_png_bytes(
    hbmp: windows::Win32::Graphics::Gdi::HBITMAP,
) -> Result<Vec<u8>, String> {
    use windows::Win32::Graphics::Gdi::*;
    
    let mut bmp = BITMAP::default();
    if unsafe { GetObjectW(
        HGDIOBJ(hbmp.0),
        std::mem::size_of::<BITMAP>() as i32,
        Some(&mut bmp as *mut _ as *mut _),
    ) } == 0
    {
        let _ = unsafe { DeleteObject(HGDIOBJ(hbmp.0)) };
        return Err("GetObjectW failed".into());
    }
    
    let width = bmp.bmWidth as i32;
    let height = bmp.bmHeight as i32;
    let mut bi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height, // Top-down DIB
            biPlanes: 1,
            biBitCount: 32,
            biCompression: 0,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: Default::default(),
    };
    
    let stride = (width * 4) as usize;
    let mut buffer = vec![0u8; stride * height as usize];
    let hdc: HDC = unsafe { CreateCompatibleDC(None) };
    
    if hdc.0.is_null() {
        let _ = unsafe { DeleteObject(HGDIOBJ(hbmp.0)) };
        return Err("CreateCompatibleDC failed".into());
    }
    
    let _old = unsafe { SelectObject(hdc, HGDIOBJ(hbmp.0)) };
    let got = unsafe { GetDIBits(
        hdc,
        hbmp,
        0,
        height as u32,
        Some(buffer.as_mut_ptr() as *mut _),
        &mut bi as *mut _,
        DIB_RGB_COLORS,
    ) };
    
    let _ = unsafe { DeleteDC(hdc) };
    let _ = unsafe { DeleteObject(HGDIOBJ(hbmp.0)) };
    
    if got == 0 {
        return Err("GetDIBits failed".into());
    }
    
    // Convert BGRA to RGBA
    for px in buffer.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    
    let img = image::RgbaImage::from_raw(width as u32, height as u32, buffer)
        .ok_or("rgba from raw failed")?;
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    
    Ok(png)
}

pub async fn live_computer_stats(tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>, mut stop_rx: tokio::sync::watch::Receiver<bool>) -> anyhow::Result<(), anyhow::Error> {
    // Cadence: 400 ms when idle so the admin's charts stay responsive,
    // 2 s when a stress run is in flight so we don't oversaturate the
    // TCP connection — chatty live telemetry will starve WASM plugin
    // RPC, Cmd::CallRemotePluginTool responses, etc. The check is
    // cheap (single atomic load) and re-evaluated every tick.
    const IDLE_INTERVAL: Duration = Duration::from_millis(400);
    const STRESS_INTERVAL: Duration = Duration::from_millis(2000);

    loop {
        let interval = if stress_runner::is_stress_active() {
            STRESS_INTERVAL
        } else {
            IDLE_INTERVAL
        };
        tokio::select! {
            _ = stop_rx.changed() => {
                if *stop_rx.borrow() {
                    log::info!("live_computer_stats: received stop signal");
                    break;
                }
            }
            _ = tokio::time::sleep(interval) => {
                match crate::filesystem::system_info::get_sysinfo_no_gpu().await {
                    Ok(mut systeminfo) => {
                        // Enrich the bare sysinfo with the data the
                        // shared `stress-kit` telemetry agent already
                        // collects in its own thread:
                        //   - GPU info via NVML (sysinfo Components
                        //     misses GPUs on Windows without vendor
                        //     drivers, so `get_sysinfo_no_gpu` is
                        //     pessimistic by default).
                        //   - WHEA / TDR counters (Windows error and
                        //     GPU-timeout running totals that don't
                        //     surface anywhere else in `SystemInformation`).
                        let snapshot = crate::filesystem::system_info::current_telemetry_snapshot();

                        if systeminfo.gpu_info.card.is_empty() && !snapshot.gpus.is_empty() {
                            use database::schema::GraphicsCard;
                            systeminfo.gpu_info.card = snapshot
                                .gpus
                                .iter()
                                .enumerate()
                                .map(|(i, g)| GraphicsCard {
                                    id: i.to_string(),
                                    name: g.name.clone(),
                                    brand: g.vendor.clone(),
                                    memory: g.memory_total_mb.unwrap_or(0).saturating_mul(1024 * 1024),
                                    temperature: g.temp_c.unwrap_or(0.0) as u32,
                                    ..Default::default()
                                })
                                .collect();
                        }
                        if let Some(w) = snapshot.whea {
                            systeminfo.whea = Some(database::schema::WheaCounters {
                                delta_since_program_start: w.delta_since_program_start,
                                absolute_since_boot: w.total_retained,
                            });
                        }
                        if let Some(t) = snapshot.tdr {
                            systeminfo.tdr = Some(database::schema::TdrCounters {
                                delta_since_program_start: t.delta_since_program_start,
                                absolute_since_boot: t.absolute_since_boot,
                            });
                        }
                        // ACPI thermal zones from the WMI fallback —
                        // sysinfo's Component temperature surface goes
                        // empty on modern Windows so `component_temps`
                        // arrives blank by default. Merge in the
                        // telemetry-agent readings; live entries win
                        // (in case any platform-specific path on the
                        // sysinfo side did populate something).
                        for reading in &snapshot.thermals {
                            systeminfo
                                .component_temps
                                .entry(reading.label.clone())
                                .or_insert(reading.temp_c);
                        }
                        if !snapshot.cores.is_empty() {
                            systeminfo.cpu_cores = snapshot
                                .cores
                                .iter()
                                .map(|c| database::schema::CpuCoreLive {
                                    index: c.index,
                                    usage_pct: c.usage_pct,
                                    freq_mhz: c.freq_mhz,
                                    temp_c: c.temp_c,
                                })
                                .collect();
                        }

                        tx.send(serialize_system_info(&systeminfo))?
                    }
                    Err(e) => log::error!("Error with live data {e:?}"),
                }
            }
        }
    }
    Ok(())
}

impl<'a> TerminalApp<'a> {
    pub fn send_buffer(
        f: &mut ratatui::Frame, 
        last_sent: &mut Instant, 
        send_interval: Duration, 
        can_start: &mut bool,
        buffer_tx: tokio::sync::mpsc::UnboundedSender<(usize, Buffer)>
    ) {
        let now = Instant::now(); // Changed: Throttle buffer sending
        if now.duration_since(*last_sent) >= send_interval {
            if *can_start {
                let buffer_to_send = f.buffer_mut().clone();
                let count = f.count();
                // Broadcast to any TCP admin sessions so they receive the
                // same live ratatui rendering that the WS relay path carries.
                if let Ok(serialized) = encode_buffer_with_timestamp(count as u64, &buffer_to_send) {
                    crate::tcp_listener::broadcast_term_frame(serialized);
                }
                std::thread::scope(|s| {
                    s.spawn(|| {
                        if let Err(e) = buffer_tx.send((count, buffer_to_send)) {
                            log::warn!("Failed to send buffer: {:?}", e);
                        }
                    });
                });
                *last_sent = now;
            }
        }
    }
}

pub async fn create_client(mut client: ConnectedClient) -> anyhow::Result<ConnectedClient> {
    client.connected = true;

    // Fetch the existing row first so we can honor `customer_locked`. The
    // OA3 product-key lookup below resolves to the *original* Windows
    // license purchaser; for used machines that the shop has resold the
    // friendly_name from this lookup is wrong and admins manually re-link
    // via the admin console, which sets `customer_locked = true`. We must
    // not clobber that here on every reconnect.
    let existing_row = query_id::<ConnectedClient>(
        CONNECTED_CLIENT_TABLE.to_string(),
        client.id.clone(),
    )
    .await;
    log::info!("websockets -> query_id: {existing_row:?}");

    let existing: Option<ConnectedClient> = match &existing_row {
        Ok(opt) => opt.clone(),
        Err(_) => None,
    };
    let locked = existing.as_ref().map(|c| c.customer_locked).unwrap_or(false);

    // Carry the lock + admin-set linkage forward across the upsert so we
    // never accidentally reset them when the local client builds a
    // fresh `ConnectedClient` from scratch on startup.  Also carry the
    // cached friendly_name/customer forward whenever they're already
    // populated, so we don't refetch them from PrestaShop/Everest on
    // every reconnect — the OA3 product key is hardware-derived and
    // won't change between sessions, so a prior successful lookup is
    // authoritative until an admin clears it.
    let cached_friendly = existing
        .as_ref()
        .and_then(|c| c.friendly_name.clone())
        .filter(|s| !s.is_empty());
    if let Some(prev) = existing.as_ref() {
        client.customer_locked = prev.customer_locked;
        if locked || cached_friendly.is_some() {
            client.friendly_name = prev.friendly_name.clone();
            client.customer = prev.customer.clone();
        }
    }

    // Attempt to lookup customer by OA3 serial number (Windows only).
    // Skipped when `customer_locked` is true (admin override) or when
    // the DB row already has a cached `friendly_name` from a prior
    // successful lookup.
    #[cfg(target_os = "windows")]
    if locked {
        log::info!(
            "websockets -> create_client: customer_locked is true; \
             skipping OA-serial customer lookup"
        );
    } else if cached_friendly.is_some() {
        log::info!(
            "websockets -> create_client: friendly_name already cached in DB ({:?}); \
             skipping OA-serial customer lookup",
            cached_friendly
        );
    } else {
        use crate::filesystem::oa_serial::{get_oa_style_serial, to_oa3_13digit};
        use crate::filesystem::customer_lookup::lookup_customer_by_serial;

        match get_oa_style_serial() {
            Ok(raw_serial) => {
                log::info!("websockets -> Raw OA serial: {}", raw_serial);

                match to_oa3_13digit(&raw_serial) {
                    Ok(serial13) => {
                        log::info!("websockets -> 13-digit serial: {}", serial13);

                        match lookup_customer_by_serial(&serial13).await {
                            Ok(customer_string) => {
                                log::info!("websockets -> Customer found: {}", customer_string);
                                client.friendly_name = Some(customer_string);
                            }
                            Err(e) => {
                                log::warn!("websockets -> Customer lookup failed: {:?}", e);
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!("websockets -> Failed to convert serial to 13-digit: {:?}", e);
                    }
                }
            }
            Err(e) => {
                log::warn!("websockets -> Failed to get OA serial: {:?}", e);
            }
        }
    }

    let check_id_existence = check_id_existence(
        CONNECTED_CLIENT_TABLE.to_string(),
        client.id.clone(),
    )
    .await;

    log::info!("websockets -> check_id_existence: {check_id_existence:?}");

    // Persist the client via explicit SET clauses with per-field
    // `.bind()`.  This used to be `UPDATE $id MERGE $patch` with a
    // `serde_json::Value::Object` patch, but `serde_json::to_value(rid)`
    // encodes a `RecordId` as a generic JSON object that SurrealDB
    // rejects against typed record fields ("Couldn't coerce value for
    // field `assigned_user`… Expected `none | record<user>` but found
    // `{ key: …, table: 'user' }`").  Binding each field separately
    // preserves the type info Surreal needs to coerce a RecordId into
    // a typed `record<…>` literal.
    //
    // **UPSERT, not UPDATE.**  SurrealDB 3.x's `UPDATE $id SET …` is
    // strictly an update and silently no-ops on a missing row.  A
    // terminal-mode client connecting before the row exists would
    // otherwise be permanently invisible to the admin console.
    // UPSERT creates-or-updates so the first call lands.
    let mut sets: Vec<&'static str> = vec![
        "client_hash = $client_hash",
        "connection_string = $connection_string",
        "connected = $connected",
        "last_update = time::now()",
        "customer_locked = $customer_locked",
    ];
    let has_assigned = client.assigned_user.is_some();
    if has_assigned {
        sets.push("assigned_user = $assigned_user");
    }
    let has_computer = client.computer.is_some();
    if has_computer {
        sets.push("computer = $computer");
    }
    // Only write friendly_name / customer when this run actually
    // resolved them — never write None, which under content() was
    // wiping admin edits made via the relink popup.
    let has_name = client
        .friendly_name
        .as_ref()
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    if has_name {
        sets.push("friendly_name = $friendly_name");
    }
    let has_customer = client.customer.is_some();
    if has_customer {
        sets.push("customer = $customer");
    }

    let query = format!("UPSERT $id SET {} RETURN AFTER", sets.join(", "));
    let dbh = db();
    let mut q = dbh
        .query(&query)
        .bind(("id", client.id.clone()))
        .bind(("client_hash", client.client_hash.clone()))
        .bind(("connection_string", client.connection_string.clone()))
        .bind(("connected", client.connected))
        .bind(("customer_locked", client.customer_locked));
    if has_assigned {
        q = q.bind(("assigned_user", client.assigned_user.clone().unwrap()));
    }
    if has_computer {
        q = q.bind(("computer", client.computer.clone().unwrap()));
    }
    if has_name {
        q = q.bind(("friendly_name", client.friendly_name.clone().unwrap()));
    }
    if has_customer {
        q = q.bind(("customer", client.customer.clone().unwrap()));
    }
    let merge_res = q.await;
    match merge_res {
        Ok(_) => log::info!(
            "websockets -> create_client: partial-merge UPDATE applied for {:?}",
            client.id
        ),
        Err(e) => log::warn!(
            "websockets -> create_client: partial-merge UPDATE failed for {:?}: {e}",
            client.id
        ),
    }
    Ok(client)
}
