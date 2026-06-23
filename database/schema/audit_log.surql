-- ============================================================
-- TABLE: audit_log
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- Read-only audit trail; rows are written out-of-band, never via app sessions.

DEFINE TABLE audit_log TYPE ANY SCHEMALESS
    PERMISSIONS
        FOR select FULL,
        FOR create, update, delete NONE;

-- Spawns an ALERT notification on each failed signin/signup attempt.
DEFINE EVENT failed_attempt ON audit_log WHEN $event = 'CREATE' THEN {
    IF $value.signin_attempt != NONE {
        IF $value.signin_attempt.success == false {
            CREATE notification CONTENT { notification_description: string::concat('Failed signin attempt ', $value.signin_attempt.email), notification_type: 'ALERT', status: 'Unread', user: user:jm9a7l3v32gsiccr7pgw }
        }
    };
    IF $value.signup_attempt != NONE {
        IF $value.signup_attempt != '' {
            CREATE notification CONTENT { notification_description: string::concat('Failed signup attempt ', $value.signup_attempt), notification_type: 'ALERT', status: 'Unread', user: user:jm9a7l3v32gsiccr7pgw }
        }
    };
};
