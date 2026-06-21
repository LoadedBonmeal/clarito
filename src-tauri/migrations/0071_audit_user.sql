-- P2 Wave 8: add user attribution to audit_log.
-- Nullable — existing rows keep NULL (stays valid; archive job untouched).
-- Do NOT add a BEFORE DELETE trigger: it would break the background archive job
-- which DELETEs rows from audit_log after copying them to audit_archive.
ALTER TABLE audit_log ADD COLUMN user_id    TEXT;
ALTER TABLE audit_log ADD COLUMN user_label TEXT;
