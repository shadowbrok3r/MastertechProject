use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use std::process::Stdio;

pub struct PersistentShell {
    process: Option<tokio::process::Child>,
    stdin: Option<tokio::process::ChildStdin>,
    output_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
}

impl PersistentShell {
    pub fn new(
        output_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    ) -> Self {
        Self {
            process: None,
            stdin: None,
            output_tx,
        }
    }

    pub async fn start(&mut self) -> anyhow::Result<()> {
        let mut process = if cfg!(target_os = "windows") {
            tokio::process::Command::new("cmd")
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

        let mut stdout_reader = BufReader::new(stdout).lines();
        let mut stderr_reader = BufReader::new(stderr).lines();

        // Store the stdin for sending commands
        self.stdin = Some(stdin);
        self.process = Some(process);

        // Handle stdout
        let tx_clone = self.output_tx.clone();
        tokio::spawn(async move {
            while let Some(line) = stdout_reader.next_line().await? {
                tx_clone.send(format!("{}\n", line).into_bytes()).ok();
            }
            Ok::<(), anyhow::Error>(())
        });

        // Handle stderr
        let tx_clone = self.output_tx.clone();
        tokio::spawn(async move {
            while let Some(line) = stderr_reader.next_line().await? {
                tx_clone.send(format!("ERROR: {}\n", line).into_bytes()).ok();
            }
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    pub async fn send_command(&mut self, command: String) -> anyhow::Result<()> {
        if let Some(stdin) = &mut self.stdin {
            let command_with_newline = format!("{}\n", command);
            stdin.write_all(command_with_newline.as_bytes()).await?;
            stdin.flush().await?;
            
            // Send a marker to indicate command was sent
            self.output_tx.send(format!("COMMAND_SENT: {}\n", command).into_bytes()).ok();
        }
        Ok(())
    }

    pub async fn close(&mut self) -> anyhow::Result<()> {
        if let Some(mut process) = self.process.take() {
            if let Some(stdin) = self.stdin.take() {
                drop(stdin); // Close stdin to signal process to terminate
            }
            let _ = process.wait().await;
            self.output_tx.send("SHELL_CLOSED\nDONE".as_bytes().to_vec()).ok();
        }
        Ok(())
    }
}

pub async fn handle_command_payload(
    string_payload: String, 
    tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>
) 
    -> Result<tokio::sync::mpsc::Sender<String>, anyhow::Error>  
{ 
    // #[cfg(target_os="windows")]{ return handle_windows_cmd(string_payload, tx.clone()).await?; }
    if cfg!(target_os="windows") { 
        let _ = handle_windows_cmd(string_payload.clone(), tx.clone()).await?;
    }
    Ok(handle_linux_cmd(string_payload, tx.clone()).await?)
}

pub async fn handle_windows_cmd(
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

pub async fn handle_linux_cmd(
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