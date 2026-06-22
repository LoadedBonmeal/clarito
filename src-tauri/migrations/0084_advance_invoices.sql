-- Migration 0084: FACTURĂ DE AVANS (advance invoice) support.
--
-- Art. 282 Cod Fiscal: TVA devine exigibilă la data încasării avansului (pentru
-- emitent) sau la data plății (pentru beneficiar). Contul specific pentru avansuri
-- este 419 „Clienți-creditori" (avansuri încasate de la clienți) și
-- 4091 „Furnizori-debitori" (avansuri plătite furnizorilor).
--
-- Schema changes:
--   1. invoices.invoice_kind TEXT — 'standard' (default) | 'advance'
--      An 'advance' issued invoice posts D 4111 = C 419 + C 4427 (not 707).
--
--   2. received_invoices.is_advance INTEGER — 0 (default) | 1
--      An advance received invoice posts D 4091 + D 4426 = C 401 (not 607).
--
--   3. advance_invoice_settlements — links a FINAL invoice to one or more
--      advance invoices it settles, carrying the advance's original VAT rate
--      (art. 282: storno uses the advance's OWN rate, not the delivery rate).
--      The settlement record drives the storno GL entries at finalization.

-- 1. Add invoice_kind to issued invoices.
ALTER TABLE invoices ADD COLUMN invoice_kind TEXT NOT NULL DEFAULT 'standard';

-- 2. Add is_advance to received invoices.
ALTER TABLE received_invoices ADD COLUMN is_advance INTEGER NOT NULL DEFAULT 0;

-- 3. Settlement table: links final invoice → advance invoice(s).
CREATE TABLE IF NOT EXISTS advance_invoice_settlements (
    id              TEXT PRIMARY KEY,
    company_id      TEXT NOT NULL REFERENCES companies(id) ON DELETE CASCADE,

    -- The FINAL invoice that settles one or more advances.
    final_invoice_id TEXT NOT NULL REFERENCES invoices(id) ON DELETE CASCADE,

    -- The ADVANCE invoice being settled (issued advance → 419 storned).
    advance_invoice_id TEXT NOT NULL REFERENCES invoices(id) ON DELETE CASCADE,

    -- Advance BASE amount (net) and VAT at the ADVANCE's own rate (TEXT, Decimal).
    -- Storno always uses these values and the advance_vat_rate, regardless of the
    -- delivery rate (art. 282: "la rata în vigoare la data avansului").
    advance_base    TEXT NOT NULL,
    advance_vat     TEXT NOT NULL,
    advance_vat_rate TEXT NOT NULL,   -- e.g. "21.00" or "19.00" (historic)

    created_at      INTEGER NOT NULL DEFAULT (unixepoch()),

    UNIQUE(final_invoice_id, advance_invoice_id)
);

CREATE TABLE IF NOT EXISTS advance_received_settlements (
    id              TEXT PRIMARY KEY,
    company_id      TEXT NOT NULL REFERENCES companies(id) ON DELETE CASCADE,

    -- The FINAL received invoice that settles one or more advance received invoices.
    final_received_id TEXT NOT NULL REFERENCES received_invoices(id) ON DELETE CASCADE,

    -- The ADVANCE received invoice being settled (4091 storned).
    advance_received_id TEXT NOT NULL REFERENCES received_invoices(id) ON DELETE CASCADE,

    -- Advance BASE amount and VAT at the ADVANCE's own rate.
    advance_base    TEXT NOT NULL,
    advance_vat     TEXT NOT NULL,
    advance_vat_rate TEXT NOT NULL,

    created_at      INTEGER NOT NULL DEFAULT (unixepoch()),

    UNIQUE(final_received_id, advance_received_id)
);

CREATE INDEX IF NOT EXISTS idx_adv_settlements_final
    ON advance_invoice_settlements(company_id, final_invoice_id);
CREATE INDEX IF NOT EXISTS idx_adv_settlements_advance
    ON advance_invoice_settlements(company_id, advance_invoice_id);

CREATE INDEX IF NOT EXISTS idx_adv_recv_settle_final
    ON advance_received_settlements(company_id, final_received_id);
CREATE INDEX IF NOT EXISTS idx_adv_recv_settle_advance
    ON advance_received_settlements(company_id, advance_received_id);
