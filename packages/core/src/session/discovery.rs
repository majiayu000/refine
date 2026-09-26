//! 会话文件发现
//!
//! 扫描 Claude Code 和 Codex 的会话 JSONL 文件

use super::types::SessionSource;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tracing::warn;

/// 发现的会话文件
#[derive(Debug, Clone)]
pub struct DiscoveredSession {
    pub path: PathBuf,
    pub source: SessionSource,
    pub project: Option<String>,
    /// 文件最后修改时间；stat/mtime 失败时记录 warn 并回退为 UNIX_EPOCH。
    pub modified_at: SystemTime,
}

/// 扫描所有会话文件
///
/// Claude Code: `$CLAUDE_CONFIG_DIR/projects/*/*.jsonl`（默认 `~/.claude`）
/// Codex: `$CODEX_HOME/sessions/**/*.jsonl`（默认 `~/.codex`）
///
/// `mtime_after`: 仅返回 `modified_at >= mtime_after` 的文件；`None` 返回全部。
pub fn discover_sessions(
    source_filter: Option<SessionSource>,
    mtime_after: Option<SystemTime>,
) -> Vec<DiscoveredSession> {
    let roots = match source_filter {
        Some(SessionSource::ClaudeCode) => {
            agent_sessions::Roots::from_env_for(agent_sessions::Agent::ClaudeCode)
        }
        Some(SessionSource::Codex) => {
            agent_sessions::Roots::from_env_for(agent_sessions::Agent::Codex)
        }
        Some(SessionSource::Cursor | SessionSource::RememRaw) => return Vec::new(),
        None => agent_sessions::Roots::from_env(),
    };
    match roots {
        Ok(roots) => discover_sessions_with_roots(&roots, source_filter, mtime_after),
        Err(error) => {
            tracing::error!(%error, "failed to resolve session roots");
            Vec::new()
        }
    }
}

/// 可测试版本：指定 home 目录，不读取进程环境变量。
pub fn discover_sessions_in(
    home: &Path,
    source_filter: Option<SessionSource>,
    mtime_after: Option<SystemTime>,
) -> Vec<DiscoveredSession> {
    discover_sessions_with_roots(
        &agent_sessions::Roots::from_home(home),
        source_filter,
        mtime_after,
    )
}

fn discover_sessions_with_roots(
    roots: &agent_sessions::Roots,
    source_filter: Option<SessionSource>,
    mtime_after: Option<SystemTime>,
) -> Vec<DiscoveredSession> {
    use agent_sessions::{Agent, DiscoverFilter};
    let agents = match source_filter {
        Some(SessionSource::ClaudeCode) => vec![Agent::ClaudeCode],
        Some(SessionSource::Codex) => vec![Agent::Codex],
        Some(SessionSource::Cursor | SessionSource::RememRaw) => return Vec::new(),
        None => vec![Agent::ClaudeCode, Agent::Codex],
    };
    let discovered = agent_sessions::discover(
        roots,
        &DiscoverFilter {
            agents,
            include_subagents: false,
            modified_after: mtime_after,
        },
    );
    map_discovery(roots, discovered, mtime_after)
}

fn map_discovery(
    roots: &agent_sessions::Roots,
    discovered: agent_sessions::Discovery,
    mtime_after: Option<SystemTime>,
) -> Vec<DiscoveredSession> {
    let mut candidates: Vec<_> = discovered
        .files
        .into_iter()
        .map(|file| (file.agent, file.path, file.modified))
        .collect();
    for error in discovered.errors {
        if error.operation == agent_sessions::DiscoverOperation::InspectFile {
            warn!(path = %error.path.display(), error = %error.source,
                "failed to read mtime; file treated as oldest for --latest");
            if mtime_after.is_none_or(|cutoff| SystemTime::UNIX_EPOCH >= cutoff) {
                candidates.push((error.agent, error.path, SystemTime::UNIX_EPOCH));
            }
        } else {
            warn!(path = %error.path.display(), error = %error.source, "failed to discover session file");
        }
    }
    let mut results: Vec<_> = candidates
        .into_iter()
        .filter_map(|(agent, path, modified_at)| {
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("agent-"))
            {
                return None;
            }
            let (source, project) = match agent {
                agent_sessions::Agent::ClaudeCode => {
                    let relative = path
                        .strip_prefix(roots.claude.as_ref()?.join("projects"))
                        .ok()?;
                    // Retain the legacy project/session directory depth policy.
                    if !(2..=3).contains(&relative.components().count()) {
                        return None;
                    }
                    let project = relative
                        .components()
                        .next()?
                        .as_os_str()
                        .to_str()
                        .unwrap_or("unknown")
                        .to_owned();
                    (SessionSource::ClaudeCode, Some(project))
                }
                agent_sessions::Agent::Codex => {
                    if !path.starts_with(roots.codex.as_ref()?.join("sessions")) {
                        return None;
                    }
                    (SessionSource::Codex, None)
                }
                _ => return None,
            };
            Some(DiscoveredSession {
                path,
                source,
                project,
                modified_at,
            })
        })
        .collect();
    results.sort_by(|a, b| a.path.cmp(&b.path));
    results
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

        let results = discover_sessions_in(home, Some(SessionSource::ClaudeCode), None);
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

        let results = discover_sessions_in(home, Some(SessionSource::Codex), None);
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

        let claude_only = discover_sessions_in(home, Some(SessionSource::ClaudeCode), None);
        assert_eq!(claude_only.len(), 1);

        let all = discover_sessions_in(home, None, None);
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn modified_at_is_populated() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();

        let project_dir = home.join(".claude/projects/proj");
        fs::create_dir_all(&project_dir).unwrap();
        fs::write(project_dir.join("sess.jsonl"), "{}").unwrap();

        let results = discover_sessions_in(home, Some(SessionSource::ClaudeCode), None);
        assert_eq!(results.len(), 1);
        // A freshly-created file must have mtime > UNIX_EPOCH
        assert!(results[0].modified_at > SystemTime::UNIX_EPOCH);
    }

    #[test]
    fn latest_returns_most_recent_n() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();

        let project_dir = home.join(".claude/projects/proj");
        fs::create_dir_all(&project_dir).unwrap();

        // Create 5 files with explicitly staggered mtimes (1 s apart)
        let _base = filetime::FileTime::from_unix_time(1_700_000_000, 0);
        let files = ["a.jsonl", "b.jsonl", "c.jsonl", "d.jsonl", "e.jsonl"];
        for (i, name) in files.iter().enumerate() {
            let p = project_dir.join(name);
            fs::write(&p, "{}").unwrap();
            let t = filetime::FileTime::from_unix_time(1_700_000_000 + i as i64, 0);
            filetime::set_file_mtime(&p, t).unwrap();
        }

        let mut discovered = discover_sessions_in(home, Some(SessionSource::ClaudeCode), None);
        assert_eq!(discovered.len(), 5);

        // Simulate --latest 3
        discovered.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
        discovered.truncate(3);

        // Newest 3 are e, d, c (offsets 4, 3, 2)
        let names: Vec<&str> = discovered
            .iter()
            .map(|s| s.path.file_name().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(names, vec!["e.jsonl", "d.jsonl", "c.jsonl"]);
    }

    #[test]
    fn latest_n_larger_than_total_returns_all() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();

        let project_dir = home.join(".claude/projects/proj");
        fs::create_dir_all(&project_dir).unwrap();
        for name in &["x.jsonl", "y.jsonl", "z.jsonl"] {
            fs::write(project_dir.join(name), "{}").unwrap();
        }

        let mut discovered = discover_sessions_in(home, Some(SessionSource::ClaudeCode), None);
        // --latest 100 on 3 files
        discovered.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
        discovered.truncate(100);
        assert_eq!(discovered.len(), 3);
    }

    #[test]
    fn existing_limit_returns_path_order() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();

        let project_dir = home.join(".claude/projects/proj");
        fs::create_dir_all(&project_dir).unwrap();
        for name in &["a.jsonl", "b.jsonl", "c.jsonl"] {
            fs::write(project_dir.join(name), "{}").unwrap();
        }

        // discover_sessions_in already sorts by path
        let discovered = discover_sessions_in(home, Some(SessionSource::ClaudeCode), None);
        let limited: Vec<_> = discovered.into_iter().take(2).collect();
        assert_eq!(limited.len(), 2);
        let names: Vec<&str> = limited
            .iter()
            .map(|s| s.path.file_name().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(names, vec!["a.jsonl", "b.jsonl"]);
    }

    #[test]
    fn mtime_after_filters_old_files() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();

        let project_dir = home.join(".claude/projects/proj");
        fs::create_dir_all(&project_dir).unwrap();

        // old.jsonl: mtime = epoch + 1000 s (definitely old)
        // new.jsonl: mtime = epoch + 9000 s (newer)
        let old_path = project_dir.join("old.jsonl");
        let new_path = project_dir.join("new.jsonl");
        fs::write(&old_path, "{}").unwrap();
        fs::write(&new_path, "{}").unwrap();
        filetime::set_file_mtime(&old_path, filetime::FileTime::from_unix_time(1000, 0)).unwrap();
        filetime::set_file_mtime(&new_path, filetime::FileTime::from_unix_time(9000, 0)).unwrap();

        // cutoff = epoch + 5000 s — only new.jsonl should pass
        let cutoff = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(5000);
        let results = discover_sessions_in(home, Some(SessionSource::ClaudeCode), Some(cutoff));
        assert_eq!(results.len(), 1);
        assert!(results[0].path.file_name().unwrap() == "new.jsonl");
    }
    #[test]
    fn explicit_roots_preserve_codex_sessions_scope_and_claude_depth() {
        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path().join("custom-claude");
        let codex = tmp.path().join("custom-codex");
        for path in [
            claude.join("projects/proj/id/main.jsonl"),
            claude.join("projects/proj/id/deeper/ignored.jsonl"),
            codex.join("archived_sessions/archive.jsonl"),
            codex.join("sessions/archived_sessions/current.jsonl"),
        ] {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "{}").unwrap();
        }
        let found = discover_sessions_with_roots(
            &agent_sessions::Roots {
                claude: Some(claude),
                codex: Some(codex),
            },
            None,
            None,
        );
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].project.as_deref(), Some("proj"));
        assert_eq!(found[1].source, SessionSource::Codex);
        assert_eq!(found[1].path.file_name().unwrap(), "current.jsonl");
    }
    #[test]
    fn file_metadata_failure_retains_epoch_candidate_but_directory_failure_does_not() {
        use agent_sessions::{Agent, DiscoverError, DiscoverOperation, Discovery};
        let tmp = tempfile::tempdir().unwrap();
        let roots = agent_sessions::Roots::from_home(tmp.path());
        let candidate = tmp.path().join(".claude/projects/proj/session.jsonl");
        let failure = || Discovery {
            files: vec![],
            errors: vec![
                DiscoverError {
                    agent: Agent::ClaudeCode,
                    path: candidate.clone(),
                    operation: DiscoverOperation::InspectFile,
                    source: std::io::Error::other("mtime unavailable"),
                },
                DiscoverError {
                    agent: Agent::ClaudeCode,
                    path: tmp.path().join(".claude/projects/bad.jsonl"),
                    operation: DiscoverOperation::ReadDirectory,
                    source: std::io::Error::other("directory unavailable"),
                },
            ],
        };
        let found = map_discovery(&roots, failure(), None);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, candidate);
        assert_eq!(found[0].modified_at, SystemTime::UNIX_EPOCH);
        assert!(map_discovery(
            &roots,
            failure(),
            Some(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1))
        )
        .is_empty());
    }
    #[cfg(unix)]
    #[test]
    fn non_utf8_project_name_retains_unknown_label() {
        use std::os::unix::ffi::OsStringExt;
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp
            .path()
            .join(".claude/projects")
            .join(std::ffi::OsString::from_vec(vec![0xff]));
        // APFS rejects invalid UTF-8 directory creation; inject the shared
        // discovery result to exercise the same Unix path-label policy.
        let found = map_discovery(
            &agent_sessions::Roots::from_home(tmp.path()),
            agent_sessions::Discovery {
                files: vec![agent_sessions::SessionFile {
                    agent: agent_sessions::Agent::ClaudeCode,
                    path: project.join("session.jsonl"),
                    kind: agent_sessions::FileKind::Main,
                    size: 2,
                    modified: SystemTime::UNIX_EPOCH,
                }],
                errors: vec![],
            },
            None,
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].project.as_deref(), Some("unknown"));
    }
}
