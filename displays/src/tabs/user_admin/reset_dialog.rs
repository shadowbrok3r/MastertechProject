//! The "Reset a password" dialog Managers and Root open from the account menu, and the code card.

use crossbeam::channel::{Receiver, Sender};
use database::schema::user::User;
use database::schema::user::password_reset::{self, IssuedCode, RESET_CODE_DIGITS};
use database::schema::{Datetime, RecordId, UserAuthorization};
use eframe::egui::{Context, Id, RichText, Ui};

use super::spawn_job;
use crate::ui_tools::controls::{self, IconBox, IconButton, ModalWidth, Role, Tokens};
use crate::ui_tools::{fonts, icons::svg};

/// Point size of a shown reset code.
const CODE_PT: f32 = 30.0;

/// Clock time of `at` in this machine's zone.
pub(super) fn local_clock(at: &Datetime) -> String {
    at.with_timezone(&chrono::Local)
        .format("%-I:%M %p")
        .to_string()
}

/// The code with a space between its halves.
fn spaced(code: &str) -> String {
    let half = RESET_CODE_DIGITS / 2;
    match (code.get(..half), code.get(half..)) {
        (Some(head), Some(tail)) if code.len() == RESET_CODE_DIGITS => format!("{head} {tail}"),
        _ => code.to_string(),
    }
}

/// A just-issued code for `name`, with its expiry and how to use it.
pub fn code_card(ui: &mut Ui, code: &IssuedCode, name: &str) {
    let t = Tokens::get(ui.ctx());
    controls::callout(ui, t.success, svg::KEY, |ui| {
        ui.label(RichText::new(format!("Reset code for {name}")).strong());
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(spaced(&code.code))
                    .font(fonts::mono(CODE_PT))
                    .color(t.text),
            );
            if IconButton::new(svg::COPY, IconBox::Md)
                .tip("Copy the code")
                .show(ui)
                .clicked()
            {
                ui.ctx().copy_text(code.code.clone());
            }
        });
        ui.label(
            RichText::new(format!(
                "Works once, until {}. On the sign-in screen they choose \"Forgot password?\" and enter their \
                 username, this code and a new password.",
                local_clock(&code.expires_at)
            ))
            .weak(),
        );
    });
}

/// Who a dialog result belongs to, and the result.
type Issued = (String, Result<IssuedCode, String>);

pub struct ResetCodeDialog {
    open: bool,
    target: Option<RecordId>,
    issuing: bool,
    issued: Option<(String, IssuedCode)>,
    error: Option<String>,
    tx: Sender<Issued>,
    rx: Receiver<Issued>,
}

impl Default for ResetCodeDialog {
    fn default() -> Self {
        let (tx, rx) = crossbeam::channel::unbounded();
        Self {
            open: false,
            target: None,
            issuing: false,
            issued: None,
            error: None,
            tx,
            rx,
        }
    }
}

impl ResetCodeDialog {
    /// Whether `user` may issue codes: an active Root or Manager.
    pub fn available_to(user: Option<&User>) -> bool {
        user.is_some_and(|u| {
            u.is_active()
                && matches!(
                    u.get_authorization(),
                    UserAuthorization::Root | UserAuthorization::Manager
                )
        })
    }

    pub fn open(&mut self) {
        self.open = true;
        self.target = None;
        self.issued = None;
        self.error = None;
    }

    fn drain(&mut self) {
        while let Ok((name, result)) = self.rx.try_recv() {
            self.issuing = false;
            match result {
                Ok(code) => {
                    self.issued = Some((name, code));
                    self.error = None;
                }
                Err(e) => self.error = Some(e),
            }
        }
    }

    /// Draws the dialog while it is open; call once per frame.
    pub fn ui(&mut self, ctx: &Context, me: Option<&User>) {
        self.drain();
        if !self.open {
            return;
        }
        let Some(me) = me.filter(|u| Self::available_to(Some(u))) else {
            self.open = false;
            return;
        };
        let users = crate::get_database_users();
        let targets = password_reset::reset_targets(me, &users);
        let labels: Vec<(Option<RecordId>, String)> = targets
            .iter()
            .map(|u| {
                let label = format!(
                    "{} · {} · {}",
                    u.get_name(),
                    u.get_store().as_str(),
                    u.get_authorization().as_str()
                );
                (Some(u.get_id()), label)
            })
            .collect();
        let scope = if me.get_authorization() == UserAuthorization::Root {
            "As Root you can reset anyone's password but your own.".to_string()
        } else {
            format!(
                "Managers can reset User and Warehouse accounts at {}.",
                me.get_store().as_str()
            )
        };

        let buttons: &[(Role, &str)] = if self.issued.is_some() {
            &[(Role::Primary, "Done"), (Role::Secondary, "Reset another")]
        } else {
            &[(Role::Primary, "Issue code"), (Role::Cancel, "Cancel")]
        };
        let t = Tokens::get(ctx);
        let outcome = controls::modal(
            ctx,
            Id::new("reset_code_dialog"),
            svg::KEY,
            "Reset a password",
            ModalWidth::Narrow,
            buttons,
            |ui| {
                if let Some((name, code)) = &self.issued {
                    code_card(ui, code, name);
                    return;
                }
                ui.label(&scope);
                let options: Vec<(Option<RecordId>, &str)> = labels
                    .iter()
                    .map(|(id, label)| (id.clone(), label.as_str()))
                    .collect();
                if options.is_empty() {
                    controls::note(
                        ui,
                        svg::INFO,
                        "There is no account you can reset.",
                        t.text_dim,
                    );
                } else {
                    controls::Dropdown::new("reset_code_target", &mut self.target, &options)
                        .placeholder("Choose an account")
                        .width(ui.available_width())
                        .show(ui);
                }
                if self.issuing {
                    controls::loading_row(ui, "Issuing a code…");
                }
                if let Some(e) = &self.error {
                    controls::note(ui, svg::WARNING_CIRCLE, e, t.error);
                }
            },
        );

        match (self.issued.is_some(), outcome.clicked) {
            (true, Some(0)) => self.open = false,
            (true, Some(1)) => {
                self.issued = None;
                self.target = None;
            }
            (false, Some(0)) => match self.target.clone() {
                Some(id) if !self.issuing => {
                    let name = targets
                        .iter()
                        .find(|u| u.get_id() == id)
                        .map(|u| u.get_name().to_string())
                        .unwrap_or_default();
                    self.issuing = true;
                    self.error = None;
                    spawn_job(ctx, &self.tx, async move {
                        let result = password_reset::issue_reset_code(&id)
                            .await
                            .map_err(|e| e.to_string());
                        (name, result)
                    });
                }
                Some(_) => {}
                None => self.error = Some("Choose an account first.".to_string()),
            },
            _ => {}
        }
        if outcome.should_close {
            self.open = false;
        }
        if self.issuing {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_split_in_half() {
        assert_eq!(spaced("123456"), "123 456");
        assert_eq!(spaced("12345"), "12345");
    }
}
