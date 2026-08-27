use anyhow::{bail, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::fmt;

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
        matches!(self, Self::IngestSessions { dry_run: true, .. })
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
}
