/// A note can be either a DB-only note or one that is synchronised
/// with Prestashop.  We encode that *at the type level*.
pub enum NoteKind {
    DatabaseOnly,
    Prestashop {
        service_number: String,
        /// `None` while we are creating the first message/thread.
        thread_id:     Option<String>,
        /// `None` before Prestashop replies (`POST /customer_messages`).
        message_id:    Option<String>,
        private:       bool,
    },
}

/// Anything the app considers a *person*.
#[derive(Clone)]
pub struct Author {
    pub username: String,
    pub user_id:  RecordId,
    pub employee_id: String, // Prestashop employee id – always required
}

// ---------- phantom flags ---------------------------------------------------
mod flag { pub struct Yes; pub struct No; }

// ---------- main builder ----------------------------------------------------
pub struct TaskNoteBuilder<
    HasAuthor = flag::No,
    HasText   = flag::No,
    HasKind   = flag::No,
> {
    // ► always present fields
    created_at:  Datetime,
    id:          Option<RecordId>,        // generated in `finish_async`

    // ► optional until we know what we build
    task_id:       Option<RecordId>,
    note:          Option<String>,
    author:        Option<Author>,
    kind:          Option<NoteKind>,
}

// ---------- entry point -----------------------------------------------------
impl TaskNoteBuilder<flag::No, flag::No, flag::No> {
    pub fn new() -> Self {
        Self {
            created_at: Utc::now().into(),
            id: None,
            task_id: None,
            note: None,
            author: None,
            kind: None,
        }
    }
}

// ---------- fluent setters --------------------------------------------------
impl<A, T, K> TaskNoteBuilder<A, T, K> {
    pub fn task_id(mut self, id: RecordId) -> Self {
        self.task_id = Some(id); self
    }
}

impl<T, K> TaskNoteBuilder<flag::No, T, K> {
    pub fn author(self, author: Author) -> TaskNoteBuilder<flag::Yes, T, K> {
        TaskNoteBuilder { author: Some(author), ..self }
    }
}
impl<A, K> TaskNoteBuilder<A, flag::No, K> {
    pub fn text(self, txt: impl Into<String>) -> TaskNoteBuilder<A, flag::Yes, K> {
        TaskNoteBuilder { note: Some(txt.into()), ..self }
    }
}
impl<A, T> TaskNoteBuilder<A, T, flag::No> {
    pub fn db_only(self) -> TaskNoteBuilder<A, T, flag::Yes> {
        TaskNoteBuilder { kind: Some(NoteKind::DatabaseOnly), ..self }
    }

    pub fn prestashop(
        self,
        service_number: impl Into<String>,
        private: bool,
    ) -> TaskNoteBuilder<A, T, flag::Yes> {
        TaskNoteBuilder {
            kind: Some(NoteKind::Prestashop {
                service_number: service_number.into(),
                thread_id: None,
                message_id: None,
                private,
            }),
            ..self
        }
    }
}

impl TaskNoteBuilder<flag::Yes, flag::Yes, flag::Yes> {
    /// Consumes the builder, fulfils all cross-field obligations,
    /// talks to Prestashop if necessary, and returns the ready payload.
    pub async fn finish_async(mut self) -> anyhow::Result<TaskNotePayload> {
        // --- 1. generate / reconcile `id` -----------------------------------
        self.id = match &self.kind {
            Some(NoteKind::Prestashop { message_id, .. }) if message_id.is_some() => {
                Some(RecordId::from((TASK_NOTE_TABLE, message_id.clone().unwrap())))
            }
            _ => Some(RecordId::from((TASK_NOTE_TABLE, nanoid::nanoid!()))),
        };

        // --- 2. ensure legal combinations -----------------------------------
        if matches!(self.kind, Some(NoteKind::DatabaseOnly))
            && self.task_id.is_none()
        {
            anyhow::bail!("A database-only note must belong to a task");
        }

        // --- 3. perform side effects ----------------------------------------
        match &mut self.kind {
            Some(NoteKind::Prestashop { service_number, thread_id, message_id, private }) => {
                // 3-a. ensure we have a thread
                if thread_id.is_none() && !private {
                    *thread_id = Some(
                        self.create_or_fetch_thread(service_number).await?
                    );
                }

                // 3-b. create customer message when there is a thread but no message
                if message_id.is_none() && !private {
                    *message_id = Some(
                        self.create_customer_message(
                            thread_id.clone().unwrap(),
                            self.author.as_ref().unwrap()
                        ).await?
                    );
                }
            }
            _ => { /* nothing to do */ }
        }

        // --- 4. assemble final payload --------------------------------------
        let author = self.author.unwrap();
        let txt    = self.note.unwrap();

        Ok(TaskNotePayload {
            id:                self.id.unwrap(),
            task_id:           self.task_id,
            created_at:        self.created_at,
            note:              txt,
            username:          author.username,
            id_customer_thread: match &self.kind {
                Some(NoteKind::Prestashop { thread_id, .. }) => thread_id.clone(),
                _ => None,
            },
            id_customer_message: match &self.kind {
                Some(NoteKind::Prestashop { message_id, .. }) => message_id.clone(),
                _ => None,
            },
            id_employee:       Some(author.employee_id),
            user:              author.user_id,
            service_number:    match &self.kind {
                Some(NoteKind::Prestashop { service_number, .. }) => Some(service_number.clone()),
                _ => None,
            },
            private:           match &self.kind {
                Some(NoteKind::Prestashop { private, .. }) => *private,
                _ => false,
            },
        })
    }

    // ───────── helper calls to your existing async routines ────────────────
    async fn create_or_fetch_thread(&self, service: &str) -> anyhow::Result<String> {
        // delegate to your existing logic
        Prestashop::default()
            .find_or_create_thread(service)
            .await
    }

    async fn create_customer_message(
        &self,
        thread_id: String,
        author: &Author,
    ) -> anyhow::Result<String> {
        Prestashop::default()
            .create_customer_message(&author.employee_id, &thread_id, self.note.as_ref().unwrap())
            .await
            .map(|resp| resp.id)
    }
}