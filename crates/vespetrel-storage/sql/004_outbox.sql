-- Durable outbox for queued and scheduled sends.
--
-- A message is committed here before any SMTP attempt, so a crash mid-send loses the intent
-- rather than the message. `attempts` and `last_error` drive the retry backoff; the
-- `attempts < MAX` predicate in `outbox.rs` is what stops an exhausted message from being
-- retried forever.

CREATE TABLE IF NOT EXISTS outbox (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    composed_message TEXT NOT NULL,
    scheduled_at INTEGER NOT NULL,
    send_at INTEGER NOT NULL,
    is_cancelled INTEGER NOT NULL DEFAULT 0,
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT
);

CREATE INDEX IF NOT EXISTS idx_outbox_send ON outbox(send_at, is_cancelled);
CREATE INDEX IF NOT EXISTS idx_outbox_account ON outbox(account_id);