use crate::routes::api::live::client_state::Session;
use database::{schema::{Notification, TaskPayload, NOTIFICATION_TABLE, TASK_TABLE}, Database};
use core::fmt::Debug;
use futures::StreamExt;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use socketioxide::extract::{Data, SocketRef, State};
use surrealdb::engine::remote::ws::Client;
use surrealdb::method::Stream;
use surrealdb::Notification as SurrealNotification;

use self::client_state::{Sessions, Store};

pub mod client_state;
pub mod websocket_middleware;


pub async fn live_connection_handler(socket: SocketRef) {
    socket.on(
        "listen_tasks",
        |socket: SocketRef, 
            Data::<Store>(room), 
            db: State<Database>,
            State(Sessions(_sessions))
        | async move {
            let session = socket.extensions.get::<Session>().map(|sess| sess.session_id);
            let mut other_count = 0;

            let _ = socket.rooms().map(|x| {
                debug!("listen_tasks rooms => {x:?}");
            });
            
            if let Some(session_id) = session{
                other_count += 1;
                info!("Listening to tasks {other_count}, session id: {:?} on room {:?}", session_id, room);
                let _ = tokio::spawn(async move {
                    let task_stream: Stream<'_, Client, Vec<TaskPayload>> =
                        db.database.select(TASK_TABLE).live().await.unwrap();
                    // let ticket_stream: Stream<'_, Client, Vec<TicketData>> =
                    //     db.database.select(TICKET_TABLE).live().await.unwrap();
                    handle_streams(task_stream, socket,format!("{:?}", room), "task_notification".to_string()).await;
                }).await;
            }else{
                warn!("listen_tasks => no session_id was found");
            }
    });

    socket.on(
        "listen_notifications",
        |socket: SocketRef, 
            Data::<Store>(room), 
            db: State<Database>,
            State(Sessions(_sessions))
        | async move {
            let session = socket.extensions.get::<Session>().map(|sess| sess.session_id);
            let mut other_count = 0;

            let _ = socket.rooms().map(|x| {
                debug!("listen_notifications rooms => {x:?}");
            });
            
            if let Some(session_id) = session{
                other_count += 1;
                info!("Listening to notifications {other_count}, session id: {:?} on room {:?}", session_id, room);
                let _ = tokio::spawn(async move {
                    let notification_stream: Stream<'_, Client, Vec<Notification>> =
                        db.database.select(NOTIFICATION_TABLE).live().await.unwrap();
                    handle_streams(notification_stream, socket,format!("{:?}", room), "notification".to_string()).await;
                }).await;
            }else{
                warn!("listen_tasks => no session_id was found");
            }
    });

    socket.on(
        "getTasks",
        |socket: SocketRef, 
        db: State<Database>, 
        State(Sessions(_sessions))
    | async move {
        let session = socket.extensions.get::<Session>().map(|sess| sess.session_id);
        
        if let Some(_session_id) = session{
            let task_data: Vec<Value> = db
            .database
            .query( // WHERE service_number = {}
                "SELECT * FROM task ORDER BY due_date DESC 
                FETCH service_ticket, service_ticket.computer, service_ticket.customer, task_note"
            )
            .await
            .unwrap()
            .take(0)
            .unwrap();

            let _ = socket.emit("currentTasks", [task_data]);
        }else{
            warn!("listen_tasks => no session_id was found");
        }
    });

    socket.on(
        "join",
        |socket: SocketRef, 
            Data::<Store>(room), 
            State(Sessions(_session_state))
    | async move {
        warn!("joining a room");
        let _ = socket.leave_all();
        let _ = socket.join(format!("{:?}", room));
        
        let session_ref = socket.extensions.get::<Session>().map(|sess|
            sess.clone().session_id
        );
        
        // let mut sessions = session_state.write().await;
        let _ = socket.rooms().map(|x| {
            debug!("rooms before join => {x:?}");
        });

        if let Some(session_id) = session_ref{
            warn!("session_id: {session_id:?}");

            let _ = socket.emit("session", session_id.to_string());
        }

    });

    socket.on_disconnect(| socket: SocketRef, State(Sessions(session_state)) | async move{
        error!("Client disconnecting");
        let session = socket.extensions.remove::<Session>().unwrap().clone();
        let _ = socket.rooms().map(|x| {
            debug!("rooms before on_disconnect => {x:?}");
        });
        session_state
            .write()
            .await
            .get_mut(&session.session_id)
            .unwrap()
            .connected = false;

        session_state
            .write()
            .await
            .remove(&session.session_id);

        let _ = socket.disconnect();
        // let _ = socket.leave_all();
        warn!("Left all rooms"); 
    });
    
}

async fn handle_streams<T>(
    mut notification_stream: impl futures::Stream<Item = Result<SurrealNotification<T>, surrealdb::Error>> + Unpin,
    socket: SocketRef,
    room: String,
    event: String
) where T: Serialize + Deserialize<'static> + Debug {
    let mut count = 0;

    let id = socket.id;

    while let Some(notification) = notification_stream.next().await {
        count += 1; 
        match notification{
            Ok(notification) => {
                // let action = notification.action;
                let data = notification.data;
                let action = format!("{:?}", notification.action);

                if !socket.connected(){
                    let _ = socket.disconnect();
                    break;
                }else{
                    let _ = socket.within(room.to_owned()).emit(event.clone(), serde_json::json!({"action": action, "data": data})).unwrap();
                }
                info!("{count}\n{id:?}\n{:?}", socket.id);
                error!("socket.connected(): {:?}", socket.connected());
            },
            Err(err) => {
                error!("Error: {err:?}");
                // let _ =  socket.within(room.to_owned()).emit("error", err).unwrap();
            }
        };
    }; 
}