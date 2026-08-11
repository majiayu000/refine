use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

const QUARANTINE_ENV: &str = "REFINE_INGEST_QUARANTINE_PATH";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct QuarantineRecord {
    pub url: String,
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
}

impl QuarantineStore {
    pub(super) fn load() -> Result<Self> {
        Self::load_from(default_path()?)
    }

    pub(super) fn load_from(path: PathBuf) -> Result<Self> {
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
            records.insert(record.url.clone(), record);
        }

        Ok(Self {
            path,
            records,
            dirty: false,
        })
    }

    pub(super) fn contains(&self, url: &str) -> bool {
        self.records.contains_key(url)
    }

    pub(super) fn len(&self) -> usize {
        self.records.len()
    }

    pub(super) fn count_matching(&self, urls: &HashSet<String>) -> usize {
        urls.iter()
            .filter(|url| self.records.contains_key(url.as_str()))
            .count()
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn record(&mut self, url: &str, code: &str, message: &str) {
        let now = Utc::now();
        match self.records.get_mut(url) {
            Some(record) => {
                record.code = code.to_string();
                record.message = message.to_string();
                record.last_seen = now;
                record.attempts = record.attempts.saturating_add(1);
            }
            None => {
                self.records.insert(
                    url.to_string(),
                    QuarantineRecord {
                        url: url.to_string(),
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
        if self.records.remove(url).is_some() {
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

        let file_name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("ingest-quarantine.jsonl");
        let temp_path = parent.join(format!(".{file_name}.{}.tmp", std::process::id()));
        let write_result = (|| -> Result<()> {
            let mut file = std::fs::File::create(&temp_path).with_context(|| {
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
        store.record("remem://one", "sensitive_words_detected", "blocked");
        store.record("remem://one", "sensitive_words_detected", "blocked again");
        store.save_if_dirty().unwrap();

        let loaded = QuarantineStore::load_from(path).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded.records["remem://one"].attempts, 2);
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
        store.record("remem://selected", "blocked", "selected");
        store.record("file://unrelated", "blocked", "unrelated");

        let selected = HashSet::from(["remem://selected".to_string()]);
        assert_eq!(store.count_matching(&selected), 1);

        let other_provider = HashSet::from(["remem://other".to_string()]);
        assert_eq!(store.count_matching(&other_provider), 0);
        assert_eq!(store.len(), 2);
    }
}
