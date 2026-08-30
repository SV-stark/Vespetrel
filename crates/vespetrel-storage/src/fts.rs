use rusqlite::{params, Connection};

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub message_id: String,
    pub subject: Option<String>,
    pub snippet: Option<String>,
    pub rank: f64,
}

/// Sub-15ms FTS5 search with BM25 ranking - §3.2
pub fn search_messages(
    conn: &Connection,
    query: &str,
    account_id: Option<&str>,
    limit: usize,
) -> anyhow::Result<Vec<SearchResult>> {
    let sql = if account_id.is_some() {
        r#"
        SELECT message_id, subject, snippet(messages_fts, 6, '<b>', '</b>', '...', 20) as snippet, rank
        FROM messages_fts
        WHERE messages_fts MATCH ?1 AND account_id = ?2
        ORDER BY rank
        LIMIT ?3
        "#
    } else {
        r#"
        SELECT message_id, subject, snippet(messages_fts, 6, '<b>', '</b>', '...', 20) as snippet, rank
        FROM messages_fts
        WHERE messages_fts MATCH ?1
        ORDER BY rank
        LIMIT ?2
        "#
    };

    let mut stmt = conn.prepare(sql)?;
    let rows = if let Some(aid) = account_id {
        stmt.query_map(params![query, aid, limit as i64], |row| {
            Ok(SearchResult {
                message_id: row.get(0)?,
                subject: row.get(1)?,
                snippet: row.get(2)?,
                rank: row.get(3)?,
            })
        })?
    } else {
        stmt.query_map(params![query, limit as i64], |row| {
            Ok(SearchResult {
                message_id: row.get(0)?,
                subject: row.get(1)?,
                snippet: row.get(2)?,
                rank: row.get(3)?,
            })
        })?
    };

    let mut results = Vec::new();
    for r in rows {
        results.push(r?);
    }
    Ok(results)
}
