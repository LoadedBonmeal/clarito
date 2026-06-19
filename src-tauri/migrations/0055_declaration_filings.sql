-- Istoricul depunerilor de declarații: un rând per export reușit.
-- `kind`        — tipul declarației: 'D300','D390','D394','D112','D205','D207','SAFT','BILANT'
-- `period`      — 'YYYY-MM' pentru lunar, 'YYYY' pentru anual
-- `anaf_status` — starea curentă: EXPORTED | SUBMITTED | ACCEPTED | REJECTED
CREATE TABLE declaration_filings (
    id               TEXT PRIMARY KEY NOT NULL,
    company_id       TEXT NOT NULL,
    kind             TEXT NOT NULL,
    period           TEXT NOT NULL,
    is_rectificative INTEGER NOT NULL DEFAULT 0,
    file_path        TEXT,
    anaf_status      TEXT NOT NULL DEFAULT 'EXPORTED',
    filed_at         INTEGER NOT NULL
);

CREATE INDEX idx_decl_filings_company ON declaration_filings(company_id, kind, period);
