-- ============================================================
-- TABLE: ai_task_item
-- Checklist rows for an ai_task. Canonical mutable check state;
-- each item is mirrored write-once into diagnostic_entry.
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================

DEFINE TABLE ai_task_item TYPE ANY SCHEMALESS PERMISSIONS FULL;

DEFINE FIELD ai_task_ref ON ai_task_item TYPE record<ai_task>                 PERMISSIONS FULL;
DEFINE FIELD text        ON ai_task_item TYPE string                          PERMISSIONS FULL;
DEFINE FIELD position    ON ai_task_item TYPE int                             PERMISSIONS FULL;
DEFINE FIELD checked     ON ai_task_item TYPE bool DEFAULT false              PERMISSIONS FULL;
DEFINE FIELD checked_by  ON ai_task_item TYPE none | record<user>             PERMISSIONS FULL;
DEFINE FIELD checked_at  ON ai_task_item TYPE none | datetime                 PERMISSIONS FULL;
DEFINE FIELD entry_ref   ON ai_task_item TYPE none | record<diagnostic_entry> PERMISSIONS FULL;
DEFINE FIELD created_at  ON ai_task_item TYPE datetime DEFAULT time::now()    PERMISSIONS FULL;

DEFINE INDEX ai_task_item_idx ON ai_task_item FIELDS ai_task_ref, checked;

-- Last box checked flips the parent to awaiting_followup (race-safe via the status guard).
DEFINE EVENT ai_task_item_checked ON ai_task_item
    WHEN $event = 'UPDATE' AND $before.checked = false AND $value.checked = true
    THEN {
        LET $remaining = array::first((SELECT VALUE count() FROM ai_task_item WHERE ai_task_ref = $value.ai_task_ref AND checked = false GROUP ALL));
        IF $remaining = NONE OR $remaining = 0 {
            UPDATE $value.ai_task_ref SET status = 'awaiting_followup', completed_at = time::now()
                WHERE status = 'open';
        };
    };

-- Unchecking during review reverts the parent to open (grace-window undo).
DEFINE EVENT ai_task_item_unchecked ON ai_task_item
    WHEN $event = 'UPDATE' AND $before.checked = true AND $value.checked = false
    THEN {
        UPDATE $value.ai_task_ref SET status = 'open', completed_at = NONE, review_acknowledged_at = NONE
            WHERE status = 'awaiting_followup';
    };
