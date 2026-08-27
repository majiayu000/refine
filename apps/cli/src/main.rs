//! Refine CLI
//!
//! 知识管理命令行工具

mod cli;
mod handlers;
mod ingest_sessions;
mod insights;
mod insights_checkpoint;
mod insights_manifest;
mod remem_sessions;
mod repair_item_links;
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
    if let Err(error) = dotenvy::dotenv() {
        if should_report_dotenv_error(&error) {
            eprintln!("警告: 加载 .env 失败 ({error})");
        }
    }

    tracing_subscriber::fmt()
        .with_env_filter("refine=info,refine_core=info")
        .init();

    let cli = Cli::parse();
    let db_path = match &cli.db {
        Some(raw) => PathBuf::from(raw),
        None => resolve_db_path(&[]),
    };
    if cli.command.is_item_link_maintenance() {
        return repair_item_links::handle(&cli.command, &db_path);
    }
    let read_only_preview = cli.command.is_read_only_preview();
    if !read_only_preview {
        ensure_db_dir(&db_path).map_err(|e| anyhow::anyhow!(e))?;
    }
    if !read_only_preview {
        match migrate_stale_dbs(&db_path).map_err(anyhow::Error::msg)? {
            MigrationReport::NoOp => {}
            MigrationReport::Migrated {
                sources,
                rows_copied,
            } => {
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
        }
    }

    let store = Arc::new(if read_only_preview {
        SqliteStore::open_read_only(&db_path).context(
            "以只读方式打开数据库失败（dry-run 不会创建或迁移数据库；请先运行一次正式命令）",
        )?
    } else {
        SqliteStore::open(&db_path).context("打开数据库失败")?
    });
    let repo: Arc<dyn ItemRepository> = store.clone();
    let engine = Arc::new(SearchEngine::new(repo));

    handlers::run(cli.command, store, engine, &db_path).await
}

fn should_report_dotenv_error(error: &dotenvy::Error) -> bool {
    !error.not_found()
}

#[cfg(test)]
mod dotenv_tests {
    use super::should_report_dotenv_error;
    use std::io;

    #[test]
    fn missing_dotenv_is_silent_but_real_io_errors_are_reported() {
        let missing = dotenvy::Error::Io(io::ErrorKind::NotFound.into());
        let denied = dotenvy::Error::Io(io::ErrorKind::PermissionDenied.into());

        assert!(!should_report_dotenv_error(&missing));
        assert!(should_report_dotenv_error(&denied));
    }
}
