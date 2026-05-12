//! Warehouse fleet dashboard tab.
//!
//! Renders a read-only summary of the QC fleet: which machines are online,
//! their last heartbeat, CPU load, and last report time.  Data is sourced
//! from the orchestrator REST API via a background poller.

use eframe::egui::{Color32, Grid, Label, RichText, Ui};
use crate::app_state::SharedContext;

impl SharedContext {
    /// Render the "Fleet Dashboard" tab for warehouse employees.
    ///
    /// This is a minimal read-only view: agent list + last known status.
    /// The admin console provides full per-machine control.
    pub fn fleet_dashboard(&mut self, ui: &mut Ui) {
        ui.heading("Fleet Dashboard");
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Live QC machine status.  Use the Admin Console tab to connect to a specific machine.",
            )
            .weak(),
        );
        ui.separator();

        // Poll the fleet list from the orchestrator if a URL is configured.
        // For now we display a helpful placeholder until the polling layer is wired in.
        //
        // TODO: add a `fleet_agents: Arc<Mutex<Vec<FleetAgentSummary>>>` field to
        // `SharedContext` and spawn a background task that calls
        // `GET /api/v1/qc/agents` every 15 seconds, then render the results here.
        ui.add_space(8.0);

        if let Some(ref agents) = self.fleet_agents {
            if agents.is_empty() {
                ui.colored_label(Color32::YELLOW, "No agents registered with the orchestrator yet.");
                return;
            }
            Grid::new("fleet_dashboard_grid")
                .num_columns(5)
                .striped(true)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    // Header
                    ui.label(RichText::new("Machine").strong());
                    ui.label(RichText::new("Version").strong());
                    ui.label(RichText::new("CPU %").strong());
                    ui.label(RichText::new("Last Heartbeat").strong());
                    ui.label(RichText::new("Last Report").strong());
                    ui.end_row();

                    for agent in agents {
                        let cpu_color = if agent.cpu_avg_pct > 90.0 {
                            Color32::LIGHT_RED
                        } else if agent.cpu_avg_pct > 60.0 {
                            Color32::YELLOW
                        } else {
                            Color32::LIGHT_GREEN
                        };

                        ui.add(Label::new(&agent.machine_id));
                        ui.add(Label::new(&agent.agent_version));
                        ui.colored_label(cpu_color, format!("{:.1}%", agent.cpu_avg_pct));
                        ui.add(Label::new(
                            RichText::new(&agent.last_heartbeat).monospace().small(),
                        ));
                        ui.add(Label::new(
                            RichText::new(
                                agent.last_report_at.as_deref().unwrap_or("—"),
                            )
                            .monospace()
                            .small(),
                        ));
                        ui.end_row();
                    }
                });
        } else {
            ui.colored_label(Color32::GRAY, "Configure the orchestrator URL in Settings to see live fleet data.");
            ui.add_space(8.0);
            ui.label("Once configured, this panel will list every QC machine, its CPU load, and last report time.");
        }
    }
}
