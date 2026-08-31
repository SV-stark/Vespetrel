use rusqlite::{Connection, params};

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub message_id: String,
    pub subject: Option<String>,
    pub snippet: Option<String>,
    pub rank: f64,
}

/// Escape user input for SQLite FTS5 MATCH queries
pub fn escape_fts5_query(raw: &str) -> String {
    let mut terms = Vec::new();
    for word in raw.split_whitespace() {
        let clean: String = word
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '@' || *c == '.' || *c == '_' || *c == '-')
            .collect();
        if !clean.is_empty() {
            // Enclose each token in double quotes to prevent FTS5 keyword collisions
            terms.push(format!("\"{clean}\""));
        }
    }
    terms.join(" ")
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
        SELECT message_id, subject, snippet(messages_fts, 6, '<b>', '</b>', '...', 20) as snippet, rank
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
        assert_eq!(
            escape_fts5_query("user@example.com OR 1=1"),
            "\"user@example.com\" \"OR\" \"11\""
        );
        assert_eq!(escape_fts5_query("   "), "");
    }
}
