-- P2 Wave 8: Multi-user RBAC — users table.
-- Brick-safety: nullable columns; no DROP; all new tables use CREATE IF NOT EXISTS.
-- Roles: admin, contabil, operator, viewer.
CREATE TABLE IF NOT EXISTS users (
    id            TEXT    PRIMARY KEY,
    username      TEXT    NOT NULL UNIQUE,
    password_hash TEXT    NOT NULL,
    role          TEXT    NOT NULL CHECK(role IN ('admin','contabil','operator','viewer')),
    is_active     INTEGER NOT NULL DEFAULT 1,
    failed_attempts INTEGER NOT NULL DEFAULT 0,
    locked_until  INTEGER,         -- unix timestamp; NULL = not locked
    created_at    INTEGER NOT NULL,
    last_login    INTEGER           -- NULL until first successful login
);
