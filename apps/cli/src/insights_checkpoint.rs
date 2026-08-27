use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use refine_core::session::{DataQualityStats, RouteResult};
use serde::{Deserialize, Serialize};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

const CHECKPOINT_ENV: &str = "REFINE_INSIGHTS_CHECKPOINT_PATH";
pub(crate) const CHECKPOINT_VERSION: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DatasetSignature {
    pub checkpoint_version: u32,
    pub observation_count: usize,
    pub latest_updated_at: DateTime<Utc>,
    pub with_prescription: bool,
    #[serde(default)]
    pub period_days: Option<usize>,
    #[serde(default)]
    pub llm_identity: String,
    #[serde(default)]
    pub prompt_identity: String,
    #[serde(default)]
    pub data_quality: DataQualityStats,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct InsightsCheckpoint {
    pub signature: DatasetSignature,
    pub route_results: Vec<RouteResult>,
    pub updated_at: DateTime<Utc>,
}

impl InsightsCheckpoint {
    pub(crate) fn load_matching(signature: DatasetSignature) -> Result<Self> {
        let path = checkpoint_path()?;
        Self::load_matching_from(&path, signature)
    }

    fn load_matching_from(path: &Path, signature: DatasetSignature) -> Result<Self> {
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::empty(signature));
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("读取 insights checkpoint 失败: {}", path.display()));
            }
        };
        let checkpoint: Self = serde_json::from_str(&content).with_context(|| {
            format!("insights checkpoint 损坏，拒绝静默忽略: {}", path.display())
        })?;
        if checkpoint.signature == signature {
            Ok(checkpoint)
        } else {
            Ok(Self::empty(signature))
        }
    }

    pub(crate) fn empty(signature: DatasetSignature) -> Self {
        Self {
            signature,
            route_results: Vec::new(),
            updated_at: Utc::now(),
        }
    }

    pub(crate) fn contains_route(&self, route_id: usize) -> bool {
        self.route_results
            .iter()
            .any(|result| result.route_id == route_id)
    }

    pub(crate) fn extend(&mut self, results: impl IntoIterator<Item = RouteResult>) {
        for result in results {
            self.route_results
                .retain(|existing| existing.route_id != result.route_id);
            self.route_results.push(result);
        }
        self.route_results.sort_by_key(|result| result.route_id);
        self.updated_at = Utc::now();
    }

    pub(crate) fn save(&self) -> Result<()> {
        let path = checkpoint_path()?;
        atomic_write_json(&path, self)
    }

    pub(crate) fn clear() -> Result<()> {
        let path = checkpoint_path()?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error)
                .with_context(|| format!("删除 insights checkpoint 失败: {}", path.display())),
        }
    }

    pub(crate) fn path() -> Result<PathBuf> {
        checkpoint_path()
    }
}

fn checkpoint_path() -> Result<PathBuf> {
    if let Ok(path) = std::env::var(CHECKPOINT_ENV) {
        if !path.trim().is_empty() {
            return Ok(PathBuf::from(path));
        }
    }
    let home = dirs::home_dir().context("无法定位 HOME，不能保存 insights checkpoint")?;
    Ok(home.join(".refine").join("insights-checkpoint.json"))
}

fn atomic_write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("checkpoint 路径没有父目录: {}", path.display()))?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("创建 checkpoint 目录失败: {}", parent.display()))?;
    secure_default_parent(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("insights-checkpoint.json");
    let temp_path = parent.join(format!(".{file_name}.{}.tmp", std::process::id()));
    let result = (|| -> Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options
            .open(&temp_path)
            .with_context(|| format!("创建 checkpoint 临时文件失败: {}", temp_path.display()))?;
        serde_json::to_writer_pretty(&mut file, value)
            .context("序列化 insights checkpoint 失败")?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        std::fs::rename(&temp_path, path).with_context(|| {
            format!(
                "原子替换 insights checkpoint 失败: {} -> {}",
                temp_path.display(),
                path.display()
            )
        })?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    result
}

fn secure_default_parent(parent: &Path) -> Result<()> {
    #[cfg(unix)]
    if dirs::home_dir().as_deref().map(|home| home.join(".refine")) == Some(parent.to_path_buf()) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("收紧 checkpoint 目录权限失败: {}", parent.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoint_replaces_route_by_id() {
        let signature = DatasetSignature {
            checkpoint_version: CHECKPOINT_VERSION,
            observation_count: 1,
            latest_updated_at: Utc::now(),
            with_prescription: true,
            period_days: None,
            llm_identity: "test:model-a:endpoint-a".into(),
            prompt_identity: "insights:test-v1".into(),
            data_quality: DataQualityStats::default(),
        };
        let mut checkpoint = InsightsCheckpoint::empty(signature);
        checkpoint.extend([RouteResult {
            route_id: 1,
            route_title: "first".into(),
            content: "old".into(),
        }]);
        checkpoint.extend([RouteResult {
            route_id: 1,
            route_title: "first".into(),
            content: "new".into(),
        }]);
        assert_eq!(checkpoint.route_results.len(), 1);
        assert_eq!(checkpoint.route_results[0].content, "new");
    }

    #[test]
    fn checkpoint_round_trip_requires_matching_signature() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("checkpoint.json");
        let signature = DatasetSignature {
            checkpoint_version: CHECKPOINT_VERSION,
            observation_count: 3,
            latest_updated_at: Utc::now(),
            with_prescription: true,
            period_days: None,
            llm_identity: "test:model-a:endpoint-a".into(),
            prompt_identity: "insights:test-v1".into(),
            data_quality: DataQualityStats::default(),
        };
        let mut checkpoint = InsightsCheckpoint::empty(signature.clone());
        checkpoint.extend([RouteResult {
            route_id: 4,
            route_title: "route".into(),
            content: "done".into(),
        }]);
        atomic_write_json(&path, &checkpoint).unwrap();

        let loaded = InsightsCheckpoint::load_matching_from(&path, signature.clone()).unwrap();
        assert!(loaded.contains_route(4));

        let mismatch = DatasetSignature {
            observation_count: 4,
            ..signature.clone()
        };
        let reset = InsightsCheckpoint::load_matching_from(&path, mismatch).unwrap();
        assert!(reset.route_results.is_empty());

        let llm_mismatch = DatasetSignature {
            llm_identity: "test:model-b:endpoint-b".into(),
            ..signature
        };
        let reset = InsightsCheckpoint::load_matching_from(&path, llm_mismatch).unwrap();
        assert!(reset.route_results.is_empty());

        let quality_mismatch = DatasetSignature {
            data_quality: DataQualityStats {
                input_observations: 3,
                linked_observations: 2,
                detached_observations: 1,
                mode_excluded_observations: 0,
                eligible_observations: 2,
                cohort_identity: "sha256:changed".into(),
            },
            ..checkpoint.signature.clone()
        };
        let reset = InsightsCheckpoint::load_matching_from(&path, quality_mismatch).unwrap();
        assert!(reset.route_results.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn checkpoint_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("checkpoint.json");
        let signature = DatasetSignature {
            checkpoint_version: CHECKPOINT_VERSION,
            observation_count: 1,
            latest_updated_at: Utc::now(),
            with_prescription: false,
            period_days: Some(30),
            llm_identity: "private-provider".into(),
            prompt_identity: "prompt".into(),
            data_quality: DataQualityStats::default(),
        };
        atomic_write_json(&path, &InsightsCheckpoint::empty(signature)).unwrap();
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
