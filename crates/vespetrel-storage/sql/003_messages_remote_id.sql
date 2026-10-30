-- Stable remote identifier for a message, alongside `remote_uid`.
--
-- `remote_uid` is only unique within a folder; `remote_id` survives a move between folders, so
-- it is what dedup and cross-folder reconciliation key on.
--
-- The `ALTER TABLE` itself lives in the migration's guard: SQLite has no
-- `ADD COLUMN IF NOT EXISTS`, so introspecting first is what keeps the step idempotent across
-- databases created by different code paths. See `ensure_remote_id_column`.

CREATE INDEX IF NOT EXISTS idx_messages_remote_id ON messages(folder_id, remote_id);