use anyhow::{bail, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::fmt;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum IngestProvider {
    Auto,
    Remem,
    Local,
}

impl IngestProvider {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Remem => "remem",
            Self::Local => "local",
        }
    }

    pub(crate) fn resolve(explicit: Option<Self>, legacy_local_scan: bool) -> Result<Self> {
        if legacy_local_scan {
            if let Some(provider) = explicit {
                if provider != Self::Local {
                    bail!(
                        "--legacy-local-scan is a deprecated alias for --provider local and cannot be combined with --provider {}; use --provider local or remove the alias",
                        provider.as_str()
                    );
                }
            }
            return Ok(Self::Local);
        }

        Ok(explicit.unwrap_or(Self::Auto))
    }
}

impl fmt::Display for IngestProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Parser)]
#[command(name = "refine")]
#[command(about = "智能知识复用引擎 - 从 AI 对话中提炼知识", long_about = None)]
#[command(version)]
pub struct Cli {
    /// 数据库路径（默认使用与 server/desktop 相同的统一路径）
    #[arg(long)]
    pub db: Option<String>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum CognitivePortraitCommands {
    /// Collect a versioned current/previous evidence bundle without an LLM.
    Collect {
        /// Equal current/previous rolling window length.
        #[arg(long, default_value = "90")]
        period: usize,
        /// Fixed RFC3339 cutoff; defaults to the current UTC time.
        #[arg(long)]
        cutoff: Option<String>,
        /// Destination JSON bundle.
        #[arg(long)]
        output: PathBuf,
    },
    /// Validate a candidate portrait against its deterministic bundle.
    Validate {
        /// Versioned collector bundle.
        #[arg(long)]
        bundle: PathBuf,
        /// Candidate portrait markdown.
        #[arg(long)]
        portrait: PathBuf,
        /// Optional previous portrait for novelty/repetition checks.
        #[arg(long)]
        previous: Option<PathBuf>,
        /// Destination quality report JSON (written on pass and failure).
        #[arg(long)]
        output: PathBuf,
    },
}

#[derive(Subcommand)]
pub enum Commands {
    /// 从对话中提炼知识
    Extract {
        /// 从标准输入读取
        #[arg(long)]
        stdin: bool,
    },
    /// 搜索知识
    Search {
        /// 搜索关键词
        query: String,
        /// 限制结果数量
        #[arg(short, long, default_value = "10")]
        limit: usize,
    },
    /// 列出所有知识
    List {
        /// 按类型过滤 (knowledge, skill, snippet)
        #[arg(short, long)]
        r#type: Option<String>,
        /// 限制数量
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },
    /// 显示知识详情
    Show {
        /// 知识 ID
        id: String,
    },
    /// 删除知识
    Delete {
        /// 知识 ID
        id: String,
    },
    /// 添加知识
    Add {
        /// 标题
        #[arg(short, long)]
        title: String,
        /// 摘要
        #[arg(short, long)]
        summary: String,
        /// 类型 (knowledge, skill, snippet)
        #[arg(long, default_value = "knowledge")]
        r#type: String,
    },
    /// 列出所有文档
    Docs {
        /// 限制数量
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },
    /// 显示文档详情及关联知识
    DocShow {
        /// 文档 ID
        id: String,
    },
    /// 从 AI 编程会话中提取认知观测
    IngestSessions {
        /// Local scanner source filter (claude, codex); requires --provider local.
        #[arg(long)]
        source: Option<String>,
        /// Session provider: auto (default), remem, or local.
        #[arg(long, value_enum, value_name = "PROVIDER")]
        provider: Option<IngestProvider>,
        /// 限制处理数量（按路径顺序取前 N 个），与 --latest 互斥
        #[arg(short, long, conflicts_with = "latest")]
        limit: Option<usize>,
        /// 仅处理最近修改的 N 个会话（按 mtime 降序），与 --limit 互斥
        #[arg(long, conflicts_with = "limit")]
        latest: Option<usize>,
        /// 仅预览，不实际处理
        #[arg(long)]
        dry_run: bool,
        /// Deprecated alias for --provider local.
        #[arg(long)]
        legacy_local_scan: bool,
        /// Explicitly retry sessions previously quarantined for deterministic provider rejection.
        #[arg(long)]
        retry_quarantined: bool,
        /// Reconcile Codex provenance tags on existing observations without an LLM call.
        #[arg(long)]
        backfill_session_metadata: bool,
    },
    /// 生成认知洞察报告
    Insights {
        /// 分析最近 N 天并比较前一等长窗口（默认 7 天）
        #[arg(short, long, conflicts_with = "all")]
        period: Option<usize>,
        /// 显式生成全历史 snapshot；不输出跨期趋势
        #[arg(long, conflicts_with = "period")]
        all: bool,
        /// 生成 L4 处方（需要 LLM）
        #[arg(long)]
        prescription: bool,
    },
    /// Deterministic cognitive portrait data and quality operations.
    CognitivePortrait {
        #[command(subcommand)]
        command: CognitivePortraitCommands,
    },
    /// Audit historical session observations that have no Document link.
    AuditItemLinks {
        /// Expected deployed detached-row baseline; requires --cutoff.
        #[arg(long)]
        baseline_detached_count: Option<u64>,
        /// RFC3339 deployment cutoff; requires --baseline-detached-count.
        #[arg(long)]
        cutoff: Option<String>,
    },
    /// Repair the exact historical shadow-Document-ID subset (dry-run by default).
    RepairItemLinks {
        /// Immutable historical SQLite evidence file.
        #[arg(long)]
        evidence: String,
        /// Expected SHA-256 of the evidence file.
        #[arg(long)]
        evidence_sha256: String,
        /// Apply the proven plan. Without this flag, no file is changed.
        #[arg(long)]
        apply: bool,
        /// New SQLite backup path. Required with --apply and must not exist.
        #[arg(long)]
        backup: Option<String>,
    },
    /// 搜索文档原文
    DocSearch {
        /// 搜索关键词
        query: String,
        /// 限制结果数量
        #[arg(short, long, default_value = "10")]
        limit: usize,
    },
    /// Removed — use 'mirror dashboard' instead
    #[command(hide = true)]
    Growth,
    /// Removed — use 'mirror score' instead
    #[command(hide = true)]
    Explore,
    /// Removed — use 'mirror score' instead
    #[command(hide = true)]
    DeepInquiry,
}

impl Commands {
    pub(crate) fn is_read_only_preview(&self) -> bool {
        matches!(
            self,
            Self::IngestSessions { dry_run: true, .. }
                | Self::AuditItemLinks { .. }
                | Self::RepairItemLinks { apply: false, .. }
                | Self::CognitivePortrait {
                    command: CognitivePortraitCommands::Collect { .. }
                }
        )
    }

    pub(crate) fn is_item_link_maintenance(&self) -> bool {
        matches!(
            self,
            Self::AuditItemLinks { .. } | Self::RepairItemLinks { .. }
        )
    }

    pub(crate) fn is_cognitive_portrait_validation(&self) -> bool {
        matches!(
            self,
            Self::CognitivePortrait {
                command: CognitivePortraitCommands::Validate { .. }
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ingest_command(args: &[&str]) -> (Option<IngestProvider>, bool) {
        let cli = Cli::try_parse_from(args).expect("CLI arguments should parse");
        let Commands::IngestSessions {
            provider,
            legacy_local_scan,
            ..
        } = cli.command
        else {
            panic!("expected ingest-sessions command");
        };
        (provider, legacy_local_scan)
    }

    #[test]
    fn ingest_provider_defaults_to_auto() {
        let (provider, legacy_local_scan) = ingest_command(&["refine", "ingest-sessions"]);
        assert_eq!(
            IngestProvider::resolve(provider, legacy_local_scan).unwrap(),
            IngestProvider::Auto
        );
    }

    #[test]
    fn ingest_provider_accepts_all_first_class_values() {
        for (value, expected) in [
            ("auto", IngestProvider::Auto),
            ("remem", IngestProvider::Remem),
            ("local", IngestProvider::Local),
        ] {
            let (provider, legacy_local_scan) =
                ingest_command(&["refine", "ingest-sessions", "--provider", value]);
            assert_eq!(
                IngestProvider::resolve(provider, legacy_local_scan).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn legacy_local_scan_remains_a_local_alias_and_rejects_contradictions() {
        let (provider, legacy_local_scan) = ingest_command(&[
            "refine",
            "ingest-sessions",
            "--legacy-local-scan",
            "--source",
            "codex",
        ]);
        assert_eq!(
            IngestProvider::resolve(provider, legacy_local_scan).unwrap(),
            IngestProvider::Local
        );

        let (provider, legacy_local_scan) = ingest_command(&[
            "refine",
            "ingest-sessions",
            "--provider",
            "remem",
            "--legacy-local-scan",
        ]);
        let error = IngestProvider::resolve(provider, legacy_local_scan)
            .expect_err("remem and the local alias contradict each other");
        assert!(error.to_string().contains("--provider local"));
    }

    #[test]
    fn insights_requires_explicit_all_for_full_history() {
        let cli = Cli::try_parse_from(["refine", "insights"]).unwrap();
        let Commands::Insights { period, all, .. } = cli.command else {
            panic!("expected insights");
        };
        assert_eq!(period.unwrap_or(7), 7);
        assert!(!all);

        let cli = Cli::try_parse_from(["refine", "insights", "--all"]).unwrap();
        let Commands::Insights { all, .. } = cli.command else {
            panic!("expected insights");
        };
        assert!(all);
        assert!(Cli::try_parse_from(["refine", "insights", "--all", "--period", "7"]).is_err());
    }

    #[test]
    fn cognitive_portrait_collect_defaults_to_rolling_ninety_days() {
        let cli = Cli::try_parse_from([
            "refine",
            "cognitive-portrait",
            "collect",
            "--output",
            "bundle.json",
        ])
        .unwrap();
        let Commands::CognitivePortrait {
            command:
                CognitivePortraitCommands::Collect {
                    period,
                    cutoff,
                    output,
                },
        } = cli.command
        else {
            panic!("expected cognitive portrait collect");
        };
        assert_eq!(period, 90);
        assert!(cutoff.is_none());
        assert_eq!(output, PathBuf::from("bundle.json"));
    }

    #[test]
    fn cognitive_portrait_validate_is_storage_independent() {
        let cli = Cli::try_parse_from([
            "refine",
            "cognitive-portrait",
            "validate",
            "--bundle",
            "bundle.json",
            "--portrait",
            "portrait.md",
            "--output",
            "quality.json",
        ])
        .unwrap();
        assert!(cli.command.is_cognitive_portrait_validation());
        assert!(!cli.command.is_item_link_maintenance());
    }
}
