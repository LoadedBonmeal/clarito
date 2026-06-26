-- Audit hardening: gl_entry carried only an index on currency_code (0086). The hot read paths scan
-- the whole table on large ledgers:
--   • account_code — trial balance, fișă cont, the 4428 cash-VAT split, VAT reconciliation.
--   • journal_pk   — the join back to gl_journal AND the ON DELETE CASCADE child lookup (SQLite needs
--                    an index on the child FK column or every parent delete scans gl_entry).
-- Both are pure additive indexes (no data change); safe + idempotent.

CREATE INDEX IF NOT EXISTS idx_gl_entry_account ON gl_entry(account_code);
CREATE INDEX IF NOT EXISTS idx_gl_entry_journal ON gl_entry(journal_pk);
