//! Shell job executor.
//!
//! Two correctness rules the existing `:9001` `run_shell_command` broke:
//!   1. The exit status is captured and reported. A job that fails must say so.
//!   2. stdout and stderr are drained concurrently with the wait. Waiting on a
//!      process while its pipes fill deadlocks once the buffer is full.

use std::process::Stdio;
use std::time::Instant;

use displays::remote_exec::{JobExit, JobOutcome, JobState, JobStream, RemoteJobSpec, ShellKind};
use tokio::io::{AsyncReadExt, BufReader};

use super::job::JobHandle;
use super::journal;

/// Applied when the caller does not set `timeout_secs`.
pub const DEFAULT_TIMEOUT_SECS: u64 = 3600;

/// Upper bound regardless of what the caller asks for.
const MAX_TIMEOUT_SECS: u64 = 86_400;

/// No output for this long is the wedge signal — a job still producing output
/// has not hung, however long it has run.
const IDLE_TIMEOUT_SECS: u64 = 600;

/// Read granularity from each pipe.
const READ_CHUNK: usize = 8192;

fn script_extension(shell: ShellKind) -> &'static str {
    match shell {
        ShellKind::Cmd => "bat",
        _ => "ps1",
    }
}

/// Writes the script to a temp file. Passing a script as a command-line
/// argument mangles quoting; a file does not.
fn stage_script(job_id: &str, shell: ShellKind, script: &str) -> std::io::Result<std::path::PathBuf> {
    let mut path = std::env::temp_dir();
    path.push(format!("mtech_remote_exec_{job_id}.{}", script_extension(shell)));
    std::fs::write(&path, script)?;
    Ok(path)
}

fn build_command(
    shell: ShellKind,
    script_path: &std::path::Path,
    cwd: Option<&str>,
    env: &[(String, String)],
) -> tokio::process::Command {
    let mut cmd = match shell {
        ShellKind::Cmd => {
            let mut c = tokio::process::Command::new("cmd.exe");
            c.arg("/C").arg(script_path);
            c
        }
        ShellKind::Pwsh | ShellKind::PowerShell => {
            let exe = if matches!(shell, ShellKind::Pwsh) { "pwsh.exe" } else { "powershell.exe" };
            let mut c = tokio::process::Command::new(exe);
            c.arg("-NoProfile")
                .arg("-NonInteractive")
                .arg("-ExecutionPolicy")
                .arg("Bypass")
                .arg("-File")
                .arg(script_path);
            c
        }
    };
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // CREATE_NO_WINDOW: never flash a console on a customer's desktop.
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    cmd
}

/// Runs the job to completion, recording output and a real exit status.
pub async fn run(handle: JobHandle, spec: RemoteJobSpec) {
    let started = Instant::now();
    let RemoteJobSpec::Shell {
        shell,
        script,
        cwd,
        env,
        timeout_secs,
        redact,
    } = spec;

    let timeout = timeout_secs
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
        .min(MAX_TIMEOUT_SECS);

    let script_path = match stage_script(&handle.job_id, shell, &script) {
        Ok(p) => p,
        Err(e) => {
            finish_spawn_failure(&handle, started, format!("cannot stage script: {e}"));
            return;
        }
    };

    let mut cmd = build_command(shell, &script_path, cwd.as_deref(), &env);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let _ = std::fs::remove_file(&script_path);
            finish_spawn_failure(&handle, started, format!("spawn failed: {e}"));
            return;
        }
    };

    let pid = child.id();
    handle.set_pid(pid);
    handle.set_state(JobState::Running);

    #[cfg(windows)]
    if let Some(raw) = child.raw_handle() {
        handle.tree.assign(raw as isize);
    }

    let mut stdout = child.stdout.take().map(BufReader::new);
    let mut stderr = child.stderr.take().map(BufReader::new);
    let mut out_buf = vec![0u8; READ_CHUNK];
    let mut err_buf = vec![0u8; READ_CHUNK];
    let mut last_output = Instant::now();

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(timeout);
    let mut outcome: Option<JobOutcome> = None;
    let mut exit_code: Option<i32> = None;

    loop {
        if handle.is_cancelled() && outcome.is_none() {
            handle.tree.terminate();
            let _ = child.start_kill();
            outcome = Some(JobOutcome::Cancelled);
        }
        if tokio::time::Instant::now() >= deadline && outcome.is_none() {
            handle.push_output(
                JobStream::Meta,
                format!("[remote_exec] wall-clock timeout after {timeout}s; killing tree\n")
                    .into_bytes(),
            );
            handle.tree.terminate();
            let _ = child.start_kill();
            outcome = Some(JobOutcome::TimedOut);
        }
        if last_output.elapsed().as_secs() >= IDLE_TIMEOUT_SECS && outcome.is_none() {
            handle.push_output(
                JobStream::Meta,
                format!(
                    "[remote_exec] no output for {IDLE_TIMEOUT_SECS}s; treating as wedged and \
                     killing tree\n"
                )
                .into_bytes(),
            );
            handle.tree.terminate();
            let _ = child.start_kill();
            outcome = Some(JobOutcome::TimedOut);
        }

        tokio::select! {
            biased;

            n = async {
                match stdout.as_mut() {
                    Some(r) => r.read(&mut out_buf).await,
                    // Keep this branch pending once the pipe is done.
                    None => std::future::pending().await,
                }
            } => {
                match n {
                    Ok(0) => stdout = None,
                    Ok(n) => {
                        last_output = Instant::now();
                        if !redact {
                            handle.push_output(JobStream::Stdout, out_buf[..n].to_vec());
                        } else {
                            handle.push_output(JobStream::Stdout, Vec::new());
                        }
                    }
                    Err(_) => stdout = None,
                }
            }

            n = async {
                match stderr.as_mut() {
                    Some(r) => r.read(&mut err_buf).await,
                    None => std::future::pending().await,
                }
            } => {
                match n {
                    Ok(0) => stderr = None,
                    Ok(n) => {
                        last_output = Instant::now();
                        if !redact {
                            handle.push_output(JobStream::Stderr, err_buf[..n].to_vec());
                        } else {
                            handle.push_output(JobStream::Stderr, Vec::new());
                        }
                    }
                    Err(_) => stderr = None,
                }
            }

            status = child.wait() => {
                match status {
                    Ok(st) => {
                        exit_code = st.code();
                        if outcome.is_none() {
                            outcome = Some(JobOutcome::Exited);
                        }
                    }
                    Err(e) => {
                        outcome = Some(JobOutcome::SpawnFailed(format!("wait failed: {e}")));
                    }
                }
                break;
            }

            _ = tokio::time::sleep(std::time::Duration::from_millis(250)) => {}
        }
    }

    // Drain whatever the pipes still hold; the child has exited so this ends.
    if !redact {
        if let Some(r) = stdout.as_mut() {
            let mut rest = Vec::new();
            if r.read_to_end(&mut rest).await.is_ok() && !rest.is_empty() {
                handle.push_output(JobStream::Stdout, rest);
            }
        }
        if let Some(r) = stderr.as_mut() {
            let mut rest = Vec::new();
            if r.read_to_end(&mut rest).await.is_ok() && !rest.is_empty() {
                handle.push_output(JobStream::Stderr, rest);
            }
        }
    }

    let _ = std::fs::remove_file(&script_path);

    let outcome = outcome.unwrap_or(JobOutcome::Exited);
    let state = match (&outcome, exit_code) {
        (JobOutcome::Exited, Some(0)) => JobState::Succeeded,
        (JobOutcome::Exited, _) => JobState::Failed,
        (JobOutcome::Cancelled, _) => JobState::Cancelled,
        (JobOutcome::Killed, _) => JobState::Cancelled,
        (JobOutcome::TimedOut, _) => JobState::TimedOut,
        (JobOutcome::SpawnFailed(_), _) | (JobOutcome::GateDenied(_), _) => JobState::Failed,
    };

    let (stdout_bytes, stderr_bytes, truncated_bytes, last_seq) = handle
        .inner
        .lock()
        .map(|i| {
            (
                i.ring.stdout_bytes,
                i.ring.stderr_bytes,
                i.ring.evicted_bytes(),
                i.ring.last_seq(),
            )
        })
        .unwrap_or((0, 0, 0, 0));

    let exit = JobExit {
        job_id: handle.job_id.clone(),
        outcome: outcome.clone(),
        exit_code,
        duration_ms: started.elapsed().as_millis() as u64,
        stdout_bytes,
        stderr_bytes,
        truncated_bytes,
        last_seq,
    };

    journal::record_exit(
        &handle.job_id,
        &format!("{outcome:?}"),
        exit_code,
        exit.duration_ms,
        stdout_bytes,
        stderr_bytes,
        truncated_bytes,
    );
    log::info!(
        "[remote_exec] job {} finished: {:?} exit={:?} in {}ms",
        handle.job_id,
        outcome,
        exit_code,
        exit.duration_ms
    );
    handle.finish(exit, state);
}

fn finish_spawn_failure(handle: &JobHandle, started: Instant, msg: String) {
    log::warn!("[remote_exec] job {}: {msg}", handle.job_id);
    handle.push_output(JobStream::Meta, msg.clone().into_bytes());
    let exit = JobExit {
        job_id: handle.job_id.clone(),
        outcome: JobOutcome::SpawnFailed(msg.clone()),
        exit_code: None,
        duration_ms: started.elapsed().as_millis() as u64,
        stdout_bytes: 0,
        stderr_bytes: 0,
        truncated_bytes: 0,
        last_seq: 0,
    };
    journal::record_exit(&handle.job_id, "SpawnFailed", None, exit.duration_ms, 0, 0, 0);
    handle.finish(exit, JobState::Failed);
}
