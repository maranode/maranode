CREATE TABLE IF NOT EXISTS workspaces (
    id              TEXT PRIMARY KEY,
    slug            TEXT NOT NULL UNIQUE,
    name            TEXT NOT NULL,
    api_key_hash    TEXT,
    model_allowlist TEXT NOT NULL DEFAULT '',
    rate_limit_rpm  INTEGER,
    system_prompt   TEXT,
    created_at      TEXT NOT NULL
);

INSERT OR IGNORE INTO workspaces (id, slug, name, api_key_hash, model_allowlist, rate_limit_rpm, system_prompt, created_at)
VALUES (
    lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-4' ||
    substr(lower(hex(randomblob(2))),2) || '-' ||
    substr('89ab', abs(random()) % 4 + 1, 1) ||
    substr(lower(hex(randomblob(2))),2) || '-' ||
    lower(hex(randomblob(6))),
    'default',
    'Default',
    NULL,
    '',
    NULL,
    NULL,
    datetime('now')
);
