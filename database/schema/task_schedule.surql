-- ============================================================
-- TABLE: task_schedule            last changed: ai_assistant_tools
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- Recurring and one-off tasks; each run creates a task. Rust: `database::schema::task_schedule`.
-- Record users schedule within their own store; active Root schedules for anyone.

DEFINE TABLE task_schedule TYPE NORMAL SCHEMAFULL
    PERMISSIONS
        FOR select WHERE $access = 'user' AND (assignee = $auth.id OR created_by = $auth.id OR ($auth.authorization = 'Root' AND $auth.active = true)),
        FOR create WHERE $access = 'user' AND $auth.active = true AND ($auth.authorization = 'Root' OR (created_by = $auth.id AND assignee.store = $auth.store)),
        FOR update WHERE $access = 'user' AND $auth.active = true AND ($auth.authorization = 'Root' OR ((created_by = $auth.id OR assignee = $auth.id) AND assignee.store = $auth.store)),
        FOR delete WHERE $access = 'user' AND (assignee = $auth.id OR created_by = $auth.id OR ($auth.authorization = 'Root' AND $auth.active = true));

DEFINE FIELD title          ON task_schedule TYPE string ASSERT string::len($value) > 0 AND string::len($value) <= 120;
DEFINE FIELD description    ON task_schedule TYPE none | string ASSERT $value == NONE OR string::len($value) <= 1000;
DEFINE FIELD assignee       ON task_schedule TYPE record<user> READONLY;
DEFINE FIELD created_by     ON task_schedule TYPE none | record<user> DEFAULT $auth.id READONLY;

-- once fires at next_run; day = Mon-Sat; week = weekdays (1 Mon .. 7 Sun); month = month_day, clamped.
DEFINE FIELD every          ON task_schedule TYPE string ASSERT $value IN ['once', 'day', 'week', 'month'];
DEFINE FIELD interval       ON task_schedule TYPE int DEFAULT 1 ASSERT $value >= 1 AND $value <= 12;
DEFINE FIELD weekdays       ON task_schedule TYPE array<int> DEFAULT [];
DEFINE FIELD weekdays.*     ON task_schedule TYPE int ASSERT $value >= 1 AND $value <= 7;
DEFINE FIELD month_day      ON task_schedule TYPE none | int ASSERT $value == NONE OR ($value >= 1 AND $value <= 31);
-- Store-local HH:MM (America/Denver).
DEFINE FIELD at             ON task_schedule TYPE string ASSERT $value = /^([01][0-9]|2[0-3]):[0-5][0-9]$/;

DEFINE FIELD next_run       ON task_schedule TYPE none | datetime;
DEFINE FIELD last_run       ON task_schedule TYPE none | datetime;
DEFINE FIELD runs           ON task_schedule TYPE int DEFAULT 0;
DEFINE FIELD active         ON task_schedule TYPE bool DEFAULT true;

-- Hours from a run until its task is due; NONE = store close that day.
DEFINE FIELD due_hours      ON task_schedule TYPE none | int ASSERT $value == NONE OR ($value >= 0 AND $value <= 720);
DEFINE FIELD priority       ON task_schedule TYPE none | string;
DEFINE FIELD service_number ON task_schedule TYPE none | string;
DEFINE FIELD origin         ON task_schedule TYPE none | string ASSERT $value == NONE OR $value IN ['ai', 'tech'];
DEFINE FIELD created_at     ON task_schedule TYPE datetime DEFAULT time::now() READONLY;

DEFINE INDEX task_schedule_due      ON task_schedule FIELDS active, next_run;
DEFINE INDEX task_schedule_assignee ON task_schedule FIELDS assignee;
