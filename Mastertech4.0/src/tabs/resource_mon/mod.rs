use crate::app_state::MastertechContext;
use displays::tabs::resource_monitor::{MachineDriveRow, MachineInfo, ResourceMonitorState};
use eframe::egui::Ui;
use serde_json::Value;
use tokio::spawn;

impl MastertechContext {
    pub fn show_resource_monitor(&mut self, ui: &mut Ui) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use crate::filesystem::system_info::current_telemetry_snapshot;
            self.shared_ctx
                .resource_mon
                .set_telemetry(current_telemetry_snapshot());
        }
        self.shared_ctx
            .resource_mon
            .set_machine_info(machine_info_from_context(self));

        self.shared_ctx.resource_mon.display(ui);

        let resource_monitor = &mut self.shared_ctx.resource_mon;
        if matches!(resource_monitor.state, ResourceMonitorState::RequestingData) {
            resource_monitor.state = ResourceMonitorState::AllCharts;
            let tx = resource_monitor.sysinfo_channel.0.clone();
            spawn(async move {
                let res = crate::filesystem::system_info::live_computer_stats(tx).await;
                log::info!("Getting live sys stats: {res:?}");
            });
        }
    }
}

fn machine_info_from_context(ctx: &MastertechContext) -> MachineInfo {
    let mut drives = Vec::new();
    for disk_index in 0..ctx.disk_num {
        if let Some(disk) = ctx.disks.get(disk_index) {
            let disk_letter = disk
                .get("drive_letter")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let drive_type = disk
                .get("drive_type")
                .and_then(Value::as_str)
                .unwrap_or("");
            let drive_type = if drive_type.starts_with("Unknown") {
                "Network Drive?".to_string()
            } else {
                drive_type.to_string()
            };
            let space_label = format!(
                "{} Gb / {} Gb",
                disk.get("space_left").and_then(Value::as_str).unwrap_or(""),
                disk.get("total_size").and_then(Value::as_str).unwrap_or("")
            );
            drives.push(MachineDriveRow {
                index: disk_index,
                letter: disk_letter,
                drive_type,
                space_label,
            });
        }
    }

    MachineInfo {
        hostname: ctx.computer_data.hostname.clone(),
        cpu: ctx.computer_data.cpu.clone(),
        ram_gb: format!("{} Gb", ctx.computer_data.ram),
        gpu: ctx.computer_data.gpu.clone(),
        drives,
    }
}
