-- Account sending aliases / identities
CREATE TABLE IF NOT EXISTS account_aliases (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    email TEXT NOT NULL,
    name TEXT,
    reply_to TEXT,
    signature_id TEXT REFERENCES signatures(id) ON DELETE SET NULL,
    is_default INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_account_aliases_account ON account_aliases(account_id);
