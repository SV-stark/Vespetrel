-- Rebuild FTS5 with the accent-folding unicode61 tokenizer.
--
-- `remove_diacritics 2` folds "Café" and "cafe" to the same term, so a user typing an
-- unaccented word finds accented mail. The default `remove_diacritics 1` did not handle the
-- full range, and the index is rebuilt from `messages` rather than incrementally because the
-- tokenizer change invalidates every existing posting.

DROP TABLE IF EXISTS messages_fts;

CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
    message_id UNINDEXED,
    account_id UNINDEXED,
    subject,
    from_address,
    from_name,
    to_addresses,
    body_content,
    tokenize = 'unicode61 remove_diacritics 2'
);

-- Re-populate FTS5 index from messages table
INSERT INTO messages_fts(message_id, account_id, subject, from_address, from_name, to_addresses, body_content)
SELECT id, account_id, subject, from_address, from_name, to_addresses, COALESCE(body_text_preview, body_snippet, '')
FROM messages;

-- Re-create sync triggers on messages table
DROP TRIGGER IF EXISTS messages_ai;
DROP TRIGGER IF EXISTS messages_ad;
DROP TRIGGER IF EXISTS messages_au;

CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts(message_id, account_id, subject, from_address, from_name, to_addresses, body_content)
    VALUES (new.id, new.account_id, new.subject, new.from_address, new.from_name, new.to_addresses, COALESCE(new.body_text_preview, new.body_snippet, ''));
END;

CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages BEGIN
    DELETE FROM messages_fts WHERE message_id = old.id;
END;

-- NOTE: this recreation omits the `account_id` arm, which version 5 restores. The omission
-- was the original bug that migration exists to fix.
CREATE TRIGGER IF NOT EXISTS messages_au AFTER UPDATE ON messages
WHEN old.subject IS NOT new.subject
  OR old.account_id IS NOT new.account_id
  OR old.from_address IS NOT new.from_address
  OR old.from_name IS NOT new.from_name
  OR old.to_addresses IS NOT new.to_addresses
  OR old.body_text_preview IS NOT new.body_text_preview
  OR old.body_snippet IS NOT new.body_snippet
BEGIN
    DELETE FROM messages_fts WHERE message_id = old.id;
    INSERT INTO messages_fts(message_id, account_id, subject, from_address, from_name, to_addresses, body_content)
    VALUES (new.id, new.account_id, new.subject, new.from_address, new.from_name, new.to_addresses, COALESCE(new.body_text_preview, new.body_snippet, ''));
END;