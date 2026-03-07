use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use std::process::Stdio;

pub struct PersistentShell {
    process: Option<tokio::process::Child>,
    output_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    command_queue_tx: tokio::sync::mpsc::UnboundedSender<String>,
    is_ready: std::sync::Arc<tokio::sync::Mutex<bool>>,
}

impl PersistentShell {
    pub fn new(
        output_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    ) -> Self {
        let (command_queue_tx, _) = tokio::sync::mpsc::unbounded_channel();
        Self {
            process: None,
            output_tx,
            command_queue_tx,
            is_ready: std::sync::Arc::new(tokio::sync::Mutex::new(true)),
        }
    }

    pub async fn start(&mut self) -> anyhow::Result<()> {
        let mut process = if cfg!(target_os = "windows") {
            tokio::process::Command::new("powershell")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?
        } else {
            tokio::process::Command::new("sh")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?
        };

        let stdout = process.stdout.take().expect("Failed to get stdout");
        let stderr = process.stderr.take().expect("Failed to get stderr");
        let stdin = process.stdin.take().expect("Failed to get stdin");

        let mut stdout_reader = BufReader::new(stdout);
        let mut stderr_reader = BufReader::new(stderr);

        // Store the process
        self.process = Some(process);

        // Create command queue processing
        let (command_queue_tx, mut command_queue_rx) = tokio::sync::mpsc::unbounded_channel();
        self.command_queue_tx = command_queue_tx;

        // Handle command queue processing with proper stdin handling
        let output_tx_clone = self.output_tx.clone();
        let is_ready_clone = self.is_ready.clone();
        
        tokio::spawn(async move {
            let mut stdin = stdin;
            while let Some(command) = command_queue_rx.recv().await {
                // Wait for shell to be ready
                {
                    let mut ready = is_ready_clone.lock().await;
                    while !*ready {
                        drop(ready);
                        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                        ready = is_ready_clone.lock().await;
                    }
                    *ready = false; // Mark as busy
                }
                
                // Send command to shell
                let command_with_newline = format!("{}\n", command);
                if let Err(e) = stdin.write_all(command_with_newline.as_bytes()).await {
                    log::error!("Failed to write command to stdin: {}", e);
                    break;
                } else if let Err(e) = stdin.flush().await {
                    log::error!("Failed to flush stdin: {}", e);
                    break;
                }
            }
        });

        // Handle stdout with timeout-based command completion detection
        // Instead of detecting prompts, we use a timeout: if no output for 500ms after
        // the last line, we consider the command complete and send DONE.
        let tx_clone = self.output_tx.clone();
        let is_ready_clone = self.is_ready.clone();
        tokio::spawn(async move {
            use tokio::time::{timeout, Duration};
            
            let idle_timeout = Duration::from_millis(500);
            let mut received_any_output = false;
            let mut line_buf = Vec::new();
            
            loop {
                match timeout(idle_timeout, stdout_reader.read_until(b'\n', &mut line_buf)).await {
                    Ok(Ok(n)) if n > 0 => {
                        let line = String::from_utf8_lossy(&line_buf).to_string();
                        line_buf.clear();
                        let trimmed = line.trim_end();
                        
                        let is_prompt_echo = trimmed.starts_with("PS ") && trimmed.contains(">");
                        
                        if !is_prompt_echo {
                            tx_clone.send(format!("{}\n", trimmed).into_bytes()).ok();
                        }
                        received_any_output = true;
                    }
                    Ok(Ok(_)) => {
                        break;
                    }
                    Ok(Err(e)) => {
                        log::error!("Error reading stdout: {}", e);
                        break;
                    }
                    Err(_) => {
                        if received_any_output {
                            tx_clone.send("DONE".as_bytes().to_vec()).ok();
                            received_any_output = false;
                            
                            let mut ready = is_ready_clone.lock().await;
                            *ready = true;
                        }
                    }
                }
            }
            Ok::<(), anyhow::Error>(())
        });

        // Handle stderr
        let tx_clone = self.output_tx.clone();
        tokio::spawn(async move {
            let mut line_buf = Vec::new();
            loop {
                match stderr_reader.read_until(b'\n', &mut line_buf).await {
                    Ok(n) if n > 0 => {
                        let line = String::from_utf8_lossy(&line_buf).to_string();
                        line_buf.clear();
                        tx_clone.send(format!("ERROR: {}\n", line.trim_end()).into_bytes()).ok();
                    }
                    _ => break,
                }
            }
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    pub async fn send_command(&mut self, command: String) -> anyhow::Result<()> {
        // Queue the command instead of sending immediately
        self.command_queue_tx.send(command)?;
        Ok(())
    }

    pub async fn close(&mut self) -> anyhow::Result<()> {
        if let Some(mut process) = self.process.take() {
            // Try to terminate the process gracefully
            let _ = process.kill().await;
            let _ = process.wait().await;
            self.output_tx.send("SHELL_CLOSED\nDONE".as_bytes().to_vec()).ok();
        }
        Ok(())
    }
}

pub async fn _handle_command_payload(
    string_payload: String, 
    tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>
) 
    -> Result<tokio::sync::mpsc::Sender<String>, anyhow::Error>  
{ 
    // #[cfg(target_os="windows")]{ return handle_windows_cmd(string_payload, tx.clone()).await?; }
    if cfg!(target_os="windows") { 
        let _ = _handle_windows_cmd(string_payload.clone(), tx.clone()).await?;
    }
    Ok(_handle_linux_cmd(string_payload, tx.clone()).await?)
}

pub async fn _handle_windows_cmd(
    command_payload: String, 
    tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>
) 
    -> anyhow::Result<tokio::process::ChildStdin, anyhow::Error>
{
    use tokio::{process::{Child, ChildStdin}, time::Instant};

    let start = Instant::now();
    log::info!("websockets -> Executing command: {}", command_payload);
    let mut process: Child = tokio::process::Command::new("cmd")
        .arg("/C")
        .arg(&command_payload)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    // Create a Tokio stream for stdout
    let mut stdout = process.stdout.take().expect("Failed to get stdout");
    // Create a Tokio stream for stderr
    let mut stderr = process.stderr.take().expect("Failed to get stderr");
    let stdin: ChildStdin = process.stdin.take().expect("Failed to open stdin");
    
    let tx_clone = tx.clone();
    tokio::spawn(async move {
        let mut stdout_buf = Vec::new();
        stdout.read_to_end(&mut stdout_buf).await.ok();
        tx_clone.send(stdout_buf).ok();
    });

    let tx_clone = tx.clone();
    tokio::spawn(async move {
        let mut stderr_buf = Vec::new();
        stderr.read_to_end(&mut stderr_buf).await.ok();
        tx_clone.send(stderr_buf).ok();
    });

    let output = process.wait_with_output().await?;
    log::info!("websockets -> output: {:?}", output);
    let duration = start.elapsed();
    log::info!("websockets -> Command executed in {:?}", duration);
    let tx_clone = tx.clone();
    if !output.status.success() {
        log::info!("websockets -> output status not successfull");
        tx_clone.send(output.stderr).ok();
    }
    
    // Send DONE marker to indicate command completion
    tx.send("DONE".as_bytes().to_vec()).ok();

    Ok(stdin)
}

pub async fn handle_windows_cmd_interactive(
    command_payload: String, 
    tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<String>
) 
    ->  anyhow::Result<(), anyhow::Error> 
{
    let mut process: tokio::process::Child = tokio::process::Command::new("cmd")
        .arg("/C")
        .arg(&command_payload)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    // Create a Tokio stream for stdout / stderr
    let stdout = process.stdout.take().expect("Failed to get stdout");
    let stderr = process.stderr.take().expect("Failed to get stderr");
    let mut stdin: tokio::process::ChildStdin = process.stdin.take().expect("Failed to open stdin");

    let mut stdout_reader = BufReader::new(stdout).lines();
    let mut stderr_reader = BufReader::new(stderr).lines();

    // Ensure the child process is spawned in the runtime so it can
    // make progress on its own while we await for any output.
    let tx_done = tx.clone();
    tokio::spawn(async move {
        let status = process.wait().await.expect("child process encountered an error");
        log::info!("websockets -> child status was: {}", status);
        // Send DONE marker when process completes
        tx_done.send("DONE".as_bytes().to_vec()).ok();
    });

    let tx_clone = tx.clone();
    tokio::spawn(async move {
        while let Some(line) = stderr_reader.next_line().await? {
            tx_clone.send(format!("{}\n", line).into_bytes()).ok();
        }
        Ok::<(), anyhow::Error>(())
    });

    let tx_clone = tx.clone();
    tokio::spawn(async move {
        while let Some(line) = stdout_reader.next_line().await? {
            tx_clone.send(format!("{}\n", line).into_bytes()).ok();
        }
        Ok::<(), anyhow::Error>(())
    });
    
    tokio::spawn(async move {
        while let Some(input) = rx.recv().await {
            if input != "quit".to_string() {
                // Add newline to input for command execution
                let input_with_newline = format!("{}\n", input);
                if let Err(e) = stdin.write_all(input_with_newline.as_bytes()).await {
                    log::info!("websockets -> Failed to write to stdin: {}", e);
                    break;
                }
                if let Err(e) = stdin.flush().await {
                    log::info!("websockets -> Failed to flush stdin: {}", e);
                    break;
                }
            } else { 
                // Send quit command and break
                let _ = stdin.write_all("exit\n".as_bytes()).await;
                let _ = stdin.flush().await;
                break; 
            }
        }
    });

    Ok(())
}

pub async fn _handle_linux_cmd(
    command_payload: String, 
    tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>
) 
    -> anyhow::Result<tokio::sync::mpsc::Sender<String>, anyhow::Error> 
{
    let mut process: tokio::process::Child = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(&command_payload)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = process.stdout.take().expect("Failed to get stdout");
    let stderr = process.stderr.take().expect("Failed to get stderr");
    let stdin: tokio::process::ChildStdin = process.stdin.take().expect("Failed to open stdin");

    let mut stdout_reader = BufReader::new(stdout).lines();
    let mut stderr_reader = BufReader::new(stderr).lines();
    // Use a channel to allow sending input to the stdin of the process
    let (input_tx, mut input_rx) = tokio::sync::mpsc::channel::<String>(32);

    let tx_clone = tx.clone();
    // let mut idle_timeout = sleep(Duration::from_secs(10));
    // pin!(idle_timeout);
    let t = input_tx.clone();
    tokio::spawn(async move {
        // Process both stdout and stderr
        loop {
            let in_tx = input_tx.clone();
            tokio::select! {
                stdout_line = stdout_reader.next_line() => {
                    if let Ok(Some(line)) = stdout_line {
                        if line.contains("Enter") || line.contains("Password:") {
                            // Command is asking for input; handle interactively
                            log::info!("Detected interactive prompt: {}", line);
                            in_tx.send("YourInputHere\n".to_string()).await?;
                        } else {
                            tx_clone.send(line.into_bytes())?;
                        }
                    }
                }
                stderr_line = stderr_reader.next_line() => {
                    if let Ok(Some(line)) = stderr_line {
                        if line.contains("Enter") || line.contains("Password:") {
                            log::info!("Detected interactive prompt: {}", line);
                            input_tx.send("YourInputHere\n".to_string()).await?;
                        } else {
                            tx_clone.send(line.into_bytes())?;
                        }
                    }
                }
                // _ = &mut idle_timeout => {
                //     // No output within the timeout duration
                //     input_tx.send("DefaultInput\n".to_string()).await?;
                // }
            }
        }
        #[allow(unreachable_code)]
        Ok::<(), anyhow::Error>(())
    });

    // Spawn a task to handle stdin input using `input_rx`
    tokio::spawn(async move {
        let mut stdin = stdin; // Move `stdin` into this task
        while let Some(input) = input_rx.recv().await {
            if let Err(e) = stdin.write_all(input.as_bytes()).await {
                log::error!("Failed to write to stdin: {:?}", e);
                break;
            }
            // Ensure each input is flushed after writing
            if let Err(e) = stdin.flush().await {
                log::error!("Failed to flush stdin: {:?}", e);
                break;
            }
        }
    });
    
    let tx_clone = tx.clone();
    tokio::spawn(async move {
        // Wait for the child process to complete
        if let Ok(status) = process.wait().await {
            log::info!("Process exited with status: {status}");
            tx_clone.send("DONE".to_string().into_bytes())?;
        }
        Ok::<(), anyhow::Error>(())
    });

    Ok(t)
}