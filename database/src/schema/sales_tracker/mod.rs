use serde::{Deserialize, Serialize};
use surrealdb::RecordId;

use crate::DATABASE;
use crate::schema::user::User;

pub const SALES_NOTE_TABLE: &str = "sales_note";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SalesNote {
    pub id: RecordId,
    pub user: RecordId,
    pub order_id: String,
    pub note: String,
}

impl SalesNote {
    pub fn new(user: &User, order_id: &str, note: &str) -> Self {
        Self {
            id: RecordId::from((SALES_NOTE_TABLE, surrealdb::RecordIdKey::from_inner(surrealdb::sql::Id::rand().into()))),
            user: user.get_id(),
            order_id: order_id.to_string(),
            note: note.to_string(),
        }
    }
}

pub async fn get_sales_notes_for_user(user: &User, order_ids: Vec<String>) -> anyhow::Result<Vec<SalesNote>, anyhow::Error> {
    if order_ids.is_empty() { return Ok(Vec::new()); }
    let ids_param = order_ids.clone();
    let results: Vec<SalesNote> = DATABASE
        .query("SELECT * FROM sales_note WHERE user == $user AND order_id IN $order_ids")
        .bind(("user", user.get_id()))
        .bind(("order_ids", ids_param))
        .await?
        .take(0)?;
    Ok(results)
}

pub async fn upsert_sales_note(user: &User, order_id: &str, note: &str) -> anyhow::Result<(), anyhow::Error> {
    let _res: Option<SalesNote> = DATABASE
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
