-- RUST-06: Prevent duplicate SPV notifications on concurrent sync.
--
-- The previous implementation used a check-then-act pattern:
--     SELECT COUNT(*) FROM notifications WHERE data = ?
--     -- (race window)
--     INSERT INTO notifications (...)
-- Two SPV sync workers racing could each see count = 0 and both insert,
-- producing duplicate notifications for the same ANAF message.
--
-- A partial UNIQUE index on `data` makes "INSERT OR IGNORE" atomic for
-- the SPV notification keys (format: "spv_msg_<id>"). Pre-existing rows
-- with NULL or empty data are not affected, so legacy notifications
-- (where data is unused) continue to work unchanged.
--
-- NOTE: if an existing install already has duplicate rows with the same
-- non-empty `data`, this index creation will fail. Operators should
-- de-duplicate first; the SPV worker only writes `data = "spv_msg_*"`
-- so collisions in practice are limited to a narrow window between
-- the previous race condition and this migration.

CREATE UNIQUE INDEX IF NOT EXISTS idx_notifications_data_unique
ON notifications(data)
WHERE data IS NOT NULL AND data != '';
