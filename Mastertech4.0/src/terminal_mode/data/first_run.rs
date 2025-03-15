use crate::{app_state::AppState, filesystem::system_info::generate_client_id, pages::login_page::HASH, terminal_mode::TerminalApp, utilities::crypto::pass_hash::load_encrypted_user_data};
use database::{schema::CONNECTED_CLIENT_TABLE, Database, WS_CLIENT_URL};
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
            match loaded_data {
                Some(login) => {
                    // let tx = ctx.db_tx.clone();
                    tokio::spawn(async move {
                        let db = Database::new(login.username, login.password, None).await;
                        log::info!("DB: {db:?}");
                    });
                },
                None => ctx.app_state_tx.try_send(AppState::Login)?
            }
        }
        Ok(())
    }
}