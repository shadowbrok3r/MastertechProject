use axum::{Router, routing::{post, get}};

use super::{
    api::{ 
        notifications::{handle_get_notifications, handle_notification_modification}, task_notes::handle_create_task_note, tasks::{
            handle_create_task, handle_get_tasks, handle_task_modification
        }, tickets::{
            handle_create_ticket, 
            handle_get_ticket, handle_update_spo
        }
    }, 
    user::{
        get_current_user, 
        get_users_in_store
    }
};

pub fn routes() -> Router {
    Router::new()
        .route(
            "/api/submitTicket", 
            post(handle_create_ticket)
        )

        .route(
            "/api/createTask", 
            post(handle_create_task)
        )
        .route(
            "/api/createChatMessage", 
            post(handle_create_task_note)
        )
        .route(
            "/api/modifyTask", 
            post(handle_task_modification)
        )
        .route(
            "/api/modifyNotification",
            post(handle_notification_modification)
        )
        .route(
            "/api/getTasks", 
            get(handle_get_tasks) 
        )
        .route(
            "/api/getTicket", 
            get(handle_get_ticket) 
        )
        .route(
            "/api/updateSpo", 
            post(handle_update_spo) 
        )
        .route(
            "/api/getNotifications",
            post(handle_get_notifications)
        )
        .route(
            "/api/getCurrentUser",
            post(get_current_user)
        )        
        .route(
            "/api/getStoreUsers",
            post(get_users_in_store)
        )
        .route(
            "/api/sql", 
            get(handle_get_ticket)
        )
        // .route(
        //     "/api/import", 
        //     get(import_surql)
        // )
        // .route(
        //     "/api/export", 
        //     get(export_surql)
        // )
}