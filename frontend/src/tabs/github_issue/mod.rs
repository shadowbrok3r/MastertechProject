use eframe::egui::{Align, Button, Color32, Layout, Stroke, TextEdit, Ui};
use reqwest::{header::{ACCEPT, AUTHORIZATION, USER_AGENT}, Client};
use wasm_bindgen_futures::spawn_local;
use log::info;

use crate::app_state::MtechServerContext;

const TOKEN: &str = "Bearer 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";


pub struct GithubIssue {
    pub github_issue_descript: String,
    pub github_issue_title: String,
}


impl MtechServerContext{
    pub fn github(&mut self, ui: &mut Ui) {
        ui.style_mut().visuals.selection.stroke.color =  Color32::BLACK;
        ui.style_mut().visuals.selection.bg_fill = Color32::from_rgb(120, 10, 120);
        ui.style_mut().visuals.widgets.inactive.fg_stroke =  Stroke::new(1.0, Color32::WHITE);
        ui.style_mut().visuals.widgets.inactive.weak_bg_fill =  Color32::from_rgb(20, 20, 25);
        ui.style_mut().visuals.widgets.inactive.bg_stroke =  Stroke::new(1.0, Color32::from_rgb(80, 80, 80));
        ui.style_mut().visuals.widgets.open.bg_fill =  Color32::from_black_alpha(50);
        ui.style_mut().visuals.widgets.open.weak_bg_fill =  Color32::from_black_alpha(50);
        ui.style_mut().visuals.widgets.active.weak_bg_fill =  Color32::from_rgb(30,30,30);
        ui.style_mut().visuals.widgets.hovered.weak_bg_fill =  Color32::TRANSPARENT;
        ui.style_mut().visuals.widgets.hovered.bg_fill =  Color32::from_rgb(12, 12, 12);
        ui.style_mut().visuals.widgets.hovered.bg_stroke =  Stroke::new(1.0, Color32::from_rgb(200, 20, 200));

        self.github_issue.display(ui);
    }
}

impl GithubIssue {
    pub fn new() -> Self {
        Self {
            github_issue_descript: String::new(),
            github_issue_title: String::new(),
        }
    }

    fn display(&mut self, ui: &mut Ui) {
        ui.with_layout(Layout::top_down(Align::Center), |ui| {  // vertical_centered(|ui| {

            ui.heading("MtechServer Bug Report");
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

                spawn_local(async move {
                    let client = Client::new();
                    
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

pub async fn create_new_issue(title: String, body: String, client: Client)  
    -> anyhow::Result<String, anyhow::Error> 
{
    let params = serde_json::json!({
        "title": title,
        "body": body,
        "assignees": ["shadowbrok3r"],
        "labels": [
            "bug"
        ]
    });

    let res = client
        .post("https://api.github.com/repos/shadowbrok3r/MtechServer2.0/issues")
        .header(AUTHORIZATION, TOKEN)
        .header(ACCEPT, "application/vnd.github+json")
        .header(USER_AGENT, "MtechServer")
        .json(&params)
        .send()
        .await?
        .text()
        .await?;

    Ok(res)
}