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
            "Uninstall Microsoft 365" => self.uninstall_microsoft365(item_text, category),
            "Uninstall OneDrive" => self.uninstall_onedrive(item_text, category),
            "Disable OneDrive Startup" => self.disable_onedrive_startup(item_text, category),
            "Disable Edge Startup Boost" => self.disable_edge_startup_boost(item_text, category),
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

    pub fn uninstall_microsoft365(&mut self, item_text: &str, category: &Category) {
        use powershell_script::PsScriptBuilder;
        self.log_message("Searching for Microsoft 365 / Office installations...");
        let ps = PsScriptBuilder::new()
            .no_profile(true)
            .non_interactive(true)
            .hidden(true)
            .print_commands(false)
            .build();
        let script = r#"
            $paths = @(
                "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*",
                "HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*",
                "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*"
            )
            $office = $paths | ForEach-Object {
                if (Test-Path $_) {
                    Get-ItemProperty $_ -ErrorAction SilentlyContinue |
                        Where-Object { $_.DisplayName -match "Microsoft 365|Microsoft Office" }
                }
            }
            if ($office) {
                foreach ($app in $office) {
                    if ($app.UninstallString) {
                        "Found: $($app.DisplayName) — uninstalling..."
                        $cmd = $app.UninstallString
                        if ($cmd -match "OfficeClickToRun") {
                            & "$env:CommonProgramFiles\Microsoft Shared\ClickToRun\OfficeC2RClient.exe" /uninstall displaylevel=false
                        } elseif ($cmd -match "MsiExec") {
                            $productCode = ([regex]'\{[A-F0-9-]+\}').Match($cmd).Value
                            if ($productCode) { msiexec /x $productCode /quiet /norestart }
                        } else {
                            Invoke-Expression "& $cmd /silent /norestart" 2>$null
                        }
                    }
                }
                "Microsoft 365/Office uninstall initiated"
            } else { "Microsoft 365/Office not found" }
        "#;
        match ps.run(script) {
            Ok(out) => {
                self.log_message(&out.stdout().unwrap_or_default());
                self.update_checklist(category.clone(), item_text, true);
            },
            Err(e) => self.log_message(&format!("Microsoft 365 uninstall failed: {e:?}")),
        }
    }

    pub fn uninstall_onedrive(&mut self, item_text: &str, category: &Category) {
        use powershell_script::PsScriptBuilder;
        self.log_message("Uninstalling OneDrive...");
        let ps = PsScriptBuilder::new()
            .no_profile(true)
            .non_interactive(true)
            .hidden(true)
            .print_commands(false)
            .build();
        let script = r#"
            taskkill /F /IM OneDrive.exe 2>$null
            Start-Sleep -Seconds 1
            $setup64 = "$env:SystemRoot\SysWOW64\OneDriveSetup.exe"
            $setup32 = "$env:SystemRoot\System32\OneDriveSetup.exe"
            if (Test-Path $setup64) {
                & $setup64 /uninstall
                "OneDrive (64-bit) uninstall initiated"
            } elseif (Test-Path $setup32) {
                & $setup32 /uninstall
                "OneDrive (32-bit) uninstall initiated"
            } else {
                winget uninstall "Microsoft.OneDrive" --silent --accept-source-agreements 2>$null
                "Attempted winget uninstall"
            }
        "#;
        match ps.run(script) {
            Ok(out) => {
                self.log_message(&out.stdout().unwrap_or_default());
                self.update_checklist(category.clone(), item_text, true);
            },
            Err(e) => self.log_message(&format!("OneDrive uninstall failed: {e:?}")),
        }
    }

    pub fn disable_onedrive_startup(&mut self, item_text: &str, category: &Category) {
        use powershell_script::PsScriptBuilder;
        self.log_message("Disabling OneDrive startup...");
        let ps = PsScriptBuilder::new()
            .no_profile(true)
            .non_interactive(true)
            .hidden(true)
            .print_commands(false)
            .build();
        let script = r#"
            $runKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
            if (Get-ItemProperty -Path $runKey -Name "OneDrive" -ErrorAction SilentlyContinue) {
                Remove-ItemProperty -Path $runKey -Name "OneDrive" -ErrorAction SilentlyContinue
                "Removed OneDrive from Run key"
            } else { "OneDrive not in Run key" }
            $odPolicies = "HKLM:\SOFTWARE\Policies\Microsoft\OneDrive"
            if (-not (Test-Path $odPolicies)) { New-Item -Path $odPolicies -Force | Out-Null }
            Set-ItemProperty -Path $odPolicies -Name "KFMBlockOptIn" -Value 1 -Type DWord
            "OneDrive Known Folder Move blocked"
            taskkill /F /IM OneDrive.exe 2>$null
        "#;
        match ps.run(script) {
            Ok(out) => {
                self.log_message(&out.stdout().unwrap_or_default());
                self.update_checklist(category.clone(), item_text, true);
            },
            Err(e) => self.log_message(&format!("Disable OneDrive startup failed: {e:?}")),
        }
    }

    pub fn disable_edge_startup_boost(&mut self, item_text: &str, category: &Category) {
        use powershell_script::PsScriptBuilder;
        self.log_message("Disabling Edge startup boost...");
        let ps = PsScriptBuilder::new()
            .no_profile(true)
            .non_interactive(true)
            .hidden(true)
            .print_commands(false)
            .build();
        let script = r#"
            $edgePolicy = "HKLM:\SOFTWARE\Policies\Microsoft\Edge"
            if (-not (Test-Path $edgePolicy)) { New-Item -Path $edgePolicy -Force | Out-Null }
            Set-ItemProperty -Path $edgePolicy -Name "StartupBoostEnabled" -Value 0 -Type DWord
            "Edge StartupBoost disabled"
            Set-ItemProperty -Path $edgePolicy -Name "BackgroundModeEnabled" -Value 0 -Type DWord
            "Edge BackgroundMode disabled"
            taskkill /F /IM msedge.exe 2>$null
        "#;
        match ps.run(script) {
            Ok(out) => {
                self.log_message(&out.stdout().unwrap_or_default());
                self.update_checklist(category.clone(), item_text, true);
            },
            Err(e) => self.log_message(&format!("Disable Edge startup boost failed: {e:?}")),
        }
    }
}