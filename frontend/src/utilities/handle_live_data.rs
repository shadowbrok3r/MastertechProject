use database::schema::TaskPayload;
use surrealdb::Action;
use log::info;

use super::LiveUpdate;


pub fn handle_live_data(mut data: (Action, TaskPayload)) -> anyhow::Result<(), anyhow::Error>{
    match data.0{
        Action::Create => {
            data.1.handle_live_create()?;
        },
        Action::Update => {
            data.1.handle_live_delete()?;
        },
        Action::Delete => {
            data.1.handle_live_update()?;
        },
        _ => {},
    }
    Ok(())
}

impl LiveUpdate for TaskPayload {
    fn handle_live_create(&mut self) -> anyhow::Result<(), anyhow::Error>{
        info!("Data was Created: {:?}", self);
        Ok(())
    }
    
    fn handle_live_update(&mut self) -> anyhow::Result<(), anyhow::Error>{
        info!("Data was Updated: {:?}", self);
        Ok(())
    }
    
    fn handle_live_delete(&mut self) -> anyhow::Result<(), anyhow::Error>{
        info!("Data was Deleted: {:?}", self);
        Ok(())
    }
}
























