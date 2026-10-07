use super::rows::{sanitize_fts_term, to_fts_query};
use rusqlite::types::Value;

pub(super) enum SearchTable {
    Items,
    Documents,
}

pub(super) struct TextSearch {
    pub sql: String,
    pub params: Vec<Value>,
}

impl TextSearch {
    /// Preserve prefix-token matching for words; match CJK phrases anywhere within a field.
    /// Short CJK terms use a literal LIKE predicate because trigram MATCH requires 3 characters.
    pub fn new(query: &str, table: SearchTable) -> Option<Self> {
        let (word_index, substring_index, columns) = match table {
            SearchTable::Items => (
                "items_fts",
                "items_substring_fts",
                &["title", "summary", "content", "tags"][..],
            ),
            SearchTable::Documents => (
                "documents_fts",
                "documents_substring_fts",
                &["title", "raw_content", "source"][..],
            ),
        };
        let terms: Vec<_> = query
            .split_whitespace()
            .filter_map(sanitize_fts_term)
            .collect();
        if terms.is_empty() {
            return None;
        }
        let (cjk, words): (Vec<_>, Vec<_>) =
            terms.into_iter().partition(|term| term.chars().any(is_cjk));
        let (long, short): (Vec<_>, Vec<_>) =
            cjk.into_iter().partition(|term| term.chars().count() >= 3);
        let mut plan = Self {
            sql: String::new(),
            params: Vec::new(),
        };
        let mut predicates = Vec::new();
        let index = if !long.is_empty() {
            substring_index
        } else if !words.is_empty() {
            word_index
        } else {
            substring_index
        };
        let rank = if long.is_empty() && words.is_empty() {
            "0.0"
        } else {
            "rank"
        };
        if !long.is_empty() {
            let phrase = long
                .iter()
                .map(|term| format!("\"{term}\""))
                .collect::<Vec<_>>()
                .join(" AND ");
            plan.params.push(Value::Text(phrase));
            predicates.push(format!("{substring_index} MATCH ?"));
        }
        if !words.is_empty() {
            plan.params
                .push(Value::Text(to_fts_query(&words.join(" "))?));
            predicates.push(if index == word_index {
                format!("{word_index} MATCH ?")
            } else {
                format!("rowid IN (SELECT rowid FROM {word_index} WHERE {word_index} MATCH ?)")
            });
        }
        for term in short {
            // Sanitization removes %, but underscore remains a literal token character.
            let pattern = format!("%{}%", term.replace('_', "\\_"));
            let alternatives = columns
                .iter()
                .map(|column| {
                    plan.params.push(Value::Text(pattern.clone()));
                    format!("{column} LIKE ? ESCAPE '\\'")
                })
                .collect::<Vec<_>>()
                .join(" OR ");
            predicates.push(format!("({alternatives})"));
        }
        plan.sql = format!(
            "SELECT rowid, {rank} AS rank FROM {index} WHERE {}",
            predicates.join(" AND ")
        );
        Some(plan)
    }
}

fn is_cjk(ch: char) -> bool {
    matches!(ch as u32,
        0x2E80..=0x2FFF | 0x3040..=0x30FF | 0x3100..=0x312F | 0x31A0..=0x31BF |
        0x31F0..=0x31FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF |
        0xF900..=0xFAFF | 0x20000..=0x323AF)
}
