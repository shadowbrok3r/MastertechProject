use anyhow::Result;
use powershell_script::PsScriptBuilder;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct TaskbarItem {
    name: String,
    path: String,
}

fn get_taskbar_items() -> Result<Vec<TaskbarItem>> {
    let ps_script = r#"
    $taskbarPath = "$env:APPDATA\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar"
    Get-ChildItem -Path $taskbarPath -Filter "*.lnk" | ForEach-Object {
        [pscustomobject]@{
            Name = $_.Name
            Path = $_.FullName
        }
    } | ConvertTo-Json -Depth 2
    "#;

    let ps = PsScriptBuilder::new()
        .no_profile(true)
        .non_interactive(true)
        .hidden(false)
        .print_commands(true)
        .build();

    let output = ps.run(ps_script)?;

    if output.success() {
        let stdout = output.stdout().unwrap_or_default();
        let taskbar_items: Vec<TaskbarItem> = serde_json::from_str(&stdout)?;
        Ok(taskbar_items)
    } else {
        Err(anyhow::anyhow!(
            "Failed to retrieve taskbar items: {}",
            output.stderr().unwrap_or_default()
        ))
    }
}
