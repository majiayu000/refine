mod cli;
mod config;
mod dashboard;
mod motd;
mod score;
mod weekly;

use anyhow::{Context, Result};
use clap::Parser;
use cli::{Cli, Commands};
use refine_core::infra::{build_llm_client_from_env, ensure_db_dir, resolve_db_path, SqliteStore};
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    if let Err(e) = dotenvy::dotenv() {
        eprintln!("提示: 未加载 .env 文件 ({})", e);
    }

    tracing_subscriber::fmt()
        .with_env_filter("mirror=info,refine_core=info")
        .init();

    let cli = Cli::parse();
    let db_path = match &cli.db {
        Some(raw) => PathBuf::from(raw),
        None => resolve_db_path(&[]),
    };
    ensure_db_dir(&db_path).map_err(|e| anyhow::anyhow!(e))?;

    let store = Arc::new(SqliteStore::open(&db_path).context("打开数据库失败")?);

    match cli.command {
        Commands::Score => score::handle_score(store).await,
        Commands::Motd => motd::handle_motd(),
        Commands::Dashboard => dashboard::handle_dashboard(store).await,
        Commands::Weekly => {
            let llm = build_llm_client_from_env()
                .ok_or_else(|| anyhow::anyhow!(
                    "weekly 需要 LLM，请配置 REFINE_ANTHROPIC_API_KEY 或 REFINE_OPENAI_API_KEY"
                ))?;
            weekly::handle_weekly(store.clone(), store, llm).await
        }
    }
}
