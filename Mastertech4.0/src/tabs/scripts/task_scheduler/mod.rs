#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct ScheduledTask {
    #[serde(rename = "CimClass")]
    pub cim_class: CimClass,
    #[serde(rename = "CimInstanceProperties")]
    pub cim_instance_properties: Vec<String>,
    #[serde(rename = "CimSystemProperties")]
    pub cim_system_properties: CimSystemProperties,
    #[serde(rename = "State")]
    pub state: Option<u8>,
    #[serde(rename = "Actions")]
    pub actions: Option<Vec<String>>,
    #[serde(rename = "Author")]
    pub author: Option<String>,
    #[serde(rename = "Date")]
    pub date: Option<String>,
    #[serde(rename = "Description")]
    pub description: Option<String>,
    #[serde(rename = "Documentation")]
    pub documentation: Option<String>,
    #[serde(rename = "Principal")]
    pub principal: Option<Principal>,
    #[serde(rename = "SecurityDescriptor")]
    pub security_descriptor: Option<String>,
    #[serde(rename = "Settings")]
    pub settings: Option<Settings>,
    #[serde(rename = "Source")]
    pub source: Option<String>,
    #[serde(rename = "TaskName")]
    pub task_name: Option<String>,
    #[serde(rename = "TaskPath")]
    pub task_path: Option<String>,
    #[serde(rename = "Triggers")]
    pub triggers: Option<Vec<String>>,
    #[serde(rename = "URI")]
    pub uri: Option<String>,
    #[serde(rename = "Version")]
    pub version: Option<String>,
    #[serde(rename = "PSComputerName")]
    pub ps_computer_name: Option<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct CimClass {
    #[serde(rename = "CimSuperClassName")]
    pub cim_super_class_name: Option<String>,
    #[serde(rename = "CimSuperClass")]
    pub cim_super_class: Option<String>,
    #[serde(rename = "CimClassProperties")]
    pub cim_class_properties: Option<String>,
    #[serde(rename = "CimClassQualifiers")]
    pub cim_class_qualifiers: Option<String>,
    #[serde(rename = "CimClassMethods")]
    pub cim_class_methods: Option<String>,
    #[serde(rename = "CimSystemProperties")]
    pub cim_system_properties: Option<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct CimSystemProperties {
    #[serde(rename = "Namespace")]
    pub namespace: Option<String>,
    #[serde(rename = "ServerName")]
    pub server_name: Option<String>,
    #[serde(rename = "ClassName")]
    pub class_name: Option<String>,
    #[serde(rename = "Path")]
    pub path: Option<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct Principal {
    #[serde(rename = "CimClass")]
    pub cim_class: Option<String>,
    #[serde(rename = "CimInstanceProperties")]
    pub cim_instance_properties: Option<String>,
    #[serde(rename = "CimSystemProperties")]
    pub cim_system_properties: Option<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct Settings {
    #[serde(rename = "CimClass")]
    pub cim_class: Option<String>,
    #[serde(rename = "CimInstanceProperties")]
    pub cim_instance_properties: Option<String>,
    #[serde(rename = "CimSystemProperties")]
    pub cim_system_properties: Option<String>,
}

/// Enum for specifying possible task triggers
pub enum TaskTrigger {
    Daily { time: String },
    Weekly { days_of_week: String, time: String },
    Once { date_time: String },
    AtLogon,
    AtStartup,
}

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

        let ps = powershell_script::PsScriptBuilder::new()
            .no_profile(true)
            .non_interactive(true)
            .hidden(true)
            .print_commands(false)
            .build();

        ps.run(&script)?;
        Ok(())
    }
}

/// Functions for listing, modifying, and deleting tasks
impl ScheduledTask {
    /// Lists all scheduled tasks
    pub fn list_tasks() -> Result<Vec<ScheduledTask>, anyhow::Error> {
        let ps = powershell_script::PsScriptBuilder::new()
            .no_profile(true)
            .non_interactive(true)
            .hidden(true)
            .print_commands(false)
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

        let ps = powershell_script::PsScriptBuilder::new()
            .no_profile(true)
            .non_interactive(true)
            .hidden(true)
            .print_commands(false)
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

        let ps = powershell_script::PsScriptBuilder::new()
            .no_profile(true)
            .non_interactive(true)
            .hidden(true)
            .print_commands(false)
            .build();

        ps.run(&script)?;
        Ok(())
    }
}

impl std::fmt::Display for TaskTrigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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
