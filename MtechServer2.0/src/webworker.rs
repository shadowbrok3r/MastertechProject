use database::{schema::{TaskPayload, User, DB, NS, USER_SCOPE}, Auth, Database, DATABASE, DB_URL_LOCAL};
use gloo_worker::{HandlerId, WorkerScope};
use surrealdb::{engine::remote::ws::Ws, opt::auth::Record};
use wasm_bindgen::{prelude::wasm_bindgen, JsError};
use wasm_bindgen_futures::spawn_local;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use gloo_console::log;

use gloo_worker::Registrable;
fn main() {
    WebWorker::registrar().register();
}

#[derive(Debug)]
pub struct Message(pub u32);

#[derive(Debug, Serialize, Deserialize)]
pub struct Input(pub String, pub String);

#[derive(Debug, Serialize, Deserialize)]
pub struct Output {
    pub tasks: Vec<TaskPayload>,
}

pub struct WebWorker;

impl gloo_worker::Worker for WebWorker {
    type Message = Message;
    type Input = Input;
    type Output = Output;

    fn create(_scope: &WorkerScope<Self>) -> Self {
        log!("create");
        Self { }
    }

    fn update(&mut self, _scope: &WorkerScope<Self>, msg: Self::Message) {
        log!(format!("update {:?}", msg));
    }

    fn received(
        &mut self,
        scope: &WorkerScope<Self>,
        msg: Self::Input,
        id: HandlerId,
    ) {
        log!(format!("received {msg:?}"));
        let scope = scope.clone();
        spawn_local(async move {
            let stuff = get_completed_tasks_for_store(msg).await.unwrap();
            scope.respond(id, Self::Output {
                tasks: stuff,
            });
        });
    }
}


// #[wasm_bindgen]
pub async fn get_completed_tasks_for_store(input: Input) -> Result<Vec<TaskPayload>, JsError> {
    gloo_console::debug!("get_completed_tasks");
    let query = r#"
        SELECT *, (
            SELECT * FROM task_note 
                WHERE task_id == $parent.id
        ) AS task_note 
        FROM task 
        WHERE $this.assignee.store == 'RIV' AND $this.completed IS true
        FETCH 
            service_ticket, 
            service_ticket.computer, 
            service_ticket.customer
        PARALLEL
    "#;
    match DATABASE.connect::<Ws>(DB_URL_LOCAL).await {
        Ok(_) => gloo_console::info!(format!("Connected to {DB_URL_LOCAL:?}")),
        Err(e) => gloo_console::info!(format!("Error connecting to database: {e:?}")),
    } //(&get_db_url()).await?;
    DATABASE.use_ns(NS).use_db(DB).await?;

    gloo_console::debug!("Signing in");
    let signin = DATABASE
        .signin(Record {
            namespace: NS,
            database: DB,
            access: USER_SCOPE,
            params: Auth {
                email: input.0.clone(),
                password: input.1.clone(),
            },
        }).await;


        match signin {
            Ok(o) => {
                gloo_console::warn!(o.as_insecure_token());
            },
            Err(e) => {
                gloo_console::warn!(e.to_string());
            },
        }
    DATABASE
        .set("email", input.0.clone().to_lowercase())
        .await?;

    let user: Result<surrealdb::Response, JsError> = match DATABASE
        .query("SELECT * FROM user WHERE email == $email")
        .await{
            Ok(res) => {
                gloo_console::info!(format!("{:?}", res));
                Ok(res)
            },
            Err(e) => {
                gloo_console::info!(e.to_string());
                Err(JsError::new(&e.to_string()))
            },
        };
    gloo_console::warn!(format!("user: {user:?}"));

    let query_results: Vec<TaskPayload> = DATABASE
        .query(query)
        .await?
        .take(0)?;
    gloo_console::warn!(format!("task len: {}", query_results.len()));

    Ok(query_results)
}
