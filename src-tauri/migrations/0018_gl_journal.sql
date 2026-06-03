-- Migration 0018: GL journal (registru jurnal + note contabile).
--
-- Implements double-entry general ledger auto-posting engine per OMFP 1802/2014
-- and ANAF D406 SAF-T guide.  The unique index on (company_id, source_type,
-- source_id) makes posting idempotent: re-posting a document deletes+replaces
-- its journal entry, never duplicates.

CREATE TABLE IF NOT EXISTS gl_journal (
  id               TEXT PRIMARY KEY NOT NULL,
  company_id       TEXT NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
  journal_id       TEXT NOT NULL,          -- VANZARI / CUMPARARI / BANCA / DIVERSE
  journal_type     TEXT NOT NULL,
  transaction_id   TEXT NOT NULL,          -- nota contabilă number (source full_number or id)
  transaction_date TEXT NOT NULL,          -- document date YYYY-MM-DD
  description      TEXT,
  source_type      TEXT NOT NULL,          -- 'INVOICE' | 'RECEIVED_INVOICE' | 'PAYMENT'
  source_id        TEXT NOT NULL,
  customer_id      TEXT,
  supplier_id      TEXT,
  created_at       INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS gl_entry (
  id             TEXT PRIMARY KEY NOT NULL,
  journal_pk     TEXT NOT NULL REFERENCES gl_journal(id) ON DELETE CASCADE,
  record_id      INTEGER NOT NULL,         -- line number within the note
  account_code   TEXT NOT NULL,            -- references chart_of_accounts.account_code by value
  debit          TEXT NOT NULL DEFAULT '0.00',   -- Decimal-as-TEXT RON; exactly one of debit/credit nonzero
  credit         TEXT NOT NULL DEFAULT '0.00',
  partner_cui    TEXT,
  customer_id    TEXT,
  supplier_id    TEXT,
  tax_type       TEXT,                      -- '300' for VAT lines, '000' otherwise
  tax_code       TEXT,                      -- VAT category code on base line, else '000000'
  tax_percentage TEXT,
  tax_base       TEXT,                      -- ON THE BASE (net) LINE ONLY
  tax_amount     TEXT                       -- ON THE BASE LINE ONLY (gross & VAT lines: 0)
);

-- Idempotent posting: upsert deletes old journal+entries (cascade) then inserts fresh ones.
CREATE UNIQUE INDEX IF NOT EXISTS idx_gl_journal_source
    ON gl_journal(company_id, source_type, source_id);

CREATE INDEX IF NOT EXISTS idx_gl_journal_company_date
    ON gl_journal(company_id, transaction_date);
