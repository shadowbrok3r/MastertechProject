use crate::{terminal_mode::{systems::{communication_system::DataMessage, notification_system::{Notification, NotificationType}}, TerminalApp}, utilities::load_encrypted_user_data};
use database::{schema::User, Database, db};
use displays::{app_state::{AppState, MainPages}, pages::login_page::HASH};

impl <'a>TerminalApp<'a> {
    pub fn first_run(&mut self) -> anyhow::Result<(), anyhow::Error> {
        if let Ok(ctx) = self.ctx.lock() {
            let loaded_data = load_encrypted_user_data(HASH);
            let app_state_tx = ctx.app_state_tx.clone();
            let data_tx = ctx.data_sender.clone();

            match loaded_data {
                Some(login) => {
                    tokio::spawn(async move {
                        let db_result = match Database::new(login.username.clone(), login.password.clone(), None).await {
                            Ok(db) => Ok(db),
                            Err(e) => {
                                log::warn!("Initial DB signin failed ({e}), checking connectivity...");
                                let already_connected = e.to_string().contains("Already connected");
                                if already_connected {
                                    Err(e)
                                } else {
                                    #[cfg(target_os = "windows")]
                                    match crate::utilities::windows::net_adapter::ensure_internet_connected().await {
                                        Ok(()) => {
                                            log::info!("Internet restored, retrying DB signin...");
                                            Database::new(login.username, login.password, None).await
                                        }
                                        Err(_) => Err(e),
                                    }
                                    #[cfg(not(target_os = "windows"))]
                                    {
                                        Err(e)
                                    }
                                }
                            }
                        };

                        match db_result {
                            Ok(db) => {
                                if let Some(ref usr) = db.user {
                                    data_tx.send(Box::new(Notification::new(
                                        NotificationType::Info, 
                                        "Logged in", 
                                        &format!("Welcome, {}", &usr.get_name()), 
                                        5
                                    )))?;
            
                                    data_tx.send(Box::new(
                                        DataMessage(usr.clone())
                                    ))?;
                                } else {
                                    log::info!("no usr");
                                    let _ = database::db().invalidate().await;
                                    app_state_tx.try_send(AppState::NoAuth("Login".to_string()))?;
                                }
                                app_state_tx.try_send(AppState::Authenticated(MainPages::Tasks))?;
                            },
                            Err(e) => {
                                log::error!("Error with db: {e:?}");
                                let check = e.to_string().contains("Already connected");
                                if check { 
                                    let user = User::get_current_user_from_auth().await;
                                    log::info!("user: {user:?}");
                                    if let Ok(Some(usr)) = user {
                                        data_tx.send(Box::new(
                                            DataMessage(usr.clone())
                                        ))?;
            
                                        data_tx.send(Box::new(Notification::new(
                                            NotificationType::Info, 
                                            "Logged in", 
                                            &format!("Welcome, {}", usr.get_name()), 
                                            5
                                        )))?;
                                    }
                                    app_state_tx.try_send(AppState::Authenticated(MainPages::Tasks))?; 
                                }
                                else { app_state_tx.try_send(AppState::NoAuth(e.to_string()))?; }
                            },
                        }
                        Ok::<(), anyhow::Error>(())
                    });
                },
                None => ctx.app_state_tx.try_send(AppState::NoAuth("Login".to_string()))?
            }
        }
        Ok(())
    }
}