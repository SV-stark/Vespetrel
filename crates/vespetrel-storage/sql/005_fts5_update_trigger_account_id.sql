-- Restore the `account_id` arm of the messages_au trigger.
--
-- Version 2 recreated that trigger and dropped `OR old.account_id IS NOT new.account_id`.
-- `account_id` is an UNINDEXED column of messages_fts and is the tenancy filter in
-- `search_messages`, so without this arm a message moved between accounts keeps its old
-- account tag: invisible to its new owner and still returned to the old one.
--
-- `body_snippet` is also in the WHEN clause because the body indexed into FTS is
-- `COALESCE(body_text_preview, body_snippet, '')`: when a message has only a snippet, a
-- snippet-only update must re-index.

DROP TRIGGER IF EXISTS messages_au;

CREATE TRIGGER messages_au AFTER UPDATE ON messages
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