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
    let filters = vespetrel_core::SearchFilters::parse_from_str(query);
    // Build FTS text query including from/to operators.
    let mut fts_parts = Vec::new();
    if !filters.text.trim().is_empty() {
        fts_parts.push(filters.text.clone());
    }
    if let Some(f) = &filters.from {
        fts_parts.push(format!("from:{}", f));
    }
    if let Some(t) = &filters.to {
        fts_parts.push(format!("to:{}", t));
    }
    let fts_query = fts_parts.join(" ");
    let sanitized_query = vespetrel_core::search::escape_fts5_query(&fts_query);
    // If no searchable terms and no operators that map to FTS, fall back to SQL-only filters.
    let has_fts = sanitized_query.is_some();

    let clamped_limit = limit.clamp(1, 200) as i64;

    // Build dynamic WHERE clause for SQL filters.
    let mut conditions = Vec::new();
    if has_fts {
        conditions.push("messages_fts MATCH ?1".to_string());
    }
    if let Some(acc) = account_id {
        conditions.push(format!("account_id = '{}'", acc.replace('\'', "''")));
    }
    if filters.has_attachment {
        conditions
            .push("message_id IN (SELECT id FROM messages WHERE has_attachments = 1)".to_string());
    }
    if let Some(before) = filters.before {
        conditions.push(format!(
            "message_id IN (SELECT id FROM messages WHERE received_at < {})",
            before
        ));
    }
    if let Some(unread) = filters.is_unread {
        if unread {
            conditions
                .push("message_id IN (SELECT id FROM messages WHERE is_read = 0)".to_string());
        } else {
            conditions
                .push("message_id IN (SELECT id FROM messages WHERE is_read = 1)".to_string());
        }
    }
    if let Some(label) = &filters.label {
        conditions.push(format!(
            "message_id IN (SELECT id FROM messages WHERE folder_id IN (SELECT id FROM folders WHERE name LIKE '%{}%'))",
            label.replace('\'', "''")
        ));
    }

    // Fallback: if no FTS, query messages table directly.
    if !has_fts {
        let mut sql = String::from("SELECT id, subject, NULL, 0.0 FROM messages WHERE 1=1");
        if let Some(acc) = account_id {
            sql.push_str(&format!(" AND account_id = '{}'", acc.replace('\'', "''")));
        }
        if filters.has_attachment {
            sql.push_str(" AND has_attachments = 1");
        }
        if let Some(before) = filters.before {
            sql.push_str(&format!(" AND received_at < {}", before));
        }
        if let Some(unread) = filters.is_unread {
            if unread {
                sql.push_str(" AND is_read = 0");
            } else {
                sql.push_str(" AND is_read = 1");
            }
        }
        if let Some(label) = &filters.label {
            sql.push_str(&format!(
                " AND folder_id IN (SELECT id FROM folders WHERE name LIKE '%{}%')",
                label.replace('\'', "''")
            ));
        }
        sql.push_str(&format!(" LIMIT {}", clamped_limit));
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
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
        return Ok(results);
    }

    let where_clause = if conditions.is_empty() {
        "1=1".to_string()
    } else {
        conditions.join(" AND ")
    };

    // If has_fts, query messages_fts with MATCH; else already handled above.
    let sql = format!(
        "SELECT message_id, subject, snippet(messages_fts, 6, '<b>', '</b>', '...', 20) as snippet, \
         bm25(messages_fts, 1.0, 1.0, 10.0, 5.0, 5.0, 3.0, 1.0) as rank \
         FROM messages_fts WHERE {} ORDER BY rank LIMIT ?2",
        where_clause
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut results = Vec::new();
    if has_fts {
        let rows = stmt.query_map(params![sanitized_query.unwrap(), clamped_limit], |row| {
            Ok(SearchResult {
                message_id: row.get(0)?,
                subject: row.get(1)?,
                snippet: row.get(2)?,
                rank: row.get(3)?,
            })
        })?;
        for r in rows {
            results.push(r?);
        }
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

        // Operator tests
        let unread_matches = search_messages(&conn, "resume is:unread", None, 10).unwrap();
        assert_eq!(unread_matches.len(), 1);

        let read_matches = search_messages(&conn, "resume is:read", None, 10).unwrap();
        assert_eq!(read_matches.len(), 0);

        let from_matches = search_messages(&conn, "from:cfo@example.com", None, 10).unwrap();
        assert_eq!(from_matches.len(), 1);

        let no_attachment_matches = search_messages(&conn, "has:attachment", None, 10).unwrap();
        assert_eq!(no_attachment_matches.len(), 0);
    }
}
