//! 摄入排除规则：`~/.refine/ingest-exclude-projects.txt`，每行一个项目路径子串。
//!
//! 自动化任务（agent 队列、批量渲染、评分流水线）在专用目录里运行，
//! 它们的会话不反映用户本人的认知习惯，提炼进 Mirror 会拉偏指标。

use anyhow::{Context, Result};
use refine_core::session::FilterConfig;
use std::path::Path;
#[cfg(not(test))]
use std::path::PathBuf;

#[cfg(not(test))]
const EXCLUDE_FILE_ENV: &str = "REFINE_INGEST_EXCLUDE_FILE";
#[cfg(not(test))]
const DEFAULT_FILE_NAME: &str = "ingest-exclude-projects.txt";

#[cfg(test)]
thread_local! {
    static TEST_PATTERNS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Tests never read the real `~/.refine` file; they inject patterns per thread.
#[cfg(test)]
pub(super) fn set_test_patterns(patterns: &[&str]) {
    TEST_PATTERNS
        .with(|cell| *cell.borrow_mut() = patterns.iter().map(|p| p.to_string()).collect());
}

#[cfg(test)]
pub(super) fn load_filter_config() -> Result<FilterConfig> {
    Ok(FilterConfig {
        excluded_project_patterns: TEST_PATTERNS.with(|cell| cell.borrow().clone()),
        ..FilterConfig::default()
    })
}

#[cfg(not(test))]
pub(super) fn load_filter_config() -> Result<FilterConfig> {
    let Some(path) = exclude_file_path() else {
        return Ok(FilterConfig::default());
    };
    let patterns = read_patterns(&path)?;
    if !patterns.is_empty() {
        println!("排除项目规则 {} 条（{}）", patterns.len(), path.display());
    }
    Ok(FilterConfig {
        excluded_project_patterns: patterns,
        ..FilterConfig::default()
    })
}

#[cfg(not(test))]
fn exclude_file_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var(EXCLUDE_FILE_ENV) {
        if !path.trim().is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    dirs::home_dir().map(|home| home.join(".refine").join(DEFAULT_FILE_NAME))
}

fn read_patterns(path: &Path) -> Result<Vec<String>> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("读取摄入排除规则失败: {}", path.display()))
        }
    };
    Ok(parse_patterns(&content))
}

fn parse_patterns(content: &str) -> Vec<String> {
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_patterns_skipping_blank_lines_and_comments() {
        let patterns = parse_patterns("# agent queues\n  /.harness/local/  \n\n/.loom-registry\n");
        assert_eq!(patterns, vec!["/.harness/local/", "/.loom-registry"]);
    }

    #[test]
    fn missing_file_means_no_exclusions() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_patterns(&dir.path().join("absent.txt"))
            .unwrap()
            .is_empty());
    }
}
