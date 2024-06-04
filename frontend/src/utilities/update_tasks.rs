

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
            let query = format!(
                "UPDATE task SET completed={completed}, status='{:?}' WHERE id={id}",
                Status::Complete
            );
            info!("ID: {id:?}");
            let update_task: Vec<Record> = db
                .database
                .query(query)
                .await
                .unwrap()
                .take(0)
                .unwrap();


                info!("Updated task: {update_task:#?}");
        })
    }

    fn update_due_date(&self, due_date: String, db: Database) {
        info!("Changing due date to {due_date:?}");
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!(
                "UPDATE task SET due_date='{}' WHERE id={id}", due_date
            );
            let _update_task: Vec<Record> = db
                .database
                .query(query)
                .await
                .unwrap()
                .take(0)
                .unwrap();


            // info!("Updated task: {update_task:#?}");
        })
    }

    fn update_assignee_initials(&self, initials: String, db: Database) {
        // self.everest_initials = Some(initials);
        info!("User initials: {:?}", initials.clone());

        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let user_query = format!(
                "SELECT id FROM user WHERE everest_initials='{initials}'"
            );
            let selected_user: Option<Record> = db
                .database
                .query(user_query)
                .await
                .unwrap()
                .take(0)
                .unwrap();

            info!("User: {selected_user:?}");

            let query = format!(
                "UPDATE task SET assignee={}, everest_initials='{initials}' WHERE id={id}", selected_user.unwrap().id
            );
            let update_task: Vec<Record> = db
                .database
                .query(query)
                .await
                .unwrap()
                .take(0)
                .unwrap();


                info!("Updated task: {update_task:#?}");
        })
    }

    fn update_task_name(&self, name: String, db: Database) {
        // self.task_name = name;
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!(
                "UPDATE task SET task_name='{name}' WHERE id={id}", 
            );
            let update_task: Vec<Record> = db
                .database
                .query(query)
                .await
                .unwrap()
                .take(0)
                .unwrap();


                info!("Updated task: {update_task:#?}");
        })
    }

    fn update_status(&self, status: Status, db: Database) {
        // self.status = status;
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let mut _query = String::new();
            match status{
                Status::Todo => {
                    _query = format!(
                        "UPDATE task SET status='{:?}' WHERE id={id}",
                        Status::Todo
                    );
                },
                Status::InRepair => {
                    _query = format!(
                        "UPDATE task SET status='{:?}' WHERE id={id}",
                        Status::InRepair
                    );
                },
                Status::Complete => {
                    _query = format!(
                        "UPDATE task SET status='{:?}' WHERE id={id}",
                        Status::Complete
                    );
                },
            }

            let update_task: Vec<Record> = db
                .database
                .query(_query)
                .await
                .unwrap()
                .take(0)
                .unwrap();


                info!("Updated task: {update_task:#?}");
        })
    }

    fn update_dep(&self, dep: Store, db: Database) {
        // self.dep = Some(dep);
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!(
                "UPDATE task SET dep='{:?}' WHERE id={id}", dep
            );
            let update_task: Vec<Record> = db
                .database
                .query(query)
                .await
                .unwrap()
                .take(0)
                .unwrap();


                info!("Updated task: {update_task:#?}");
        })
    }

    fn update_priority(&self, priority: Option<Priority>, db: Database) {
        // self.priority = priority;
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!(
                "UPDATE task SET priority='{:?}' WHERE id={id}", priority.unwrap()
            );
            let update_task: Vec<Record> = db
                .database
                .query(query)
                .await
                .unwrap()
                .take(0)
                .unwrap();


                info!("Updated task: {update_task:#?}");
        })
    }

    fn update_task_description(&self, description: Option<String>, db: Database) {
        // self.task_description = description;
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!(
                "UPDATE task SET task_description='{}' WHERE id={id}", description.unwrap()
            );
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
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!(
                "UPDATE service_order SET recommendations='{}' WHERE id={id}", recommendations.unwrap()
            );
            let _update_task: Vec<Record> = db
                .database
                .query(query)
                .await
                .unwrap()
                .take(0)
                .unwrap();
        })
    }
    
    fn update_checkin_notes(&self, checkin_notes: Option<String>, db: Database) {
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!(
                "UPDATE service_order SET checkin_notes='{}' WHERE id={id}", checkin_notes.unwrap()
            );
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

