use crate::{app_state::MastertechContext, filesystem::system_info::get_sysinfo};
use displays::tabs::resource_monitor::ResourceMonitorState;
use database::schema::SystemInformation;
use crossbeam::channel::Sender;
use eframe::egui::Ui;
use tokio::spawn;

impl MastertechContext {
    pub fn show_resource_monitor(&mut self, ui: &mut Ui) {
        self.shared_ctx.resource_mon.display(ui);
        let resource_monitor = &mut self.shared_ctx.resource_mon;
        if let ResourceMonitorState::RequestingData = resource_monitor.state {
            resource_monitor.state = ResourceMonitorState::default();
            let tx = resource_monitor.sysinfo_channel.0.clone();
            spawn(async move {
                let res = live_computer_stats(tx).await; 
                log::info!("Getting live sys stats: {res:?}");
            });
        }
    }
}

async fn live_computer_stats(tx: Sender<SystemInformation>) -> anyhow::Result<(), anyhow::Error>{
    loop {
        tx.send(get_sysinfo().await?)?;
        tokio::time::sleep(std::time::Duration::from_secs_f32(0.1)).await;
    }
    #[allow(unreachable_code)]
    Ok(())
}
