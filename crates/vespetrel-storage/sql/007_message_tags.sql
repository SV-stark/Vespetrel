CREATE TABLE IF NOT EXISTS message_tags (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id TEXT NOT NULL,
    tag TEXT NOT NULL,
    color TEXT NOT NULL DEFAULT '#3b82f6',
    UNIQUE(message_id, tag)
);

CREATE INDEX IF NOT EXISTS idx_message_tags_message ON message_tags(message_id);