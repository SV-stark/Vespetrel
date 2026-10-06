use rusqlite::{Connection, params};

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub message_id: String,
    pub subject: Option<String>,
    pub snippet: Option<String>,
    pub rank: f64,
}

/// Escape user input for SQLite FTS5 MATCH queries, preserving field prefixes (from:, to:, subject:, body:)
///
/// Delegates to [`vespetrel_core::search`] so this and `core::virtual_folder` cannot drift: they
/// previously had separate sanitizers, and the `virtual_folder` copy had no prefix support and
/// emitted the FTS5 syntax error `MATCH '"*"'` for an empty query.
///
/// Returns an empty string when nothing searchable remains; `search_messages` treats that as
/// "no results" rather than searching for everything.
pub fn escape_fts5_query(raw: &str) -> String {
    vespetrel_core::search::escape_fts5_query(raw).unwrap_or_default()
}

/// Sub-15ms FTS5 search with BM25 ranking - §3.2
pub fn search_messages(
    conn: &Connection,
    query: &str,
    account_id: Option<&str>,
    limit: usize,
) -> anyhow::Result<Vec<SearchResult>> {
    let sanitized_query = escape_fts5_query(query);
    if sanitized_query.is_empty() {
        return Ok(Vec::new());
    }

    let clamped_limit = limit.clamp(1, 200) as i64;

    let sql = r#"
        SELECT message_id, subject, snippet(messages_fts, 6, '<b>', '</b>', '...', 20) as snippet,
               bm25(messages_fts, 1.0, 1.0, 10.0, 5.0, 5.0, 3.0, 1.0) as rank
        FROM messages_fts
        WHERE messages_fts MATCH ?1 AND (?2 IS NULL OR account_id = ?2)
        ORDER BY rank
        LIMIT ?3
    "#;

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![sanitized_query, account_id, clamped_limit], |row| {
        Ok(SearchResult {
            message_id: row.get(0)?,
            subject: row.get(1)?,
            snippet: row.get(2)?,
            rank: row.get(3)?,
        })
    })?;

    let mut results = Vec::new();
    for r in rows {
        results.push(r?);
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_fts5_query() {
        assert_eq!(escape_fts5_query("hello world"), "\"hello\" \"world\"");
        // `=` is not a permitted term character, so the injection attempt collapses to "11".
        assert_eq!(
            escape_fts5_query("user@example.com OR 1=1"),
            "\"user@example.com\" \"OR\" \"11\""
        );
        assert_eq!(
            escape_fts5_query("\"test\" OR C++"),
            "\"\"\"test\"\"\" \"OR\" \"C++\""
        );
        assert_eq!(
            escape_fts5_query("from:alice@example.com"),
            "from_address:\"alice@example.com\""
        );
        assert_eq!(escape_fts5_query("   "), "");
        // A query with no searchable term must not become a match-all wildcard.
        assert_eq!(escape_fts5_query("resume*"), "\"resume\"");
        assert_eq!(vespetrel_core::search::escape_fts5_query("   "), None);
    }

    #[test]
    fn test_fts5_accent_folding_search() {
        let conn = crate::db::open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO messages_fts(message_id, account_id, subject, from_address, from_name, to_addresses, body_content)
             VALUES ('msg-1', 'acct-1', 'Meeting at the Café', 'boss@example.com', 'Boss', 'me@example.com', 'Let us discuss the résumé at the café')",
            [],
        ).unwrap();

        // Search with unaccented "cafe" should match "Café"
        let results = search_messages(&conn, "cafe", None, 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].message_id, "msg-1");

        // Search with unaccented "resume" should match "résumé"
        let results_resume = search_messages(&conn, "resume", None, 10).unwrap();
        assert_eq!(results_resume.len(), 1);
    }

    #[test]
    fn test_insert_message_populates_fts5_via_trigger() {
        let conn = crate::db::open_in_memory().unwrap();
        let acct = vespetrel_core::Account::new(
            "Test",
            "user@example.com",
            vespetrel_core::account::ProviderType::Imap,
        );
        crate::repo::upsert_account(&conn, &acct).unwrap();
        let folder = vespetrel_core::Folder::new(&acct.id, "inbox", "Inbox", "INBOX");
        crate::repo::upsert_folder(&conn, &folder).unwrap();

        let mut msg = vespetrel_core::Message::new(
            &acct.id,
            &folder.id,
            101,
            "Quarterly Financial Résumé",
            "cfo@example.com",
            vec!["user@example.com".into()],
        );
        msg.body_snippet = Some("Attached is the financial statement for the Café project.".into());
        crate::repo::insert_message(&conn, &msg).unwrap();

        // Search via FTS5 query to verify trigger worked
        let matches = search_messages(&conn, "resume", None, 10).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].message_id, msg.id);

        let cafe_matches = search_messages(&conn, "cafe", None, 10).unwrap();
        assert_eq!(cafe_matches.len(), 1);
        assert_eq!(cafe_matches[0].message_id, msg.id);
    }
}
