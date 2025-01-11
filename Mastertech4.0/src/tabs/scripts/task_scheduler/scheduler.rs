use std::fmt;

use powershell_script::PsScriptBuilder;

use super::{ScheduledTask, TaskTrigger};

impl ScheduledTaskBuilder {
    /// Creates a new ScheduledTaskBuilder with the task name
    pub fn new(task_name: &str) -> Self {
        Self {
            task_name: task_name.to_string(),
            action: None,
            trigger: None,
            description: None,
        }
    }

    /// Sets the executable action for the task
    pub fn action(mut self, executable: &str) -> Self {
        self.action = Some(executable.to_string());
        self
    }

    /// Sets the trigger for the task
    pub fn trigger(mut self, trigger: TaskTrigger) -> Self {
        self.trigger = Some(trigger);
        self
    }

    /// Sets a description for the task
    pub fn description(mut self, description: &str) -> Self {
        self.description = Some(description.to_string());
        self
    }

    /// Creates or registers the task using PowerShell
    pub async fn register(self) -> Result<(), anyhow::Error> {
        let action = self
            .action
            .ok_or_else(|| anyhow::anyhow!("Action must be specified before registering the task"))?;
        let trigger = self
            .trigger
            .ok_or_else(|| anyhow::anyhow!("Trigger must be specified before registering the task"))?;

        let script = format!(
            r#"
            $action = New-ScheduledTaskAction -Execute "{action}";
            $trigger = New-ScheduledTaskTrigger {trigger};
            Register-ScheduledTask -TaskName "{task_name}" -Action $action -Trigger $trigger -Description "{description}";
            "#,
            action = action,
            trigger = trigger,
            task_name = self.task_name,
            description = self.description.unwrap_or_default(),
        );

        let ps = PsScriptBuilder::new()
            .no_profile(true)
            .non_interactive(true)
            .hidden(false)
            .print_commands(true)
            .build();

        ps.run(&script)?;
        Ok(())
    }
}

/// Functions for listing, modifying, and deleting tasks
impl ScheduledTask {
    /// Lists all scheduled tasks
    pub async fn list_tasks() -> Result<Vec<ScheduledTask>, anyhow::Error> {
        let ps = PsScriptBuilder::new()
            .no_profile(true)
            .non_interactive(true)
            .hidden(false)
            .print_commands(true)
            .build();

        let output = ps.run(r#"Get-ScheduledTask | ConvertTo-Json"#)?;
        let tasks: Vec<ScheduledTask> = serde_json::from_str(&output.stdout().unwrap_or_default())?;
        Ok(tasks)
    }

    /// Deletes a scheduled task by name
    pub async fn delete_task(task_name: &str) -> Result<(), anyhow::Error> {
        let script = format!(
            r#"
            Unregister-ScheduledTask -TaskName "{task_name}" -Confirm:$false
            "#,
            task_name = task_name
        );

        let ps = PsScriptBuilder::new()
            .no_profile(true)
            .non_interactive(true)
            .hidden(false)
            .print_commands(true)
            .build();

        ps.run(&script)?;
        Ok(())
    }

    /// Modifies a task trigger
    pub async fn modify_trigger(task_name: &str, new_trigger: TaskTrigger) -> Result<(), anyhow::Error> {
        let script = format!(
            r#"
            $trigger = New-ScheduledTaskTrigger {trigger};
            Set-ScheduledTask -TaskName "{task_name}" -Trigger $trigger;
            "#,
            trigger = new_trigger,
            task_name = task_name
        );

        let ps = PsScriptBuilder::new()
            .no_profile(true)
            .non_interactive(true)
            .hidden(false)
            .print_commands(true)
            .build();

        ps.run(&script)?;
        Ok(())
    }
}

impl fmt::Display for TaskTrigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaskTrigger::Daily { time } => write!(f, "-Daily -At {}", time),
            TaskTrigger::Weekly { days_of_week, time } => {
                write!(f, "-Weekly -DaysOfWeek {} -At {}", days_of_week, time)
            }
            TaskTrigger::Once { date_time } => write!(f, "-Once -At {}", date_time),
            TaskTrigger::AtLogon => write!(f, "-AtLogon"),
            TaskTrigger::AtStartup => write!(f, "-AtStartup"),
        }
    }
}

/// Builder pattern for creating or modifying a scheduled task
pub struct ScheduledTaskBuilder {
    task_name: String,
    action: Option<String>,
    trigger: Option<TaskTrigger>,
    description: Option<String>,
}
