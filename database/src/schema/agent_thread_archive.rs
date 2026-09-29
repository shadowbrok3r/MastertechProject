//! Agent sessions a user archived out of their own session list; other users still list them.

use super::RecordId;
use crate::db;

pub const AGENT_THREAD_ARCHIVE_TABLE: &str = "agent_thread_archive";

/// Archives every thread in `$threads` for the signed-in user; one already archived stays as it is.
pub const ARCHIVE_SQL: &str = "FOR $t IN $threads { \
     INSERT IGNORE INTO agent_thread_archive { id: [$auth.id, $t], thread: $t }; };";

/// Returns every thread in `$threads` to the signed-in user's session list.
pub const UNARCHIVE_SQL: &str = "FOR $t IN $threads { \
     DELETE type::record('agent_thread_archive', [$auth.id, $t]); };";

/// The threads the signed-in user archived.
pub const ARCHIVED_SQL: &str = "SELECT VALUE thread FROM agent_thread_archive WHERE user = $auth.id;";

/// Archives `threads` for the signed-in user.
pub async fn archive(threads: &[RecordId]) -> anyhow::Result<()> {
    db().query(ARCHIVE_SQL).bind(("threads", threads.to_vec())).await?.check()?;
    Ok(())
}

/// Returns `threads` to the signed-in user's session list.
pub async fn unarchive(threads: &[RecordId]) -> anyhow::Result<()> {
    db().query(UNARCHIVE_SQL).bind(("threads", threads.to_vec())).await?.check()?;
    Ok(())
}

/// The threads the signed-in user archived.
pub async fn archived_by_signed_in_user() -> anyhow::Result<Vec<RecordId>> {
    let mut res = db().query(ARCHIVED_SQL).await?;
    Ok(res.take(0)?)
}
