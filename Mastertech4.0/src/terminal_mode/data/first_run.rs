use crate::{app_state::{AppState, MainPages}, pages::login_page::HASH, terminal_mode::{systems::{communication_system::DataMessage, notification_system::{Notification, NotificationType}}, TerminalApp}, utilities::crypto::pass_hash::load_encrypted_user_data};
use database::{schema::utilities::get_current_user_from_auth, Database, DATABASE};

impl <'a>TerminalApp<'a> {
    pub fn first_run(&mut self) -> anyhow::Result<(), anyhow::Error> {
        if let Ok(ctx) = self.ctx.lock() {

            // let mut client = get_client_hash();

            // let connection_url = format!(
            //     "{WS_CLIENT_URL}&room_id={}",
            //     client.id
            // );

            // ctx.url = Some(connection_url.clone());

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

                                    // client.assigned_user = Some(usr.id.clone());

                                    // let create_client = create_client(client.clone()).await;
                                    // log::info!("Client Creation: {create_client:?}");

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
                                    let user = get_current_user_from_auth().await;
                                    log::info!("user: {user:?}");
                                    if let Ok(Some(usr)) = user {
            
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