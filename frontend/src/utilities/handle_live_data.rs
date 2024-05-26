use database::schema::TaskPayload;
use surrealdb::Action;
use log::info;


pub fn handle_live_data(data: (Action, TaskPayload)) -> anyhow::Result<(), anyhow::Error>{
    match data.0{
        Action::Create => {
            handle_live_create(data.1);
        },
        Action::Update => {
            handle_live_delete(data.1);
        },
        Action::Delete => {
            handle_live_update(data.1);
        },
        _ => {},
    }
    Ok(())
}

pub fn handle_live_create(data: TaskPayload) -> anyhow::Result<(), anyhow::Error>{
    info!("Data was Created: {:?}", data.1);
    Ok(())
}

pub fn handle_live_update(data: TaskPayload) -> anyhow::Result<(), anyhow::Error>{
    info!("Data was Updated: {:?}", data.1);
    Ok(())
}

pub fn handle_live_delete(data: TaskPayload) -> anyhow::Result<(), anyhow::Error>{
    info!("Data was Deleted: {:?}", data.1);
    Ok(())
}

