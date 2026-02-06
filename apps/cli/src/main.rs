//! Refine CLI
//!
//! 知识管理命令行工具

mod cli;
mod handlers;
mod support;

use anyhow::{Context, Result};
use clap::Parser;
use cli::Cli;
use refine_core::infra::SqliteStore;
use refine_core::knowledge::ItemRepository;
use refine_core::search::SearchEngine;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("refine=info,refine_core=info")
        .init();

    let cli = Cli::parse();
    let db_path = support::get_db_path(&cli.db);
    support::ensure_db_dir(&db_path)?;

    let store = Arc::new(SqliteStore::open(&db_path).context("打开数据库失败")?);
    let repo: Arc<dyn ItemRepository> = store.clone();
    let engine = Arc::new(SearchEngine::new(repo));

    handlers::run(cli.command, store, engine).await
}
