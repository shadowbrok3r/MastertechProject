use crate::{app_state::{AppState, MainPages}, filesystem::system_info::generate_client_id, pages::login_page::HASH, terminal_mode::{systems::{communication_system::DataMessage, notification_system::{Notification, NotificationType}}, TerminalApp}, utilities::crypto::pass_hash::load_encrypted_user_data};
use database::{schema::{User, CONNECTED_CLIENT_TABLE}, Database, DATABASE, WS_CLIENT_URL};
use surrealdb::RecordId;

impl <'a>TerminalApp<'a> {
    pub fn first_run(&mut self) -> anyhow::Result<(), anyhow::Error> {
        if let Ok(mut ctx) = self.ctx.lock() {
            let service_form = self.service_tab.borrow();
            let service_data = service_form.service_data.lock();
            if let Ok(svc_data) = service_data {
                let computer_data = &svc_data.computer_data;
                let client_hash = generate_client_id(
                    computer_data.hostname.clone(), 
                    computer_data.cpu.trim().to_string()
                );
        
                let url_string = format!(
                    "{}:{}", 
                    computer_data.hostname.clone(), 
                    client_hash.split_at(9).0
                );
        
                ctx.client_title = url_string.clone();

                ctx.url = Some(
                    format!(
                        "{WS_CLIENT_URL}&room_id={}",
                        url_string.clone()
                    )
                );
                
                ctx.client_uuid = RecordId::from_table_key(
                    CONNECTED_CLIENT_TABLE.to_string(), 
                    url_string.clone().as_str()
                );
            }

            let loaded_data = load_encrypted_user_data(HASH);
            let app_state_tx = ctx.app_state_tx.clone();
            let data_tx = ctx.data_sender.clone();

            match loaded_data {
                Some(login) => {
                    tokio::spawn(async move {
                        match Database::new(login.username, login.password, None).await {
                            Ok(db) => {
                                if let Some(ref usr) = db.user {
                                    data_tx.send(Box::new(Notification::new(
                                        NotificationType::Info, 
                                        "Logged in", 
                                        &format!("Welcome, {}", &usr.name), 
                                        5
                                    )))?;
            
                                    data_tx.send(Box::new(
                                        DataMessage(usr.clone())
                                    ))?;
                                }else{ 
                                    log::info!("no usr"); 
                                    let _ = DATABASE.invalidate().await;
                                    app_state_tx.try_send(AppState::Login)?;
                                }
                                app_state_tx.try_send(AppState::Authenticated(MainPages::Tasks))?;
                            },
                            Err(e) => {
                                log::error!("Error with db: {e:?}");
                                let check = e.to_string().contains("Already connected");
                                log::info!("db check: {check}");
                                if check { 
                                    let user: Option<User> = DATABASE.query("SELECT * FROM user WHERE id == $auth.id")
                                        .await?
                                        .take(0)?;
                                    log::info!("user: {user:?}");
                                    if let Some(usr) = user {
            
                                        data_tx.send(Box::new(
                                            DataMessage(usr.clone())
                                        ))?;
            
                                        data_tx.send(Box::new(Notification::new(
                                            NotificationType::Info, 
                                            "Logged in", 
                                            &format!("Welcome, {}", usr.name), 
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
                None => ctx.app_state_tx.try_send(AppState::Login)?
            }
        }
        Ok(())
    }
}