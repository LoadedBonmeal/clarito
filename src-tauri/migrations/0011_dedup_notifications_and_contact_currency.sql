-- 0011: repair REG-13 (duplicate notifications.data could break 0010 unique index on
-- installs that pre-existed) and add contacts.currency for EU-client support.

-- Idempotent dedup. The unique index from 0010 (idx_notifications_data_unique) already
-- exists; we DELETE before relying on it again. This file is safe to re-run on any DB.
DELETE FROM notifications
WHERE rowid NOT IN (
    SELECT MIN(rowid) FROM notifications
    WHERE data IS NOT NULL AND data != ''
    GROUP BY data
)
AND data IS NOT NULL AND data != '';

-- Ensure the unique index is in place (no-op if 0010 already applied it).
CREATE UNIQUE INDEX IF NOT EXISTS idx_notifications_data_unique
ON notifications(data)
WHERE data IS NOT NULL AND data != '';

-- Contacts: per-contact currency for EU/non-RON clients (Fix 4 / UX-08).
-- Default NULL; UI falls back to 'RON' when unset, so this is non-breaking.
ALTER TABLE contacts ADD COLUMN currency TEXT;
