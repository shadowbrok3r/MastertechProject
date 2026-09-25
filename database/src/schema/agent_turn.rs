//! A technician's instruction to a running agent_thread.
//!
//! Desktops queue rows; the admin-agent broker claims each one and forwards it
//! to codex, so a tech never needs a socket to the agent host.

use serde::{Deserialize, Serialize};

use super::{Datetime, RecordId, SurrealValue};
use crate::db;

pub const AGENT_TURN_TABLE: &str = "agent_turn";

/// Staging directory zc-codexd reports when its hello names none.
pub const DEFAULT_UPLOAD_DIR: &str = "/tmp/zc-codexd-uploads";

/// A picture a turn carries: base64 `data` until the broker stages it, then the staged `path`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default, SurrealValue)]
pub struct TurnImage {
    #[serde(default)]
    #[surreal(default)]
    pub name: String,
    #[serde(default)]
    #[surreal(default)]
    pub mime: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[surreal(default)]
    pub data: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[surreal(default)]
    pub path: Option<String>,
}

impl TurnImage {
    /// The copy the broker keeps once the picture is staged at `path`.
    pub fn staged(&self, path: String) -> Self {
        Self {
            name: self.name.clone(),
            mime: self.mime.clone(),
            data: None,
            path: Some(path),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct AgentTurn {
    pub id: RecordId,
    pub thread: RecordId,
    /// `start`, `steer`, `interrupt`, `close`, `queue`, `compact` or `rename`.
    #[serde(default)]
    #[surreal(default)]
    pub kind: String,
    #[serde(default)]
    #[surreal(default)]
    pub text: String,
    #[serde(default)]
    #[surreal(default)]
    pub tech: Option<String>,
    /// `pending`, then `queued`/`held` while a queue turn waits, then `sent`, `failed` or `cancelled`.
    #[serde(default)]
    #[surreal(default)]
    pub status: String,
    #[serde(default)]
    #[surreal(default)]
    pub error: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub images: Vec<TurnImage>,
    #[serde(default)]
    #[surreal(default)]
    pub created_at: Option<Datetime>,
    #[serde(default)]
    #[surreal(default)]
    pub sent_at: Option<Datetime>,
}

/// A queue turn still waiting to be sent, without its image data.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct QueuedTurn {
    pub id: RecordId,
    #[serde(default)]
    #[surreal(default)]
    pub text: String,
    #[serde(default)]
    #[surreal(default)]
    pub status: String,
    #[serde(default)]
    #[surreal(default)]
    pub tech: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub image_names: Vec<String>,
    #[serde(default)]
    #[surreal(default)]
    pub created_at: Option<Datetime>,
}

impl QueuedTurn {
    pub fn is_held(&self) -> bool {
        self.status == "held"
    }
}

/// Why a turn of `kind` with this content would be refused before it is written.
pub fn refusal(kind: &str, text: &str, images: &[TurnImage]) -> Option<&'static str> {
    let empty = text.trim().is_empty() && images.is_empty();
    match kind {
        "start" | "steer" if empty => Some("empty message"),
        "rename" if text.trim().is_empty() => Some("empty title"),
        _ => None,
    }
}

impl AgentTurn {
    /// Queues an instruction; `tech` defaults from `$auth` in the schema.
    pub async fn ask(thread: &RecordId, kind: &str, text: &str) -> anyhow::Result<RecordId> {
        Self::ask_with(thread, kind, text, &[]).await
    }

    /// Queues an instruction carrying pictures.
    pub async fn ask_with(
        thread: &RecordId,
        kind: &str,
        text: &str,
        images: &[TurnImage],
    ) -> anyhow::Result<RecordId> {
        if let Some(why) = refusal(kind, text, images) {
            anyhow::bail!(why);
        }
        let images = (!images.is_empty()).then(|| images.to_vec());
        let mut res = db()
            .query(
                "CREATE agent_turn CONTENT { thread: $thread, kind: $kind, text: $text, status: 'pending', \
                 images: $images } RETURN VALUE id",
            )
            .bind(("thread", thread.clone()))
            .bind(("kind", kind.to_string()))
            .bind(("text", text.trim().to_string()))
            .bind(("images", images))
            .await?
            .check()?;
        let ids: Vec<RecordId> = res.take(0).unwrap_or_default();
        ids.into_iter().next().ok_or_else(|| anyhow::anyhow!("agent_turn was not created"))
    }

    /// Guarded claim so one broker owns a row even with several running; a queue turn moves to `queued`.
    pub async fn claim(id: &RecordId) -> anyhow::Result<bool> {
        let mut res = db()
            .query(
                "UPDATE $id SET status = IF kind = 'queue' THEN 'queued' ELSE 'sent' END, \
                 sent_at = IF kind = 'queue' THEN sent_at ELSE time::now() END \
                 WHERE status = 'pending' RETURN VALUE id",
            )
            .bind(("id", id.clone()))
            .await?;
        let claimed: Vec<RecordId> = res.take(0).unwrap_or_default();
        Ok(!claimed.is_empty())
    }

    /// Moves a waiting queue turn to `sent`; false when it was cancelled or held first.
    pub async fn take_queued(id: &RecordId) -> anyhow::Result<bool> {
        let mut res = db()
            .query("UPDATE $id SET status = 'sent', sent_at = time::now() WHERE status = 'queued' RETURN VALUE id")
            .bind(("id", id.clone()))
            .await?;
        let taken: Vec<RecordId> = res.take(0)?;
        Ok(!taken.is_empty())
    }

    /// Replaces the row's pictures with their staged copies, which carry no data.
    pub async fn record_staged(id: &RecordId, images: &[TurnImage]) -> anyhow::Result<()> {
        db().query("UPDATE $id SET images = $images")
            .bind(("id", id.clone()))
            .bind(("images", images.to_vec()))
            .await?
            .check()?;
        Ok(())
    }

    /// Fails the row and drops any picture data it still holds.
    pub async fn mark_failed(id: &RecordId, error: &str) -> anyhow::Result<()> {
        db().query(
            "UPDATE $id SET status = 'failed', error = $error, \
             images = IF images != NONE THEN images.map(|$i| { name: $i.name, mime: $i.mime, path: $i.path }) END",
        )
        .bind(("id", id.clone()))
        .bind(("error", error.chars().take(400).collect::<String>()))
        .await?;
        Ok(())
    }

    /// Cancels a queue turn that has not been sent; false when it already went out.
    pub async fn cancel(id: &RecordId) -> anyhow::Result<bool> {
        Ok(Self::take_back(id).await?.is_some())
    }

    /// Cancels a queue turn that has not been sent and returns it as it was, pictures included.
    pub async fn take_back(id: &RecordId) -> anyhow::Result<Option<Self>> {
        let mut res = db()
            .query(
                "UPDATE $id SET status = 'cancelled', error = 'removed from the queue' \
                 WHERE status IN ['pending', 'queued', 'held'] RETURN BEFORE",
            )
            .bind(("id", id.clone()))
            .await?;
        let rows: Vec<Self> = res.take(0)?;
        Ok(rows.into_iter().next())
    }

    /// Cancels every queue turn of a thread that has not been sent.
    pub async fn cancel_waiting(thread: &RecordId, why: &str) -> anyhow::Result<()> {
        db().query(
            "UPDATE agent_turn SET status = 'cancelled', error = $why, \
             images = IF images != NONE THEN images.map(|$i| { name: $i.name, mime: $i.mime, path: $i.path }) END \
             WHERE thread = $thread AND kind = 'queue' AND status IN ['pending', 'queued', 'held']",
        )
        .bind(("thread", thread.clone()))
        .bind(("why", why.to_string()))
        .await?
        .check()?;
        Ok(())
    }

    /// Holds a thread's waiting queue turns until a resume releases them.
    pub async fn hold_queue(thread: &RecordId) -> anyhow::Result<()> {
        db().query("UPDATE agent_turn SET status = 'held' WHERE thread = $thread AND kind = 'queue' AND status = 'queued'")
            .bind(("thread", thread.clone()))
            .await?
            .check()?;
        Ok(())
    }

    /// Releases a thread's held queue turns.
    pub async fn release_queue(thread: &RecordId) -> anyhow::Result<()> {
        db().query("UPDATE agent_turn SET status = 'queued' WHERE thread = $thread AND kind = 'queue' AND status = 'held'")
            .bind(("thread", thread.clone()))
            .await?
            .check()?;
        Ok(())
    }

    /// A thread's claimed queue turns, oldest first, pictures included.
    pub async fn queue_of(thread: &RecordId) -> anyhow::Result<Vec<Self>> {
        let mut res = db()
            .query(
                "SELECT * FROM agent_turn WHERE thread = $thread AND kind = 'queue' \
                 AND status IN ['queued', 'held'] ORDER BY created_at ASC",
            )
            .bind(("thread", thread.clone()))
            .await?;
        Ok(res.take(0)?)
    }

    /// A thread's queue turns not yet sent, oldest first, with picture names only.
    pub async fn waiting(thread: &RecordId) -> anyhow::Result<Vec<QueuedTurn>> {
        let mut res = db()
            .query(
                "SELECT id, text, status, tech, created_at, \
                 IF images != NONE THEN images.map(|$i| $i.name) ELSE [] END AS image_names \
                 FROM agent_turn WHERE thread = $thread AND kind = 'queue' \
                 AND status IN ['pending', 'queued', 'held'] ORDER BY created_at ASC",
            )
            .bind(("thread", thread.clone()))
            .await?;
        Ok(res.take(0)?)
    }

    /// Rows still waiting, oldest first; drains what a LIVE SELECT missed.
    pub async fn pending(limit: usize) -> anyhow::Result<Vec<Self>> {
        let mut res = db()
            .query("SELECT * FROM agent_turn WHERE status = 'pending' ORDER BY created_at ASC LIMIT $limit")
            .bind(("limit", limit))
            .await?;
        Ok(res.take(0).unwrap_or_default())
    }
}

/// FNV-1a over `bytes`.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x1000_0000_01b3)
    })
}

/// A staged picture's file name: its own made path-safe, plus a hash of its bytes.
pub fn upload_name(name: &str, bytes: &[u8]) -> String {
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let (stem, ext) = match safe.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => {
            (stem, format!(".{}", ext.to_ascii_lowercase()))
        }
        _ => (safe.as_str(), String::new()),
    };
    format!("{stem}-{:08x}{ext}", fnv1a(bytes) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use surrealdb::types::Value;

    fn sample() -> AgentTurn {
        AgentTurn {
            id: RecordId::new(AGENT_TURN_TABLE, "t1"),
            thread: RecordId::new("agent_thread", "a1"),
            kind: "queue".into(),
            text: "check the event log".into(),
            tech: Some("tech@example.com".into()),
            status: "queued".into(),
            error: None,
            images: vec![TurnImage {
                name: "shot.png".into(),
                mime: "image/png".into(),
                data: Some("AAAA".into()),
                path: None,
            }],
            created_at: None,
            sent_at: None,
        }
    }

    #[test]
    fn a_row_written_before_images_existed_still_loads() {
        let mut v = sample().into_value();
        if let Value::Object(obj) = &mut v {
            obj.remove("images");
            obj.remove("tech");
            obj.remove("status");
        }
        let turn = AgentTurn::from_value(v).expect("a row without images must deserialize");
        assert!(turn.images.is_empty());
        assert_eq!(turn.status, "");
    }

    #[test]
    fn a_staged_image_keeps_its_name_mime_and_path_but_no_data() {
        let staged = sample().images[0].staged("/tmp/u/shot-0.png".into());
        assert_eq!(staged.data, None);
        assert_eq!(staged.path.as_deref(), Some("/tmp/u/shot-0.png"));
        let v = staged.clone().into_value();
        let back = TurnImage::from_value(v).expect("a staged image round-trips");
        assert_eq!(back, staged);
    }

    #[test]
    fn a_queued_turn_without_images_reads_an_empty_name_list() {
        let full = QueuedTurn {
            id: RecordId::new(AGENT_TURN_TABLE, "q"),
            text: "hi".into(),
            status: "held".into(),
            tech: None,
            image_names: vec!["a.png".into()],
            created_at: None,
        };
        let mut v = full.into_value();
        if let Value::Object(obj) = &mut v {
            obj.remove("image_names");
            obj.remove("status");
        }
        let turn = QueuedTurn::from_value(v).expect("a bare queued turn deserializes");
        assert!(turn.image_names.is_empty() && !turn.is_held());
    }

    #[test]
    fn empty_messages_and_titles_are_refused_before_writing() {
        let image = [TurnImage::default()];
        assert_eq!(refusal("start", "  ", &[]), Some("empty message"));
        assert_eq!(refusal("steer", "", &[]), Some("empty message"));
        assert_eq!(
            refusal("start", "", &image),
            None,
            "a picture alone is a message"
        );
        assert_eq!(refusal("rename", " ", &[]), Some("empty title"));
        assert_eq!(
            refusal("queue", "", &[]),
            None,
            "an empty queue turn resumes the queue"
        );
        assert_eq!(refusal("interrupt", "", &[]), None);
    }

    #[test]
    fn upload_names_are_path_safe_and_follow_the_bytes() {
        let a = upload_name("my shot (1).PNG", b"abc");
        assert!(a.starts_with("my_shot__1_-") && a.ends_with(".png"), "{a}");
        assert_eq!(a.len(), "my_shot__1_-".len() + 8 + ".png".len());
        assert_ne!(a, upload_name("my shot (1).PNG", b"abd"));
        assert_eq!(
            upload_name("noext", b"x"),
            format!("noext-{:08x}", fnv1a(b"x") as u32)
        );
        assert_eq!(upload_name("../../etc/passwd", b"x").find('/'), None);
    }
}
