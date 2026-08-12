use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 各层目标阈值
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Targets {
    // 层1 认知深度
    pub dreyfus_green: f64,
    pub dreyfus_yellow: f64,
    pub decision_quality_green: f64,
    pub decision_quality_yellow: f64,
    // 层2 战略广度
    pub exploration_green: f64,
    pub exploration_yellow: f64,
    pub deep_invest_green_lo: f64,
    pub deep_invest_green_hi: f64,
    pub deep_invest_yellow_lo: f64,
    pub deep_invest_yellow_hi: f64,
    pub fragmentation_green: f64,
    pub fragmentation_yellow: f64,
    // 层3 协作效能
    pub delegation_green: f64,
    pub delegation_yellow: f64,
    pub mode_diversity_green: usize,
    pub mode_diversity_yellow: usize,
    pub bug_decision_green: f64,
    pub bug_decision_yellow: f64,
}

impl Default for Targets {
    fn default() -> Self {
        Self {
            dreyfus_green: 3.5,
            dreyfus_yellow: 2.5,
            decision_quality_green: 0.60,
            decision_quality_yellow: 0.40,
            exploration_green: 0.15,
            exploration_yellow: 0.08,
            deep_invest_green_lo: 0.15,
            deep_invest_green_hi: 0.30,
            deep_invest_yellow_lo: 0.10,
            deep_invest_yellow_hi: 0.30,
            fragmentation_green: 0.20,
            fragmentation_yellow: 0.40,
            delegation_green: 0.40,
            delegation_yellow: 0.55,
            mode_diversity_green: 4,
            mode_diversity_yellow: 2,
            bug_decision_green: 0.60,
            bug_decision_yellow: 0.80,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MirrorConfig {
    pub targets: Targets,
}

/// ~/.mirror/ 目录路径
pub fn mirror_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".mirror")
}

/// 确保 ~/.mirror/ 存在
pub fn ensure_mirror_dir() -> Result<PathBuf> {
    let dir = mirror_dir();
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// 加载配置，不存在则用默认值
pub fn load() -> MirrorConfig {
    let path = mirror_dir().join("config.toml");
    load_from_path(&path)
}

fn load_from_path(path: &Path) -> MirrorConfig {
    match std::fs::read_to_string(path) {
        Ok(content) => match toml::from_str(&content) {
            Ok(config) => config,
            Err(err) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %err,
                    "failed to parse mirror config, falling back to defaults"
                );
                MirrorConfig::default()
            }
        },
        Err(_) => MirrorConfig::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;
    use tracing_subscriber::fmt::MakeWriter;

    #[derive(Clone)]
    struct SharedWriter(Arc<Mutex<Vec<u8>>>);

    impl<'a> MakeWriter<'a> for SharedWriter {
        type Writer = SharedGuard;

        fn make_writer(&'a self) -> Self::Writer {
            SharedGuard(self.0.clone())
        }
    }

    struct SharedGuard(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for SharedGuard {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn default_targets_match_spec() {
        let t = Targets::default();
        assert!((t.dreyfus_green - 3.5).abs() < f64::EPSILON);
        assert!((t.decision_quality_green - 0.60).abs() < f64::EPSILON);
        assert!((t.delegation_green - 0.40).abs() < f64::EPSILON);
        assert_eq!(t.mode_diversity_green, 4);
    }

    #[test]
    fn load_missing_file_returns_default() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("missing.toml");
        let config = load_from_path(&path);
        assert!((config.targets.dreyfus_green - 3.5).abs() < f64::EPSILON);
    }

    #[test]
    fn deserialize_partial_config() {
        let toml_str = r#"
[targets]
dreyfus_green = 4.0
"#;
        let config: MirrorConfig = toml::from_str(toml_str).unwrap();
        assert!((config.targets.dreyfus_green - 4.0).abs() < f64::EPSILON);
        // 未指定字段使用默认值
        assert!((config.targets.dreyfus_yellow - 2.5).abs() < f64::EPSILON);
    }

    #[test]
    fn load_invalid_file_warns_and_returns_default() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[targets\ninvalid = 1\n").unwrap();

        let output = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .without_time()
            .with_target(false)
            .with_writer(SharedWriter(output.clone()))
            .finish();

        let config = tracing::subscriber::with_default(subscriber, || load_from_path(&path));
        assert!((config.targets.dreyfus_green - 3.5).abs() < f64::EPSILON);

        let logs = String::from_utf8(output.lock().unwrap().clone()).unwrap();
        assert!(logs.contains("failed to parse mirror config"));
        assert!(logs.contains("config.toml"));
    }
}
