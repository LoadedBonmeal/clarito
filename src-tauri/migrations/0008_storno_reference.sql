-- BIZ-13: introduce a proper FK column for storno references instead of
-- parsing the notes field. The notes-based marker ("STORNO_OF:NUMBER|...")
-- stays as a legacy fallback for rows produced before this migration.

ALTER TABLE invoices ADD COLUMN storno_of_invoice_id TEXT REFERENCES invoices(id);

-- Backfill: for rows whose notes start with "STORNO_OF:", try to recover the
-- referenced invoice id by matching its full_number within the same company.
-- The parser is intentionally simple — anything it cannot recover stays NULL
-- and falls back to the notes parser at runtime.
UPDATE invoices
SET storno_of_invoice_id = (
    SELECT orig.id
    FROM invoices orig
    WHERE orig.company_id = invoices.company_id
      AND orig.full_number = TRIM(
              REPLACE(
                  REPLACE(
                      SUBSTR(
                          invoices.notes,
                          11,
                          CASE
                              WHEN INSTR(SUBSTR(invoices.notes, 11), '|') > 0
                              THEN INSTR(SUBSTR(invoices.notes, 11), '|') - 1
                              ELSE LENGTH(SUBSTR(invoices.notes, 11))
                          END
                      ),
                      CHAR(10), ''
                  ),
                  CHAR(13), ''
              )
          )
    LIMIT 1
)
WHERE invoices.notes LIKE 'STORNO_OF:%';

CREATE INDEX IF NOT EXISTS idx_invoices_storno_of ON invoices(storno_of_invoice_id);
