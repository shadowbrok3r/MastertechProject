use eframe::egui::{Align, Button, Layout, TextEdit, Ui, Widget};
use log::info;
use tokio::spawn;

use crate::app_state::MastertechContext;

use self::issues::create_new_issue;


pub mod self_updater;
pub mod issues;

impl MastertechContext{
    pub fn github(&mut self, ui: &mut Ui) {
        ui.with_layout(Layout::top_down(Align::Center), |ui| {  // vertical_centered(|ui| {

            ui.heading("Mastertech bug report");
            TextEdit::singleline(&mut self.github_issue_title)
                .hint_text("Issue Title")
                .show(ui);

            ui.add_space(12.0); 

            ui.heading("Description");
            TextEdit::multiline(&mut self.github_issue_descript)
                .hint_text("Explain your issue")
                .show(ui);


            

            let submit = ui.add_enabled(
                !self.github_issue_descript.is_empty() 
                && !self.github_issue_title.is_empty(), 
                Button::new("Submit")
            );

            if submit.clicked(){
                let github_issue_title = self.github_issue_title.clone();
                let github_issue_descript = self.github_issue_descript.clone();
                let client = self.client.clone();
                spawn(async move {
                    let create_issue = create_new_issue(github_issue_title, github_issue_descript, client).await;

                    match create_issue{
                        Ok(val) => info!("Sent request ok: {val:?}"),
                        Err(e) => info!("Error creating issue: {e:?}")
                    }
                });
                
            }
        });
    
    }
}