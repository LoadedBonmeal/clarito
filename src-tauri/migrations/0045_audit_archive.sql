-- ROB-24: archive audit_log rows before the >2-year purge instead of hard-deleting them.
-- The audit trail (who did what, when) is retention-sensitive for a fiscal app; the old
-- cleanup DELETE'd rows older than 2 years outright, losing them. cleanup_audit_log now
-- copies expiring rows here first, then deletes from the live table. Same columns as
-- audit_log + the moment the row was moved.
CREATE TABLE IF NOT EXISTS audit_archive (
    id           TEXT PRIMARY KEY,

    action       TEXT NOT NULL,
    entity_type  TEXT NOT NULL,
    entity_id    TEXT NOT NULL,
    metadata     TEXT,

    created_at   INTEGER NOT NULL,
    archived_at  INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_audit_archive_created ON audit_archive(created_at);
