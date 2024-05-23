use log::{debug, warn};
use socketioxide::extract::{SocketRef, State, TryData};

use crate::routes::api::live::client_state::Session;

use super::client_state::{Auth,Sessions};

/// Handles the connection of a new user
pub async fn authenticate_middleware(
    socket: SocketRef,
    TryData(auth): TryData<Auth>,
    State(Sessions(session_state)): State<Sessions>
) -> Result<(), anyhow::Error> {
    let auth = auth?;
    warn!("Client entering middleware => {:#?}", auth.session_id);
    let _ = socket.rooms().map(|x|{debug!("rooms: {x:?}")});
    // Allow writing into the RwLock to operate on session_state
    let mut sessions = session_state.write().await;
    // If we there is a session_id retrieved from Auth { ...session_id }, insert the session into socket extension
    // so we can access the session throughout our connected socket
    if let Some(session) = auth.session_id.and_then(|id| sessions.get_mut(&id)) {
        warn!("we have a session for this user");
    
        if !session.connected {
            warn!("Reactivating inactive session for user");
            session.connected = true; // Reactivate the session
            socket.extensions.insert(session.clone());
        } else {
            warn!("Session is already active for this user");
        }
        socket.extensions.insert(session.clone());
    } else { // Otherwise, create a new session, then insert the session into the socket extension
        warn!("Creating a new session for user");
        let username = auth.username.ok_or(anyhow::anyhow!("invalid username"))?;
        let session = Session::new(username);
        socket.extensions.insert(session.clone());
        sessions.insert(session.session_id, session.clone());
    };
    //explicitly release this write lock. by dropping, we're enforcing the release of the lock before the end of scope.
    drop(sessions);

    // let session = socket.extensions.get::<Session>().unwrap();

    // Join the user to the room specified in Auth { room: .. }
    let _ = socket.join(format!("{:?}", auth.room));
    warn!("joined riv");

    Ok(())
}
