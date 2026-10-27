use super::{run_json, ProcessRunner, Runner};
use anyhow::{ensure, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::BTreeMap;

const PAGE_SIZE: usize = 200;
/// Bound the scan so a phrase that matches everything cannot stall a report.
const MAX_ROWS: usize = 2_000;
const SAMPLE_COUNT: usize = 5;
const SAMPLE_CHARS: usize = 160;

/// Verbatim occurrences of a phrase in raw chat turns, counted by Remem rather
/// than inferred from LLM summaries. Raw search does not expose a complete
/// archive identity, so it cannot support a reliable cross-host session count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhraseHits {
    pub phrase: String,
    pub rows: usize,
    pub by_role: BTreeMap<String, usize>,
    /// True when the scan stopped at the row cap before Remem ran out of rows.
    pub truncated: bool,
    /// Short excerpts around the phrase, newest first, to tell real use from discussion.
    pub samples: Vec<String>,
}

impl PhraseHits {
    pub fn render(&self) -> String {
        let roles = self
            .by_role
            .iter()
            .map(|(role, count)| format!("{role} {count}"))
            .collect::<Vec<_>>()
            .join("，");
        let mut out = format!(
            "短语「{}」在本期原始会话中出现 {}{} 行（{}）。跨宿主会话数无法可靠计算：raw search 未提供完整会话身份。",
            self.phrase,
            if self.truncated { "至少 " } else { "" },
            self.rows,
            if roles.is_empty() { "无" } else { &roles }
        );
        for sample in &self.samples {
            out.push_str(&format!("\n- {sample}"));
        }
        out
    }
}

#[derive(Debug, Deserialize)]
struct SearchEnvelope {
    count: usize,
    has_more: bool,
    #[serde(default)]
    results: Vec<SearchRow>,
}

#[derive(Debug, Deserialize)]
struct SearchRow {
    role: String,
    content: String,
}

pub fn count_raw_phrase(
    phrase: &str,
    since: DateTime<Utc>,
    until: DateTime<Utc>,
) -> Result<PhraseHits> {
    count_raw_phrase_with_runner(&ProcessRunner, phrase, since, until)
}

fn count_raw_phrase_with_runner<R: Runner>(
    runner: &R,
    phrase: &str,
    since: DateTime<Utc>,
    until: DateTime<Utc>,
) -> Result<PhraseHits> {
    ensure!(!phrase.trim().is_empty(), "phrase to count is empty");
    let mut rows = 0usize;
    let mut by_role = BTreeMap::new();
    let mut samples = Vec::new();
    let mut truncated = false;
    loop {
        let args = vec![
            "raw".to_string(),
            "search".to_string(),
            phrase.to_string(),
            "--since".to_string(),
            since.timestamp().to_string(),
            "--until".to_string(),
            until.timestamp().to_string(),
            "--limit".to_string(),
            PAGE_SIZE.to_string(),
            "--offset".to_string(),
            rows.to_string(),
            "--json".to_string(),
        ];
        let page: SearchEnvelope = run_json(runner, &args, "raw search")?;
        ensure!(
            page.count == page.results.len(),
            "raw search count mismatch: declared {}, received {}",
            page.count,
            page.results.len()
        );
        for row in &page.results {
            *by_role.entry(row.role.clone()).or_insert(0) += 1;
            if samples.len() < SAMPLE_COUNT {
                samples.push(format!("[{}] {}", row.role, excerpt(&row.content, phrase)));
            }
        }
        rows += page.results.len();
        if !page.has_more || page.results.is_empty() {
            break;
        }
        if rows >= MAX_ROWS {
            truncated = true;
            break;
        }
    }
    Ok(PhraseHits {
        phrase: phrase.to_string(),
        rows,
        by_role,
        truncated,
        samples,
    })
}

fn excerpt(content: &str, phrase: &str) -> String {
    let chars: Vec<char> = content.chars().collect();
    let start = content
        .find(phrase)
        .map(|byte| content[..byte].chars().count())
        .unwrap_or(0)
        .saturating_sub(SAMPLE_CHARS / 4);
    let text: String = chars.iter().skip(start).take(SAMPLE_CHARS).collect();
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::super::CommandResult;
    use super::*;
    use chrono::TimeZone;
    use serde_json::{json, Value};
    use std::cell::RefCell;
    use std::collections::VecDeque;

    struct PagedRunner {
        pages: RefCell<VecDeque<Value>>,
        calls: RefCell<Vec<Vec<String>>>,
    }

    impl Runner for PagedRunner {
        fn run(&self, args: &[String]) -> Result<CommandResult> {
            self.calls.borrow_mut().push(args.to_vec());
            let page = self
                .pages
                .borrow_mut()
                .pop_front()
                .expect("unexpected call");
            Ok(CommandResult {
                success: true,
                code: Some(0),
                stdout: serde_json::to_vec(&page).unwrap(),
                stderr: Vec::new(),
            })
        }
    }

    fn row(session: &str, role: &str, content: &str) -> Value {
        json!({"session_id": session, "project": "/repo", "role": role, "content": content})
    }

    fn window() -> (DateTime<Utc>, DateTime<Utc>) {
        (
            Utc.with_ymd_and_hms(2026, 10, 26, 0, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2026, 11, 2, 0, 0, 0).unwrap(),
        )
    }

    #[test]
    fn counts_rows_and_roles_across_pages_without_claiming_session_count() {
        let runner = PagedRunner {
            pages: RefCell::new(VecDeque::from([
                json!({"count": 2, "has_more": true, "results": [
                    row("s1", "assistant", "已验：读回两个文件 没验：新会话"),
                    row("s1", "user", "记得写 已验：")
                ]}),
                json!({"count": 1, "has_more": false, "results": [
                    row("s2", "assistant", "已验：截图")
                ]}),
            ])),
            calls: RefCell::new(Vec::new()),
        };
        let (since, until) = window();
        let hits = count_raw_phrase_with_runner(&runner, "已验：", since, until).unwrap();
        assert_eq!(hits.rows, 3);
        assert_eq!(hits.by_role.get("assistant"), Some(&2));
        assert_eq!(hits.by_role.get("user"), Some(&1));
        assert!(!hits.truncated);
        let calls = runner.calls.borrow();
        assert_eq!(calls.len(), 2);
        assert_eq!(
            calls[1][calls[1].len() - 2],
            "2",
            "second page starts at offset 2"
        );
        assert!(calls[0].contains(&since.timestamp().to_string()));
        assert!(hits.render().contains("出现 3 行"));
        assert!(hits.render().contains("跨宿主会话数无法可靠计算"));
    }

    #[test]
    fn no_hits_renders_zero() {
        let runner = PagedRunner {
            pages: RefCell::new(VecDeque::from([
                json!({"count": 0, "has_more": false, "results": []}),
            ])),
            calls: RefCell::new(Vec::new()),
        };
        let (since, until) = window();
        let hits = count_raw_phrase_with_runner(&runner, "已验：", since, until).unwrap();
        assert_eq!(hits.rows, 0);
        assert!(hits.render().contains("出现 0 行（无）"));
    }

    #[test]
    fn empty_phrase_is_rejected() {
        let runner = PagedRunner {
            pages: RefCell::new(VecDeque::new()),
            calls: RefCell::new(Vec::new()),
        };
        let (since, until) = window();
        assert!(count_raw_phrase_with_runner(&runner, "  ", since, until).is_err());
    }

    #[test]
    fn same_session_id_on_two_hosts_does_not_claim_one_session() {
        // The same complete selectors preserved by the archive summary tests.
        // raw search currently omits this identity, so do not infer a count.
        let mut first = row("s1", "assistant", "已验：截图");
        first["host"] = json!("codex-cli");
        first["source_root"] = json!("local");
        first["session_ref"] =
            json!("remem://raw-session/v2/636f6465782d636c69/6c6f63616c/2f7265706f/7331");
        let mut second = first.clone();
        second["host"] = json!("claude-code");
        second["session_ref"] =
            json!("remem://raw-session/v2/636c617564652d636f6465/6c6f63616c/2f7265706f/7331");
        let runner = PagedRunner {
            pages: RefCell::new(VecDeque::from([json!({
                "count": 3, "has_more": false, "results": [first.clone(), second, first]
            })])),
            calls: RefCell::new(Vec::new()),
        };
        let (since, until) = window();
        let hits = count_raw_phrase_with_runner(&runner, "已验：", since, until).unwrap();
        assert_eq!(hits.rows, 3);
        assert!(hits.render().contains("跨宿主会话数无法可靠计算"));
        assert!(!hits.render().contains("跨 1 个会话"));
    }

    /// Live smoke test against the local Remem archive: `cargo test -- --ignored live_remem`.
    #[test]
    #[ignore]
    fn live_remem_counts_a_phrase() {
        let until = Utc::now();
        let since = until - chrono::Duration::days(7);
        let hits = count_raw_phrase("已验：", since, until).expect("remem raw search");
        println!("{}", hits.render());
    }
}
