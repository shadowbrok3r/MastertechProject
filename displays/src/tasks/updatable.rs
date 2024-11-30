use database::{schema::{Priority, Record, Status, Store, TaskNotePayload, TaskPayload}, DATABASE};
use crate::{PlatformSpawner, Spawner, Updatable};
use surrealdb::RecordId;
use log::info;


impl Updatable for TaskPayload {
    fn update_completed(&self, completed: bool) {
        // self.completed = completed;
        let id: RecordId = self.id.clone();
        PlatformSpawner::spawn(async move {
            let query =
                format!("UPDATE task SET completed=$completed, status=$status WHERE id=$id");
            DATABASE.set("id", id).await.unwrap();
            DATABASE.set("completed", completed).await.unwrap();

            if completed {
                DATABASE.set("status", Status::Complete).await.unwrap();
            } else {
                DATABASE.set("status", Status::InRepair).await.unwrap();
            }

            let _update_task: Vec<Record> = DATABASE.query(query).await.unwrap().take(0).unwrap();
        })
    }

    fn update_due_date(&self, due_date: String) {
        let id: RecordId = self.id.clone();
        PlatformSpawner::spawn(async move {
            let query = format!("UPDATE task SET due_date=$date WHERE id=$id");

            DATABASE.set("id", id).await.unwrap();
            DATABASE.set("date", due_date).await.unwrap();

            let _update_task: Vec<Record> = DATABASE.query(query).await.unwrap().take(0).unwrap();
        })
    }

    fn update_assignee_initials(&self, initials: String) {
        let id: RecordId = self.id.clone();
        PlatformSpawner::spawn(async move {
            let user_query = format!("SELECT id FROM user WHERE everest_initials=$initials");

            DATABASE.set("id", id).await.unwrap();
            DATABASE.set("initials", initials).await.unwrap();

            let selected_user: Option<Record> =
                DATABASE.query(user_query).await.unwrap().take(0).unwrap();

            let query = format!(
                "UPDATE task SET assignee=$assignee, everest_initials=$initials WHERE id=$id"
            );

            DATABASE
                .set("assignee", selected_user.unwrap().id)
                .await
                .unwrap();
            // DATABASE.set("initials", initials).await.unwrap();

            let _update_task: Vec<Record> = DATABASE.query(query).await.unwrap().take(0).unwrap();
        })
    }

    fn update_task_name(&self, name: String) {
        let id: RecordId = self.id.clone();
        PlatformSpawner::spawn(async move {
            let query = format!("UPDATE task SET task_name=$name WHERE id=$id");

            DATABASE.set("id", id).await.unwrap();
            DATABASE.set("name", name).await.unwrap();

            let _update_task: Vec<Record> = DATABASE.query(query).await.unwrap().take(0).unwrap();
        })
    }

    fn update_status(&self, status: Status) {
        let id: RecordId = self.id.clone();
        PlatformSpawner::spawn(async move {
            let mut _query = String::new();

            DATABASE.set("id", id).await.unwrap();

            match status {
                Status::Todo => {
                    _query =
                        format!("UPDATE task SET status=$status, completed=false WHERE id=$id");
                    DATABASE.set("status", Status::Todo).await.unwrap();
                }
                Status::InRepair => {
                    _query =
                        format!("UPDATE task SET status=$status, completed=false WHERE id=$id");
                    DATABASE.set("status", Status::InRepair).await.unwrap();
                }
                Status::Complete => {
                    _query = format!("UPDATE task SET status=$status, completed=true WHERE id=$id");
                    DATABASE.set("status", Status::Complete).await.unwrap();
                }
            }

            let _update_task: Vec<Record> = DATABASE.query(_query).await.unwrap().take(0).unwrap();
        })
    }

    fn update_dep(&self, dep: Store) {
        let id: RecordId = self.id.clone();
        PlatformSpawner::spawn(async move {
            let query = format!("UPDATE task SET dep=$dep WHERE id=$id");

            DATABASE.set("id", id).await.unwrap();
            DATABASE.set("dep", dep).await.unwrap();

            let _update_task: Vec<Record> = DATABASE.query(query).await.unwrap().take(0).unwrap();
        })
    }

    fn update_priority(&self, priority: Option<Priority>) {
        let id: RecordId = self.id.clone();
        PlatformSpawner::spawn(async move {
            let query = format!("UPDATE task SET priority=$priority WHERE id=$id");

            DATABASE.set("id", id).await.unwrap();
            DATABASE.set("priority", priority.unwrap()).await.unwrap();

            let _update_task: Vec<Record> = DATABASE.query(query).await.unwrap().take(0).unwrap();
        })
    }

    fn update_task_description(&self, description: String) {
        let id: RecordId = self.id.clone();
        PlatformSpawner::spawn(async move {
            let query = format!("UPDATE task SET task_description=$description WHERE id=$id");

            DATABASE.set("id", id).await.unwrap();
            DATABASE.set("description", description).await.unwrap();

            let _update_task: Vec<Record> = DATABASE.query(query).await.unwrap().take(0).unwrap();
        })
    }

    fn update_checkin_notes(&self, checkin_notes: Option<String>) {
        let id = self.service_ticket.as_ref();
        let x = id.unwrap().id.clone();
        PlatformSpawner::spawn(async move {
            let query = format!("UPDATE service_order SET checkin_notes=$notes WHERE id=$id");

            DATABASE.set("id", checkin_notes.unwrap()).await.unwrap();
            DATABASE.set("notes", x).await.unwrap();

            let _update_task: Vec<Record> = DATABASE.query(query).await.unwrap().take(0).unwrap();
        })
    }

    fn update_task_notes(&self, new_msg: String) {
        let task_note = TaskNotePayload {
            task_id: Some(self.id.clone()),
            note: new_msg,

            ..Default::default()
        };

        PlatformSpawner::spawn(async move {
            let query = format!("CREATE task_note CONTENT $note");

            // DATABASE.set("id", id.0).await.unwrap();
            DATABASE.set("note", task_note).await.unwrap();

            let update_task: Vec<Record> = DATABASE.query(query).await.unwrap().take(0).unwrap();
            info!("Updated notes: {update_task:?}");
        })
    }
}
