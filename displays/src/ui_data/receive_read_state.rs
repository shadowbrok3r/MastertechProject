use crate::app_state::SharedContext;

impl SharedContext {
    /// Drain any pending `task_note_read` rows fetched from SurrealDB and
    /// merge them into the in-memory `last_read_notes` cache.
    ///
    /// The cache may already contain a more recent timestamp from this
    /// session (the user just opened the modal, but the DB row hasn't
    /// roundtripped yet) — we keep the larger timestamp so we never appear
    /// to "lose" a freshly read state.
    pub fn receive_read_state(&mut self) {
        while let Ok(rows) = self.read_state_rx.try_recv() {
            for row in rows {
                let incoming: chrono::DateTime<chrono::Utc> = row.read_at.into();
                self.last_read_notes
                    .entry(row.task)
                    .and_modify(|existing| {
                        if incoming > *existing {
                            *existing = incoming;
                        }
                    })
                    .or_insert(incoming);
            }
        }
    }
}
