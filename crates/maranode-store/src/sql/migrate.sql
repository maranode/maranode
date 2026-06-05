PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS models (
    id           TEXT PRIMARY KEY,
    name         TEXT NOT NULL,
    tag          TEXT NOT NULL,
    sha256       TEXT NOT NULL,
    size_bytes   INTEGER NOT NULL,
    format       TEXT NOT NULL DEFAULT 'gguf',
    quantization TEXT,
    blob_path    TEXT NOT NULL,
    imported_at  TEXT NOT NULL,
    model_type   TEXT NOT NULL DEFAULT 'llm',
    UNIQUE(name, tag)
);
