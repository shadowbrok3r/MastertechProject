

use database::{schema::{Priority, Record, Status, Store, TaskPayload}, Database};
use log::info;
use surrealdb::opt::RecordId;
use wasm_bindgen_futures::spawn_local;

use super::Updatable;

impl Updatable for TaskPayload {
    fn update_completed(&self, completed: bool, db: Database) {
        // self.completed = completed;
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!("UPDATE task SET completed=$completed, status=$status WHERE id=$id");
            db.database.set("id", id).await.unwrap();
            db.database.set("completed", completed).await.unwrap();

            if completed{
                db.database.set("status", Status::Complete).await.unwrap();
            }else{
                db.database.set("status", Status::InRepair).await.unwrap();
            }

            let _update_task: Vec<Record> = db
                .database
                .query(query)
                .await
                .unwrap()
                .take(0)
                .unwrap();
        })
    }

    fn update_due_date(&self, due_date: String, db: Database) {
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!("UPDATE task SET due_date=$date WHERE id=$id");

            db.database.set("id", id).await.unwrap();
            db.database.set("date", due_date).await.unwrap();

            let _update_task: Vec<Record> = db
                .database
                .query(query)
                .await
                .unwrap()
                .take(0)
                .unwrap();
        })
    }

    fn update_assignee_initials(&self, initials: String, db: Database) {
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let user_query = format!("SELECT id FROM user WHERE everest_initials=$initials");

            db.database.set("id", id).await.unwrap();
            db.database.set("initials", initials).await.unwrap();
            
            let selected_user: Option<Record> = db
                .database
                .query(user_query)
                .await
                .unwrap()
                .take(0)
                .unwrap();


            let query = format!("UPDATE task SET assignee=$assignee, everest_initials=$initials WHERE id=$id");

            db.database.set("assignee", selected_user.unwrap().id).await.unwrap();
            // db.database.set("initials", initials).await.unwrap();

            let _update_task: Vec<Record> = db
                .database
                .query(query)
                .await
                .unwrap()
                .take(0)
                .unwrap();
        })
    }

    fn update_task_name(&self, name: String, db: Database) {
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!("UPDATE task SET task_name=$name WHERE id=$id");

            db.database.set("id", id).await.unwrap();
            db.database.set("name", name).await.unwrap();

            let _update_task: Vec<Record> = db
                .database
                .query(query)
                .await
                .unwrap()
                .take(0)
                .unwrap();
        })
    }

    fn update_status(&self, status: Status, db: Database) {
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let mut _query = String::new();

            db.database.set("id", id).await.unwrap();

            match status{
                Status::Todo => {
                    _query = format!("UPDATE task SET status=$status, completed=false WHERE id=$id");
                    db.database.set("status", Status::Todo).await.unwrap();
                },
                Status::InRepair => {
                    _query = format!("UPDATE task SET status=$status completed=false WHERE id=$id");
                    db.database.set("status", Status::InRepair).await.unwrap();
                },
                Status::Complete => {
                    _query = format!("UPDATE task SET status=$status completed=true WHERE id=$id");
                    db.database.set("status", Status::Complete).await.unwrap();
                },
            }

            let _update_task: Vec<Record> = db
                .database
                .query(_query)
                .await
                .unwrap()
                .take(0)
                .unwrap();
        })
    }

    fn update_dep(&self, dep: Store, db: Database) {
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!("UPDATE task SET dep=$dep WHERE id=$id");

            db.database.set("id", id).await.unwrap();
            db.database.set("dep", dep).await.unwrap();

            let _update_task: Vec<Record> = db
                .database
                .query(query)
                .await
                .unwrap()
                .take(0)
                .unwrap();
        })
    }

    fn update_priority(&self, priority: Option<Priority>, db: Database) {
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!("UPDATE task SET priority=$priority WHERE id=$id");

            db.database.set("id", id).await.unwrap();
            db.database.set("priority", priority.unwrap()).await.unwrap();

            let _update_task: Vec<Record> = db
                .database
                .query(query)
                .await
                .unwrap()
                .take(0)
                .unwrap();
        })
    }

    fn update_task_description(&self, description: Option<String>, db: Database) {
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!("UPDATE task SET task_description=$description WHERE id=$id");

            db.database.set("id", id).await.unwrap();
            db.database.set("description", description.unwrap()).await.unwrap();

            let _update_task: Vec<Record> = db
                .database
                .query(query)
                .await
                .unwrap()
                .take(0)
                .unwrap();
        })
    }
    
    fn update_recommendations(&self, recommendations: Option<String>, db: Database) {
        let id = self.service_ticket.as_ref();
        let x = id.unwrap().id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!("UPDATE service_order SET recommendations=$recommendations WHERE id=$id");
            info!("Recommendations changed: {}", query);

            db.database.set("recommendations", recommendations.unwrap()).await.unwrap();
            db.database.set("id", x).await.unwrap();

            let _update_task: Vec<Record> = db
                .database
                .query(query)
                .await
                .unwrap()
                .take(0)
                .unwrap();
            info!("update_task: {:?}", _update_task);
        })
    }
    
    fn update_checkin_notes(&self, checkin_notes: Option<String>, db: Database) {
        let id = self.service_ticket.as_ref();
        let x = id.unwrap().id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!("UPDATE service_order SET checkin_notes=$notes WHERE id=$id");

            db.database.set("id", checkin_notes.unwrap()).await.unwrap();
            db.database.set("notes", x).await.unwrap();

            let _update_task: Vec<Record> = db
                .database
                .query(query)
                .await
                .unwrap()
                .take(0)
                .unwrap();
        })
    }
}

