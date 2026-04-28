//! Refine CLI
//!
//! 知识管理命令行工具

mod cli;
mod handlers;
mod ingest_sessions;
mod insights;
mod support;

use anyhow::{Context, Result};
use clap::Parser;
use cli::Cli;
use refine_core::infra::{
    ensure_db_dir, migrate_stale_dbs, resolve_db_path, MigrationReport, SqliteStore,
};
use refine_core::knowledge::ItemRepository;
use refine_core::search::SearchEngine;
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    if let Err(e) = dotenvy::dotenv() {
        eprintln!("提示: 未加载 .env 文件 ({})", e);
    }

    tracing_subscriber::fmt()
        .with_env_filter("refine=info,refine_core=info")
        .init();

    let cli = Cli::parse();
    let db_path = match &cli.db {
        Some(raw) => PathBuf::from(raw),
        None => resolve_db_path(&[]),
    };
    ensure_db_dir(&db_path).map_err(|e| anyhow::anyhow!(e))?;
    match migrate_stale_dbs(&db_path) {
        Ok(MigrationReport::NoOp) => {}
        Ok(MigrationReport::Migrated {
            sources,
            rows_copied,
        }) => {
            eprintln!(
                "[refine] migrated {} row(s) from legacy DB(s): {}",
                rows_copied,
                sources
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        Err(e) => eprintln!("[refine] warning: DB migration failed (continuing): {e}"),
    }

    let store = Arc::new(SqliteStore::open(&db_path).context("打开数据库失败")?);
    let authoritative_db_path = std::fs::canonicalize(&db_path).unwrap_or_else(|_| db_path.clone());
    let repo: Arc<dyn ItemRepository> = store.clone();
    let engine = Arc::new(SearchEngine::new(repo));

    handlers::run(cli.command, store, engine, authoritative_db_path).await
}
