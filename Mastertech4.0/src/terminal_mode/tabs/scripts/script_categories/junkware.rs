use crate::{terminal_mode::tabs::{checklist::Category, scripts::Reporter, ScriptsTab}, utilities::scripts::InstalledProgram};

impl <'a> ScriptsTab <'a> {
    pub fn handle_junkware_removal(&mut self, item_text: &str, category: &Category){
        self.current_reporter.replace(Reporter::JunkwareRemoval);
        self.log_message(&format!("Removing junkware: {}", item_text));
        match item_text {
            "OneLaunch" => self.remove_onelaunch(),
            "WebNavigator Browser" => self.remove_webnavigator(),
            "Wave Browser" => self.remove_wavesor(),
            "Clear Browser" => self.remove_clearbrowser(),
            "Shift Browser" => self.remove_shiftbrowser(),
            "Avast Browser" => self.remove_avastbrowser(),
            "Mcaffee Safe" => self.remove_mcaffeesafe(),
            "Driver Support" => self.remove_driversupport(),
            "Winzip" => self.remove_winzip(),
            "Run Junkware Category" => {
            //     self.remove_junkware(Some("Webroot TEST"));
            //     self.remove_junkware(Some("SuperAnti TEST"));
                self.remove_junkware(Some("OneLaunch"));
                self.remove_junkware(Some("WebNavigator Browser"));
                self.remove_junkware(Some("ESET Security"));
                self.remove_junkware(Some("Wave Browser"));
                self.remove_junkware(Some("Clear Browser"));
                self.remove_junkware(Some("Shift Browser"));
                self.remove_junkware(Some("Avast Browser"));
                self.remove_junkware(Some("Mcaffee Safe Search"));
                self.remove_junkware(Some("Driver Support"));
                self.remove_junkware(Some("Winzip"));
                
            }
            _ => {
                self.log_message(&format!("Unknown Junkware script: {}: {:?}", item_text, category));
            }
        }
    }

    pub fn remove_junkware(&mut self, item_text: Option<&str>) {
        if let Ok(programs) = InstalledProgram::get_installed_programs().as_mut() {
            for program in &mut *programs {
                if let Some(publisher) = &program.publisher {
                    let publisher = publisher.to_lowercase();
                    if let Some(txt) = item_text {
                        // if (txt.eq("") && publisher.contains("onelaunch"))
                        //     || (txt.eq("") && publisher.contains("webnavigator"))
                        //     || (txt.eq("") && publisher.contains("eset"))
                        //     || (txt.eq("") && publisher.contains("wavesor software"))
                        //     || (txt.eq("") && publisher.contains("clear browser"))
                        //     || (txt.eq("") && publisher.contains("shift technologies"))
                        //     || (txt.eq("") && publisher.contains("Avast Browser"))
                        //     || (txt.eq("") && publisher.contains("Mcaffee Safe"))
                        //     || (txt.eq("") && publisher.contains("driver support"))
                        //     || (txt.eq("") && publisher.contains("winzip"))
                        // {

                        // }
                        match txt {
                            "OneLaunch" if publisher.contains("onelaunch") => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled OneLaunch"),
                                Err(e) => self.log_message(&format!("Error uninstalling OneLaunch: {e:?}")),
                            }
                            "WebNavigator Browser" if publisher.contains("webnavigator") => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Web Navigator Browser"),
                                Err(e) => self.log_message(&format!("Error uninstalling Web Navigator Browser: {e:?}")),
                            }
                            "ESET Security" if publisher.contains("eset") => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled ESET"),
                                Err(e) => self.log_message(&format!("Error uninstalling ESET: {e:?}")),
                            }
                            "Wave Browser" if publisher.contains("wavesor software") => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Wave Browser"),
                                Err(e) => self.log_message(&format!("Error uninstalling Wave Browser: {e:?}")),
                            }
                            "Clear Browser" if publisher.contains("clear browser") => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Clear Browser"),
                                Err(e) => self.log_message(&format!("Error uninstalling Clear Browser: {e:?}")),
                            }
                            "Shift Browser" if publisher.contains("shift technologies") => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Shift Browser"),
                                Err(e) => self.log_message(&format!("Error uninstalling Shift Browser: {e:?}")),
                            }
                            "Avast Browser" if publisher.contains("Avast Browser") => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Avast Browser"),
                                Err(e) => self.log_message(&format!("Error uninstalling Avast Browser: {e:?}")),
                            }
                            "Mcaffee Safe Search" if publisher.contains("Mcaffee Safe") => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Mcaffee Safe Search"),
                                Err(e) => self.log_message(&format!("Error uninstalling Mcaffee Safe Search: {e:?}")),
                            }
                            "Driver Support" if publisher.contains("driver support") => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Driver Support"),
                                Err(e) => self.log_message(&format!("Error uninstalling Driver Support: {e:?}")),
                            }
                            "Winzip" if publisher.contains("winzip") => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Winzip"),
                                Err(e) => self.log_message(&format!("Error uninstalling Winzip: {e:?}")),
                            }
                            // "Webroot TEST" | "SuperAnti TEST" => {
                            //     for program in self.installed_programs.iter() {
                            //         let display_name = program.display_name.clone().unwrap_or_default().to_lowercase();
                            //         let publisher = program.publisher.clone().unwrap_or_default().to_lowercase();
                            //         if display_name.contains("webroot")
                            //             || display_name.contains("wrsa")
                            //             || publisher.contains("webroot")
                            //             || publisher.contains("wrsa")
                            //             || display_name.contains("superantispyware")
                            //             || publisher.contains("superantispyware")
                            //         {
                            //             self.log_message(&format!("Webroot or SAS found. attempting uninstall: {display_name:?}"));
                            //             program.uninstall().unwrap();
                            //         }
                            //     }
                            // }
                            _ => {}
                        }
                    } else {
                        //ccleaner browser, SAS browser extension
                        match publisher.as_str() {
                            "OneLaunch" => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled OneLaunch"),
                                Err(e) => self.log_message(&format!("Error uninstalling OneLaunch: {e:?}")),
                            }
                            "WebNavigator Browser" => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Web Navigator Browser"),
                                Err(e) => self.log_message(&format!("Error uninstalling Web Navigator Browser: {e:?}")),
                            }
                            "ESET Security" => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled ESET"),
                                Err(e) => self.log_message(&format!("Error uninstalling ESET: {e:?}")),
                            }
                            "Wave Browser" => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Wave Browser"),
                                Err(e) => self.log_message(&format!("Error uninstalling Wave Browser: {e:?}")),
                            }
                            "Clear Browser" => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Clear Browser"),
                                Err(e) => self.log_message(&format!("Error uninstalling Clear Browser: {e:?}")),
                            }
                            "Shift Browser" => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Shift Browser"),
                                Err(e) => self.log_message(&format!("Error uninstalling Shift Browser: {e:?}")),
                            }
                            "Avast Browser" => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Avast Browser"),
                                Err(e) => self.log_message(&format!("Error uninstalling Avast Browser: {e:?}")),
                            }
                            "Mcaffee Safe Search" => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Mcaffee Safe Search"),
                                Err(e) => self.log_message(&format!("Error uninstalling Mcaffee Safe Search: {e:?}")),
                            }
                            "Driver Support" => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Driver Support"),
                                Err(e) => self.log_message(&format!("Error uninstalling Driver Support: {e:?}")),
                            }
                            "Winzip" => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Winzip"),
                                Err(e) => self.log_message(&format!("Error uninstalling Winzip: {e:?}")),
                            }
                            "SuperAntiSpyware" =>  self.update_checklist(Category::Tuneup, "Is SuperAntiSpyware installed?", true),
                            "Webroot" => self.update_checklist(Category::Tuneup, "Is Webroot installed?", true),
                            _ => {}
                        }
                    }
                }
            }
        }
        self.update_checklist(Category::JunkwareRemoval, "Wave Browser", true);
        self.update_checklist(Category::JunkwareRemoval, "Clear Browser", true);
        self.update_checklist(Category::JunkwareRemoval, "Shift Browser", true);
        self.update_checklist(Category::JunkwareRemoval, "Avast Browser", true);
        self.update_checklist(Category::JunkwareRemoval, "Mcaffee Safe", true);
        self.update_checklist(Category::JunkwareRemoval, "Driver Support", true);
        self.update_checklist(Category::JunkwareRemoval, "Winzip", true);
        self.update_checklist(Category::JunkwareRemoval, "OneLaunch", true);
        self.update_checklist(Category::JunkwareRemoval, "WebNavigator Browser", true);
    }

    // JunkwareRemoval Items (assuming remove_junkware handles these)
    pub fn remove_onelaunch(&mut self) { self.remove_junkware(Some("OneLaunch")); }
    pub fn remove_webnavigator(&mut self) { self.remove_junkware(Some("WebNavigator Browser")); }
    pub fn remove_wavesor(&mut self) { self.remove_junkware(Some("Wave Browser")); }
    pub fn remove_clearbrowser(&mut self) { self.remove_junkware(Some("Clear Browser")); }
    pub fn remove_shiftbrowser(&mut self) { self.remove_junkware(Some("Shift Browser")); }
    pub fn remove_avastbrowser(&mut self) { self.remove_junkware(Some("Avast Browser")); }
    pub fn remove_mcaffeesafe(&mut self) { self.remove_junkware(Some("Mcaffee Safe")); }
    pub fn remove_driversupport(&mut self) { self.remove_junkware(Some("Driver Support")); }
    pub fn remove_winzip(&mut self) { self.remove_junkware(Some("Winzip")); }
}