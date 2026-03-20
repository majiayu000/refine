use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "mirror")]
#[command(about = "认知镜像 — 从 AI 编程会话中提取认知指纹", long_about = None)]
#[command(version)]
pub struct Cli {
    /// 数据库路径（默认使用与 refine 相同的统一路径）
    #[arg(long)]
    pub db: Option<String>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// 计算 3 层信号灯 + 9 个子指标 + 张力分析
    Score,
    /// 一行简报（适合加到 .zshrc）
    Motd,
    /// 完整 ASCII 仪表盘
    Dashboard,
    /// 本周 vs 上周差量分析
    Weekly,
}
