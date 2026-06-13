-- ROB-22: file-integrity fingerprints for the legally-retained invoice artifacts (UBL XML +
-- PDF). A fiscal app must keep submitted invoices intact for years; storing the SHA-256 +
-- byte size at write time lets `verify_invoice_integrity` later detect a MISSING (deleted/
-- moved) or CORRUPTED (edited/truncated) archive file — silent disk rot or tampering that a
-- bare path column cannot reveal. NULL = not yet hashed (legacy rows / write predating this).
ALTER TABLE invoices ADD COLUMN xml_sha256 TEXT;
ALTER TABLE invoices ADD COLUMN xml_size INTEGER;
ALTER TABLE invoices ADD COLUMN pdf_sha256 TEXT;
ALTER TABLE invoices ADD COLUMN pdf_size INTEGER;
