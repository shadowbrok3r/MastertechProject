use super::{random_record_id, RecordId, SurrealValue};
use serde::{Deserialize, Serialize};
use crate::schema::SALES_NOTE_TABLE;
use crate::schema::user::User;
use crate::db;

#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct SalesNote {
    pub id: RecordId,
    pub user: RecordId,
    pub order_id: String,
    pub note: String,
}

impl SalesNote {
    pub fn new(user: &User, order_id: &str, note: &str) -> Self {
        Self {
            id: random_record_id(SALES_NOTE_TABLE),
            user: user.get_id(),
            order_id: order_id.to_string(),
            note: note.to_string(),
        }
    }
}

pub async fn get_sales_notes_for_user(user: &User, order_ids: Vec<String>) -> anyhow::Result<Vec<SalesNote>, anyhow::Error> {
    if order_ids.is_empty() { return Ok(Vec::new()); }
    let ids_param = order_ids.clone();
    let results: Vec<SalesNote> = db()
        .query("SELECT * FROM sales_note WHERE user == $user AND order_id IN $order_ids")
        .bind(("user", user.get_id()))
        .bind(("order_ids", ids_param))
        .await?
        .take(0)?;
    Ok(results)
}

pub async fn upsert_sales_note(user: &User, order_id: &str, note: &str) -> anyhow::Result<(), anyhow::Error> {
    let _res: Option<SalesNote> = db()
        .query(
            "IF (SELECT * FROM sales_note WHERE user == $user AND order_id == $order_id)[0] != NONE THEN \
             UPDATE sales_note SET note = $note WHERE user == $user AND order_id == $order_id \
             ELSE CREATE sales_note SET user = $user, order_id = $order_id, note = $note END"
        )
    .bind(("user", user.get_id()))
    .bind(("order_id", order_id.to_string()))
    .bind(("note", note.to_string()))
        .await?
        .take(0)?;
    Ok(())
}
