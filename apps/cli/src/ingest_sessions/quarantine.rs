use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

const QUARANTINE_ENV: &str = "REFINE_INGEST_QUARANTINE_PATH";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct QuarantineRecord {
    pub url: String,
    #[serde(default)]
    pub source_version: Option<String>,
    pub code: String,
    pub message: String,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub attempts: u64,
}

pub(super) struct QuarantineStore {
    path: PathBuf,
    records: BTreeMap<String, QuarantineRecord>,
    dirty: bool,
    _lock_file: std::fs::File,
}

impl QuarantineStore {
    pub(super) fn load() -> Result<Self> {
        Self::load_from(default_path()?)
    }

    pub(super) fn load_from(path: PathBuf) -> Result<Self> {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("隔离队列路径没有父目录: {}", path.display()))?;
        std::fs::create_dir_all(parent)
            .with_context(|| format!("创建 ingest 隔离目录失败: {}", parent.display()))?;
        secure_default_parent(parent)?;
        let lock_path = path.with_extension("lock");
        let mut lock_options = std::fs::OpenOptions::new();
        lock_options
            .create(true)
            .read(true)
            .write(true)
            .truncate(false);
        #[cfg(unix)]
        lock_options.mode(0o600);
        let lock_file = lock_options
            .open(&lock_path)
            .with_context(|| format!("打开 ingest 隔离锁失败: {}", lock_path.display()))?;
        lock_file
            .lock_exclusive()
            .with_context(|| format!("获取 ingest 隔离锁失败: {}", lock_path.display()))?;

        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("读取 ingest 隔离队列失败: {}", path.display()))
            }
        };

        let mut records = BTreeMap::new();
        for (index, line) in content.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let record: QuarantineRecord = serde_json::from_str(line).with_context(|| {
                format!(
                    "ingest 隔离队列第 {} 行损坏，拒绝静默跳过: {}",
                    index + 1,
                    path.display()
                )
            })?;
            records.insert(
                record_key(&record.url, record.source_version.as_deref()),
                record,
            );
        }

        Ok(Self {
            path,
            records,
            dirty: false,
            _lock_file: lock_file,
        })
    }

    pub(super) fn contains(&self, url: &str, source_version: Option<&str>) -> bool {
        self.records.contains_key(&record_key(url, source_version))
    }

    pub(super) fn len(&self) -> usize {
        self.records.len()
    }

    pub(super) fn count_matching(&self, identities: &HashSet<String>) -> usize {
        identities
            .iter()
            .filter(|identity| self.records.contains_key(identity.as_str()))
            .count()
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn record(
        &mut self,
        url: &str,
        source_version: Option<&str>,
        code: &str,
        message: &str,
    ) {
        let now = Utc::now();
        let key = record_key(url, source_version);
        match self.records.get_mut(&key) {
            Some(record) => {
                record.code = code.to_string();
                record.message = message.to_string();
                record.last_seen = now;
                record.attempts = record.attempts.saturating_add(1);
            }
            None => {
                self.records.insert(
                    key,
                    QuarantineRecord {
                        url: url.to_string(),
                        source_version: source_version.map(ToOwned::to_owned),
                        code: code.to_string(),
                        message: message.to_string(),
                        first_seen: now,
                        last_seen: now,
                        attempts: 1,
                    },
                );
            }
        }
        self.dirty = true;
    }

    pub(super) fn resolve(&mut self, url: &str) {
        let previous_len = self.records.len();
        self.records.retain(|_, record| record.url != url);
        if self.records.len() != previous_len {
            self.dirty = true;
        }
    }

    pub(super) fn save_if_dirty(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        let parent = self
            .path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("隔离队列路径没有父目录: {}", self.path.display()))?;
        std::fs::create_dir_all(parent)
            .with_context(|| format!("创建 ingest 隔离目录失败: {}", parent.display()))?;
        secure_default_parent(parent)?;

        let file_name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("ingest-quarantine.jsonl");
        let temp_path = parent.join(format!(".{file_name}.{}.tmp", std::process::id()));
        let write_result = (|| -> Result<()> {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            options.mode(0o600);
            let mut file = options.open(&temp_path).with_context(|| {
                format!("创建 ingest 隔离临时文件失败: {}", temp_path.display())
            })?;
            for record in self.records.values() {
                serde_json::to_writer(&mut file, record).context("序列化 ingest 隔离记录失败")?;
                file.write_all(b"\n")?;
            }
            file.sync_all()?;
            std::fs::rename(&temp_path, &self.path).with_context(|| {
                format!(
                    "原子替换 ingest 隔离队列失败: {} -> {}",
                    temp_path.display(),
                    self.path.display()
                )
            })?;
            Ok(())
        })();
        if write_result.is_err() {
            let _ = std::fs::remove_file(&temp_path);
        }
        write_result?;
        self.dirty = false;
        Ok(())
    }
}

pub(super) fn record_key(url: &str, source_version: Option<&str>) -> String {
    format!("{url}\u{0}{}", source_version.unwrap_or_default())
}

fn secure_default_parent(parent: &Path) -> Result<()> {
    #[cfg(unix)]
    if dirs::home_dir().as_deref().map(|home| home.join(".refine")) == Some(parent.to_path_buf()) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("收紧 ingest 隔离目录权限失败: {}", parent.display()))?;
    }
    Ok(())
}

fn default_path() -> Result<PathBuf> {
    if let Ok(path) = std::env::var(QUARANTINE_ENV) {
        if !path.trim().is_empty() {
            return Ok(PathBuf::from(path));
        }
    }
    let home = dirs::home_dir().context("无法定位 HOME，不能保存 ingest 隔离队列")?;
    Ok(home.join(".refine").join("ingest-quarantine.jsonl"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarantine_round_trip_deduplicates_by_url() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("queue.jsonl");
        let mut store = QuarantineStore::load_from(path.clone()).unwrap();
        store.record(
            "remem://one",
            Some("v1"),
            "sensitive_words_detected",
            "blocked",
        );
        store.record(
            "remem://one",
            Some("v1"),
            "sensitive_words_detected",
            "blocked again",
        );
        store.save_if_dirty().unwrap();
        drop(store);

        let loaded = QuarantineStore::load_from(path).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(
            loaded.records[&record_key("remem://one", Some("v1"))].attempts,
            2
        );
    }

    #[test]
    fn concurrent_read_modify_write_preserves_both_updates() {
        use std::sync::mpsc;
        use std::thread;
        use std::time::Duration;

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("queue.jsonl");
        let first_path = path.clone();
        let second_path = path.clone();
        let (locked_tx, locked_rx) = mpsc::channel();

        let first = thread::spawn(move || {
            let mut store = QuarantineStore::load_from(first_path).unwrap();
            store.record("remem://one", Some("v1"), "blocked", "one");
            locked_tx.send(()).unwrap();
            thread::sleep(Duration::from_millis(100));
            store.save_if_dirty().unwrap();
        });
        locked_rx.recv().unwrap();
        let second = thread::spawn(move || {
            let mut store = QuarantineStore::load_from(second_path).unwrap();
            store.record("remem://two", Some("v1"), "blocked", "two");
            store.save_if_dirty().unwrap();
        });

        first.join().unwrap();
        second.join().unwrap();
        let loaded = QuarantineStore::load_from(path).unwrap();
        assert_eq!(loaded.len(), 2);
    }

    #[test]
    fn malformed_quarantine_fails_closed() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("queue.jsonl");
        std::fs::write(&path, "not-json\n").unwrap();
        let error = QuarantineStore::load_from(path)
            .err()
            .expect("malformed queue must fail");
        assert!(error.to_string().contains("拒绝静默跳过"));
    }

    #[test]
    fn matching_count_ignores_records_outside_the_current_selection() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("queue.jsonl");
        let mut store = QuarantineStore::load_from(path).unwrap();
        store.record("remem://selected", Some("v1"), "blocked", "selected");
        store.record("file://unrelated", Some("v1"), "blocked", "unrelated");

        let selected = HashSet::from([record_key("remem://selected", Some("v1"))]);
        assert_eq!(store.count_matching(&selected), 1);

        let other_provider = HashSet::from([record_key("remem://other", Some("v1"))]);
        assert_eq!(store.count_matching(&other_provider), 0);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn changed_snapshot_is_not_blocked_by_old_rejection() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = QuarantineStore::load_from(temp.path().join("queue.jsonl")).unwrap();
        store.record("remem://one", Some("v1"), "blocked", "old snapshot");
        assert!(store.contains("remem://one", Some("v1")));
        assert!(!store.contains("remem://one", Some("v2")));
    }

    #[cfg(unix)]
    #[test]
    fn quarantine_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("queue.jsonl");
        let mut store = QuarantineStore::load_from(path.clone()).unwrap();
        store.record("remem://one", Some("v1"), "blocked", "private detail");
        store.save_if_dirty().unwrap();
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
