//! PowerShell plumbing shared by the script executors.

use displays::scripts::catalog::ScriptDef;
use displays::scripts::executor::{ScriptContext, ScriptResult};

/// Stdout of a PowerShell run, split into lines.
pub(crate) struct PsRun {
    pub lines: Vec<String>,
    pub exit_code: Option<i32>,
}

/// A PowerShell run that did not exit successfully.
pub(crate) struct PsFailure {
    /// The error as `{e}` renders it, so callers keep their existing wording.
    pub message: String,
    pub exit_code: Option<i32>,
}

/// Runs `script` hidden, non-interactive and without loading a profile.
#[cfg(target_os = "windows")]
pub(crate) fn run(script: &str) -> Result<PsRun, PsFailure> {
    use powershell_script::{PsError, PsScriptBuilder};

    let ps = PsScriptBuilder::new()
        .no_profile(true)
        .non_interactive(true)
        .hidden(true)
        .print_commands(false)
        .build();

    match ps.run(script) {
        Ok(output) => {
            let lines = output
                .stdout()
                .unwrap_or_default()
                .lines()
                .map(str::to_string)
                .collect();
            Ok(PsRun {
                lines,
                exit_code: output.into_inner().status.code(),
            })
        }
        Err(e) => {
            let exit_code = match &e {
                PsError::Powershell(output) => output.clone().into_inner().status.code(),
                _ => None,
            };
            Err(PsFailure {
                message: e.to_string(),
                exit_code,
            })
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn run(_script: &str) -> Result<PsRun, PsFailure> {
    Err(PsFailure {
        message: "PowerShell is only available on Windows".into(),
        exit_code: None,
    })
}

/// Runs `script`, logging its output as it goes, and reports `finished` on success.
pub(crate) fn logged(
    ctx: &ScriptContext,
    def: &ScriptDef,
    starting: &str,
    script: &str,
    finished: &str,
) -> (ScriptResult, Option<i32>) {
    let (category, name) = (def.category(), def.name.as_str());
    ctx.log_info(category.clone(), name, starting);

    match run(script) {
        Ok(output) => {
            for line in output.lines {
                ctx.log_info(category.clone(), name, line);
            }
            ctx.log_success(category, name, finished);
            (ScriptResult::Success(finished.into()), output.exit_code)
        }
        Err(failure) => {
            let msg = format!("Failed: {}", failure.message);
            ctx.log_error(category, name, msg.clone());
            (ScriptResult::Error(msg), failure.exit_code)
        }
    }
}
