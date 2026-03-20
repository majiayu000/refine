use clap::{Parser, Subcommand};

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
        /// 来源过滤 (claude, codex)
        #[arg(long)]
        source: Option<String>,
        /// 限制处理数量
        #[arg(short, long)]
        limit: Option<usize>,
        /// 仅预览，不实际处理
        #[arg(long)]
        dry_run: bool,
    },
    /// 生成认知洞察报告
    Insights {
        /// 分析最近 N 天（默认全部）
        #[arg(short, long)]
        period: Option<usize>,
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
    /// 认知成长仪表盘
    Growth,
    /// 标记一次探索 session
    Explore,
    /// 标记一次深度思考 session
    DeepInquiry,
}
