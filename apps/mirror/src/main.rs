mod advice;
mod cli;
mod config;
mod dashboard;
mod document_save;
mod lang;
mod motd;
mod profile;
mod score;
mod weekly;

use anyhow::{Context, Result};
use clap::Parser;
use cli::{Cli, Commands};
use lang::Lang;
use refine_core::infra::{build_llm_client_from_env, ensure_db_dir, resolve_db_path, SqliteStore};
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    if let Err(error) = dotenvy::dotenv() {
        if should_report_dotenv_error(&error) {
            eprintln!("Warning: failed to load .env ({error})");
        }
    }

    tracing_subscriber::fmt()
        .with_env_filter("mirror=info,refine_core=info")
        .init();

    let cli = Cli::parse();

    let l: Lang = cli.lang.parse().map_err(|e: String| anyhow::anyhow!(e))?;
    lang::set_lang(l);

    let db_path = match &cli.db {
        Some(raw) => PathBuf::from(raw),
        None => resolve_db_path(&[]),
    };
    ensure_db_dir(&db_path).map_err(|e| anyhow::anyhow!(e))?;

    let store = Arc::new(SqliteStore::open(&db_path).context("Failed to open database")?);

    match cli.command {
        Commands::Score {
            since,
            all,
            require_advice,
        } => {
            let llm = build_llm_client_from_env();
            score::handle_score(store, llm, since, all, require_advice, &db_path).await
        }
        Commands::Motd => motd::handle_motd(),
        Commands::Dashboard { since, all } => dashboard::handle_dashboard(store, since, all).await,
        Commands::Weekly => weekly::handle_weekly(store.clone(), store).await,
        Commands::Profile => {
            let llm = build_llm_client_from_env().ok_or_else(|| {
                anyhow::anyhow!(
                    "profile requires LLM. Set REFINE_ANTHROPIC_API_KEY or REFINE_OPENAI_API_KEY"
                )
            })?;
            profile::handle_profile(store.clone(), store, llm).await
        }
    }
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
