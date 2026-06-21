-- 0066: Bon de transfer inter-gestiune (formular 14-3-3A, OMFP 2634/2015)
--
-- Un transfer mută stocul aceluiași produs dintr-o gestiune sursă (A) în
-- gestiunea destinație (B) la costul evaluat de motorul FIFO/LIFO/CMP al lui A.
--
-- Invariant fiscal (art. 20 alin. 4, OMFP 2634/2015 + OMFP 1802/2014 pct. 96):
--   • Transferul NU generează notă contabilă sintetică (contul 371 rămâne identic
--     la nivel de societate — mișcarea este pur analitică, pe gestiune).
--   • Costul din A se păstrează integral în B (nicio reevaluare, niciun câștig /
--     pierdere din transfer).
--   • Stocul total al produsului (Σ gestiuni) rămâne nemodificat după transfer.
--
-- Rândurile stock_ledger generate de un transfer au doc_type='TRANSFER'; funcția
-- post_stock_movement din gl.rs IGNORĂ aceste rânduri (nu postează nimic în
-- gl_journal), păstrând neutralitatea GL.

CREATE TABLE IF NOT EXISTS stock_transfers (
    id                TEXT    NOT NULL PRIMARY KEY,
    company_id        TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    product_id        TEXT    NOT NULL REFERENCES products(id),
    from_gestiune_id  TEXT    NOT NULL REFERENCES gestiune(id),
    to_gestiune_id    TEXT    NOT NULL REFERENCES gestiune(id),
    transfer_date     TEXT    NOT NULL,      -- ISO YYYY-MM-DD
    qty               TEXT    NOT NULL,      -- cantitate transferată (6 zec.)
    unit_cost         TEXT    NOT NULL,      -- cost unitar evaluat la ieșire din A (2 zec.)
    value             TEXT    NOT NULL,      -- qty × unit_cost (2 zec.)
    transfer_ref      TEXT,                  -- referință externă opțională (bon nr., etc.)
    notes             TEXT,
    created_at        INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_stock_transfers_company_date
    ON stock_transfers(company_id, transfer_date);
