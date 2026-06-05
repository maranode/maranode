PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS rag_collections (
    name            TEXT PRIMARY KEY,
    embedding_model TEXT NOT NULL,
    dim             INTEGER NOT NULL,
    created_at      TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS rag_documents (
    id          TEXT PRIMARY KEY,
    collection  TEXT NOT NULL,
    source      TEXT NOT NULL,
    sha256      TEXT NOT NULL,
    ingested_at TEXT NOT NULL,
    title       TEXT,
    author      TEXT,
    page_count  INTEGER NOT NULL DEFAULT 0,
    summary     TEXT,
    FOREIGN KEY (collection) REFERENCES rag_collections(name) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS rag_chunks (
    id          TEXT PRIMARY KEY,
    document_id TEXT NOT NULL,
    collection  TEXT NOT NULL,
    ordinal     INTEGER NOT NULL,
    text        TEXT NOT NULL,
    embedding   BLOB NOT NULL,
    page_number INTEGER NOT NULL DEFAULT 0,
    section     TEXT,
    FOREIGN KEY (document_id) REFERENCES rag_documents(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_rag_chunks_collection  ON rag_chunks(collection);
CREATE INDEX IF NOT EXISTS idx_rag_documents_collection ON rag_documents(collection);
