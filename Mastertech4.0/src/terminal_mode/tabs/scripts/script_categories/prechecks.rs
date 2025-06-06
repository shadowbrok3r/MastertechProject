use crate::{
    terminal_mode::tabs::{checklist::Category, scripts::Reporter, ScriptsTab},
    utilities::windows::{net_adapter::{check_network_adapters, connect_to_wifi, get_wlan_status, scan_wifi_networks}, registry::{align_taskbar_left, disable_notifications}}
};

impl <'a> ScriptsTab <'a> {
    pub fn handle_run_prechecks(&mut self, item_text: &str, category: &Category){
        self.current_reporter.replace(Reporter::RunPrechecks);
        self.log_message(&format!("Running precheck: {}", item_text));

        match disable_notifications() {
            Ok(results) => self.log_message(&format!("Push Notifications => {results:#?}")),
            Err(e) => self.log_message(&format!("Push Notifications => {e:?}")),
        }
        match align_taskbar_left() {
            Ok(messages) => for message in messages {
                self.log_message(&format!("TaskBarAlignment => {}", message.trim()));
            },
            Err(e) => self.log_message(&format!("TaskBarAlignment => {e:?}")),
        }

        match scan_wifi_networks() {
            Ok(networks) => {
                self.log_message(&format!("Wifi Networks: {networks:?}"));
                let connect_to_wifi = connect_to_wifi("PClaptops5.0", Some("bestburger"), None);
                self.log_message(&format!("connect_to_wifi: {connect_to_wifi:?}"));
            },
            Err(e) => self.log_message(&format!("Error Scanning Wifi Networks: {e:?}")),
        }
        match get_wlan_status() {
            Ok(_) => self.log_message("Wlan Status OK"),
            Err(e) => {
                self.log_message(&format!("Wlan Status: {e:?}"));
                self.update_checklist(category.clone(), item_text, true);
            },
        }
        match check_network_adapters() {
            Ok(adapters) => self.log_message(&format!("Network Adapters => {adapters:?}")),
            Err(e) => self.log_message(&format!("Error getting Network Adapter list => {e:?}")),
        }
    }
}