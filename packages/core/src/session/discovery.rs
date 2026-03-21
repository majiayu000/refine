//! 会话文件发现
//!
//! 扫描 Claude Code 和 Codex 的会话 JSONL 文件

use super::types::SessionSource;
use std::path::{Path, PathBuf};

/// 发现的会话文件
#[derive(Debug, Clone)]
pub struct DiscoveredSession {
    pub path: PathBuf,
    pub source: SessionSource,
    pub project: Option<String>,
}

/// 扫描所有会话文件
///
/// Claude Code: `~/.claude/projects/*/*.jsonl`
/// Codex:       `~/.codex/sessions/**/*.jsonl`
pub fn discover_sessions(source_filter: Option<SessionSource>) -> Vec<DiscoveredSession> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return Vec::new(),
    };
    discover_sessions_in(&home, source_filter)
}

/// 可测试版本：指定 home 目录
pub fn discover_sessions_in(
    home: &Path,
    source_filter: Option<SessionSource>,
) -> Vec<DiscoveredSession> {
    let mut results = Vec::new();

    if source_filter.as_ref().map_or(true, |s| *s == SessionSource::ClaudeCode) {
        discover_claude_code(home, &mut results);
    }
    if source_filter.as_ref().map_or(true, |s| *s == SessionSource::Codex) {
        discover_codex(home, &mut results);
    }

    results.sort_by(|a, b| a.path.cmp(&b.path));
    results
}

fn discover_claude_code(home: &Path, results: &mut Vec<DiscoveredSession>) {
    let projects_dir = home.join(".claude").join("projects");
    let entries = match std::fs::read_dir(&projects_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let project_dir = entry.path();
        if !project_dir.is_dir() {
            continue;
        }
        let project_name = extract_project_name(&project_dir);

        // Scan project dir and one level of subdirectories (session UUID dirs)
        let mut dirs_to_scan = vec![project_dir.clone()];
        if let Ok(sub_entries) = std::fs::read_dir(&project_dir) {
            for sub in sub_entries.flatten() {
                let sub_path = sub.path();
                if sub_path.is_dir() && !sub_path.ends_with("subagents") {
                    dirs_to_scan.push(sub_path);
                }
            }
        }

        for scan_dir in &dirs_to_scan {
            let files = match std::fs::read_dir(scan_dir) {
                Ok(f) => f,
                Err(_) => continue,
            };

            for file in files.flatten() {
                let path = file.path();
                if !is_session_jsonl(&path) {
                    continue;
                }
                if is_subagent_file(&path) {
                    continue;
                }
                results.push(DiscoveredSession {
                    path,
                    source: SessionSource::ClaudeCode,
                    project: Some(project_name.clone()),
                });
            }
        }
    }
}

fn discover_codex(home: &Path, results: &mut Vec<DiscoveredSession>) {
    let sessions_dir = home.join(".codex").join("sessions");
    walk_jsonl_recursive(&sessions_dir, SessionSource::Codex, results);
}

fn walk_jsonl_recursive(
    dir: &Path,
    source: SessionSource,
    results: &mut Vec<DiscoveredSession>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // 跳过 subagents 目录
            if path.file_name().map_or(false, |n| n == "subagents") {
                continue;
            }
            walk_jsonl_recursive(&path, source.clone(), results);
        } else if is_session_jsonl(&path) && !is_subagent_file(&path) {
            results.push(DiscoveredSession {
                path,
                source: source.clone(),
                project: None,
            });
        }
    }
}

fn is_session_jsonl(path: &Path) -> bool {
    path.extension().map_or(false, |ext| ext == "jsonl")
}

fn is_subagent_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map_or(false, |name| name.starts_with("agent-"))
}

/// 从路径提取项目名（最后一个目录组件）
fn extract_project_name(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn discover_finds_claude_code_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();

        // 创建 Claude Code 结构
        let project_dir = home.join(".claude/projects/my-project");
        fs::create_dir_all(&project_dir).unwrap();
        fs::write(project_dir.join("abc123.jsonl"), "{}").unwrap();
        fs::write(project_dir.join("agent-sub.jsonl"), "{}").unwrap();
        fs::write(project_dir.join("notes.txt"), "not a session").unwrap();

        let results = discover_sessions_in(home, Some(SessionSource::ClaudeCode));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].source, SessionSource::ClaudeCode);
        assert_eq!(results[0].project.as_deref(), Some("my-project"));
        assert!(results[0].path.to_str().unwrap().contains("abc123.jsonl"));
    }

    #[test]
    fn discover_finds_codex_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();

        let session_dir = home.join(".codex/sessions/2026");
        fs::create_dir_all(&session_dir).unwrap();
        fs::write(session_dir.join("sess1.jsonl"), "{}").unwrap();

        // subagents 目录应跳过
        let sub_dir = session_dir.join("subagents");
        fs::create_dir_all(&sub_dir).unwrap();
        fs::write(sub_dir.join("sub.jsonl"), "{}").unwrap();

        let results = discover_sessions_in(home, Some(SessionSource::Codex));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].source, SessionSource::Codex);
    }

    #[test]
    fn discover_respects_source_filter() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();

        let claude_dir = home.join(".claude/projects/proj");
        fs::create_dir_all(&claude_dir).unwrap();
        fs::write(claude_dir.join("s1.jsonl"), "{}").unwrap();

        let codex_dir = home.join(".codex/sessions");
        fs::create_dir_all(&codex_dir).unwrap();
        fs::write(codex_dir.join("s2.jsonl"), "{}").unwrap();

        let claude_only = discover_sessions_in(home, Some(SessionSource::ClaudeCode));
        assert_eq!(claude_only.len(), 1);

        let all = discover_sessions_in(home, None);
        assert_eq!(all.len(), 2);
    }
}
