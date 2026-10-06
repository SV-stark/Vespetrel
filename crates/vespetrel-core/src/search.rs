//! Canonical SQLite FTS5 query sanitization.
//!
//! This lives in `vespetrel-core` rather than `vespetrel-storage` because two callers must
//! agree byte-for-byte on how a user query becomes an FTS5 `MATCH` expression:
//! `storage::fts::search_messages` (which passes it as a bound parameter) and
//! `core::virtual_folder::to_sql_filter` (which interpolates it into a `WHERE` clause).
//!
//! They previously had separate implementations. The `virtual_folder` copy had no support for
//! `from:`-style field prefixes, so `from:alice@example.com` was searched as a literal term
//! rather than a sender filter, and it fell back to the expression `MATCH '"*"'` for a query
//! that sanitized to nothing — which FTS5 rejects as a syntax error, turning an empty saved
//! search into a failed query.

/// FTS5 column prefixes a user may apply to a term.
const FIELD_PREFIXES: [(&str, &str); 4] = [
    ("from", "from_address:"),
    ("to", "to_addresses:"),
    ("subject", "subject:"),
    ("body", "body_content:"),
];

/// Characters permitted inside a sanitized term.
///
/// FTS5 operators (`*`, `^`, `-`, `(`, `)`, `:`, quotes) are excluded, so a query can only ever
/// produce quoted phrases. Everything surviving this filter is inert as FTS5 syntax.
fn is_term_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '@' | '.' | '_' | '-' | '+' | '#' | '"')
}

/// Sanitize one raw word into a quoted FTS5 term, honouring a leading `field:` prefix.
///
/// Returns `None` when the word contains nothing searchable, so callers can decide what an
/// empty query means rather than inheriting a default.
pub fn escape_fts5_term(word: &str) -> Option<String> {
    let (prefix, term) = match word.split_once(':') {
        Some((field, value)) => match FIELD_PREFIXES
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(field))
        {
            Some((_, column)) => (Some(*column), value),
            // An unknown prefix is not a filter; search the whole word as a phrase.
            None => (None, word),
        },
        None => (None, word),
    };

    let clean: String = term.chars().filter(|c| is_term_char(*c)).collect();
    if clean.is_empty() {
        return None;
    }

    // Double embedded quotes: a `"` inside a quoted phrase would otherwise terminate it early
    // and let the remainder be parsed as FTS5 syntax.
    let escaped = clean.replace('"', "\"\"");
    Some(match prefix {
        Some(column) => format!("{column}\"{escaped}\""),
        None => format!("\"{escaped}\""),
    })
}

/// Sanitize a full user query into an FTS5 `MATCH` expression.
///
/// Returns `None` when no searchable term survives. Callers must treat that as "no results"
/// rather than substituting a wildcard: `MATCH '"*"'` is a syntax error, not a match-all.
pub fn escape_fts5_query(raw: &str) -> Option<String> {
    let terms: Vec<String> = raw
        .split_whitespace()
        .filter_map(escape_fts5_term)
        .collect();

    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

/// SQL predicate restricting `messages` to rows matching `query`.
///
/// Returns a predicate that is never true when the query has no searchable terms. This keeps the
/// sanitized expression a bound parameter, so no user input is ever concatenated into SQL.
pub fn fts5_predicate(query: &str) -> String {
    match escape_fts5_query(query) {
        Some(_) => {
            "id IN (SELECT message_id FROM messages_fts WHERE messages_fts MATCH ?)".to_string()
        }
        // An unsatisfiable predicate, not a wildcard. `MATCH '"*"'` is a syntax error and made
        // an empty saved search fail outright.
        None => "0".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terms_are_quoted_and_operators_are_neutralised() {
        assert_eq!(
            escape_fts5_query("hello world").unwrap(),
            "\"hello\" \"world\""
        );
        // FTS5 operators must not survive as syntax. `=` is stripped, so "1=1" becomes the
        // inert phrase "11" rather than anything resembling a comparison.
        assert_eq!(
            escape_fts5_query("user@example.com OR 1=1").unwrap(),
            "\"user@example.com\" \"OR\" \"11\""
        );
        assert_eq!(escape_fts5_query("resume*").unwrap(), "\"resume\"");
        // Existing quote characters are preserved and doubled, so `"test"` stays a phrase
        // instead of terminating early.
        assert_eq!(
            escape_fts5_query("\"test\" OR C++").unwrap(),
            "\"\"\"test\"\"\" \"OR\" \"C++\""
        );
    }

    #[test]
    fn field_prefixes_map_to_columns() {
        assert_eq!(
            escape_fts5_query("from:alice@example.com").unwrap(),
            "from_address:\"alice@example.com\""
        );
        assert_eq!(
            escape_fts5_query("subject:budget").unwrap(),
            "subject:\"budget\""
        );
        // Case-insensitive prefix, since users type `From:`.
        assert_eq!(
            escape_fts5_query("FROM:alice@example.com").unwrap(),
            "from_address:\"alice@example.com\""
        );
        // An unknown prefix is searched as a phrase rather than silently dropped. The `:` is
        // stripped, leaving an inert phrase rather than something FTS5 would read as a
        // column filter.
        assert_eq!(escape_fts5_query("weird:value").unwrap(), "\"weirdvalue\"");
    }

    #[test]
    fn embedded_quotes_cannot_close_the_phrase_early() {
        // A bare `"` would otherwise terminate the phrase and let the rest be read as syntax.
        let out = escape_fts5_query("a\"b").unwrap();
        assert_eq!(out, "\"a\"\"b\"");
    }

    #[test]
    fn empty_or_operator_only_query_yields_none() {
        assert!(escape_fts5_query("").is_none());
        assert!(escape_fts5_query("   ").is_none());
        assert!(escape_fts5_query("*").is_none());
        assert!(escape_fts5_query("^ ()").is_none());
    }

    #[test]
    fn empty_query_predicate_is_never_true_and_is_not_a_wildcard() {
        let predicate = fts5_predicate("   ");
        assert_eq!(predicate, "0");
        assert!(
            !predicate.contains('*'),
            "a bare wildcard is a syntax error in FTS5, not a match-all"
        );
        // A real query binds its expression as a parameter instead of interpolating it.
        assert!(fts5_predicate("budget").contains('?'));
    }
}
