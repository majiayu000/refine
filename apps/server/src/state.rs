use refine_core::infra::{
    build_llm_client_from_env, ensure_db_dir, migrate_stale_dbs, resolve_db_path, LlmClient,
    MigrationReport, SqliteStore,
};
use refine_core::knowledge::{DocumentRepository, ItemRepository};
use refine_core::search::SearchEngine;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use crate::application::ports::{ConversationRepository, EventRepository, JobRepository};
use crate::persistence::ServerPersistence;
use crate::vector_search::InMemoryVectorSearch;

pub struct AppState {
    pub store: Arc<dyn ItemRepository>,
    pub doc_store: Arc<dyn DocumentRepository>,
    pub engine: Arc<SearchEngine>,
    pub semantic_search_enabled: bool,
    pub free_quota_items: usize,
    pub premium_users: HashSet<String>,
    pub llm_client: Option<Arc<dyn LlmClient>>,
    pub api_token: Option<String>,
    /// Explicit opt-in for unauthenticated local access. Requires `REFINE_DEV_ANON=1`.
    pub dev_anon: bool,
    pub conversation_repo: Arc<dyn ConversationRepository>,
    pub job_repo: Arc<dyn JobRepository>,
    pub event_repo: Arc<dyn EventRepository>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthConfig {
    pub api_token: Option<String>,
    pub dev_anon: bool,
}

impl AuthConfig {
    pub fn from_env() -> Result<Self, String> {
        let api_token = env_var(&["REFINE_API_TOKEN"]);
        let dev_anon = env_flag(&["REFINE_DEV_ANON"]);
        if is_production_env() && api_token.is_none() {
            return Err(
                "REFINE_API_TOKEN is required when REFINE_ENV is set to production".to_string(),
            );
        }

        Ok(Self {
            api_token,
            dev_anon,
        })
    }
}

struct AppStateConfig {
    db_path: PathBuf,
    semantic_search_enabled: bool,
    free_quota_items: usize,
    premium_users: HashSet<String>,
    llm_client: Option<Arc<dyn LlmClient>>,
    auth: AuthConfig,
}

impl AppStateConfig {
    fn from_env() -> Result<Self, String> {
        Ok(Self {
            db_path: resolve_db_path(&["REFINE_SERVER_DB_PATH"]),
            semantic_search_enabled: env_flag(&["REFINE_ENABLE_SEMANTIC_SEARCH"]),
            free_quota_items: env_usize(&["REFINE_MAX_ITEMS", "REFINE_FREE_QUOTA_ITEMS"])
                .unwrap_or(0),
            premium_users: env_csv_set(&["REFINE_PREMIUM_USERS"])
                .unwrap_or_else(default_premium_users),
            llm_client: build_llm_client_from_env(),
            auth: AuthConfig::from_env()?,
        })
    }

    #[cfg(test)]
    fn for_test(db_path: PathBuf, auth: AuthConfig) -> Self {
        Self {
            db_path,
            semantic_search_enabled: false,
            free_quota_items: 0,
            premium_users: default_premium_users(),
            llm_client: None,
            auth,
        }
    }
}

impl AppState {
    pub async fn build() -> Result<Self, String> {
        Self::build_with_config(AppStateConfig::from_env()?).await
    }

    async fn build_with_config(config: AppStateConfig) -> Result<Self, String> {
        let AppStateConfig {
            db_path,
            semantic_search_enabled,
            free_quota_items,
            premium_users,
            llm_client,
            auth,
        } = config;

        ensure_db_dir(&db_path)?;
        match migrate_stale_dbs(&db_path) {
            Ok(MigrationReport::NoOp) => {}
            Ok(MigrationReport::Migrated {
                sources,
                rows_copied,
            }) => {
                tracing::info!(
                    rows_copied,
                    sources = ?sources,
                    "migrated legacy DB(s) into primary database"
                );
            }
            Err(e) => tracing::warn!("DB migration failed (continuing): {}", e),
        }
        let persistence = Arc::new(ServerPersistence::new(db_path.clone())?);
        let conversation_repo: Arc<dyn ConversationRepository> = persistence.clone();
        let job_repo: Arc<dyn JobRepository> = persistence.clone();
        let event_repo: Arc<dyn EventRepository> = persistence.clone();

        let sqlite_store = Arc::new(SqliteStore::open(&db_path).map_err(|e| e.to_string())?);
        let store: Arc<dyn ItemRepository> = sqlite_store.clone();
        let doc_store: Arc<dyn DocumentRepository> = sqlite_store;
        let mut engine_builder = SearchEngine::new(store.clone());
        if semantic_search_enabled {
            engine_builder =
                engine_builder.with_vector_search(Arc::new(InMemoryVectorSearch::new()));
        }
        let engine = Arc::new(engine_builder);

        if semantic_search_enabled {
            let existing_items = store
                .find_all()
                .await
                .map_err(|e| format!("failed to bootstrap semantic index: {}", e))?;
            for item in &existing_items {
                if let Err(err) = engine.index_item(item).await {
                    tracing::warn!(
                        "semantic index bootstrap failed for item {}: {}",
                        item.id(),
                        err
                    );
                }
            }
            tracing::info!(
                "semantic search enabled, bootstrapped {} items into vector index",
                existing_items.len()
            );
        }

        Ok(Self {
            store,
            doc_store,
            engine,
            semantic_search_enabled,
            free_quota_items,
            premium_users,
            llm_client,
            api_token: auth.api_token,
            dev_anon: auth.dev_anon,
            conversation_repo,
            job_repo,
            event_repo,
        })
    }

    #[cfg(test)]
    pub async fn build_for_test(db_path: PathBuf, auth: AuthConfig) -> Result<Self, String> {
        Self::build_with_config(AppStateConfig::for_test(db_path, auth)).await
    }
}

impl AppState {
    pub fn is_premium_user(&self, user_id: &str) -> bool {
        let normalized = user_id.trim();
        !normalized.is_empty() && self.premium_users.contains(normalized)
    }
}

fn env_var(keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| std::env::var(key).ok())
        .filter(|value| !value.trim().is_empty())
}

fn env_csv_set(keys: &[&str]) -> Option<HashSet<String>> {
    for key in keys {
        if let Ok(raw) = std::env::var(key) {
            let set = raw
                .split(',')
                .map(|entry| entry.trim().to_string())
                .filter(|entry| !entry.is_empty())
                .collect::<HashSet<_>>();
            return Some(set);
        }
    }
    None
}

fn default_premium_users() -> HashSet<String> {
    HashSet::from(["dev-user".to_string(), "token-user".to_string()])
}

fn env_flag(keys: &[&str]) -> bool {
    let Some(raw) = env_var(keys) else {
        return false;
    };
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on" | "enabled"
    )
}

fn env_usize(keys: &[&str]) -> Option<usize> {
    env_var(keys).and_then(|raw| raw.trim().parse::<usize>().ok())
}

fn is_production_env() -> bool {
    let Some(raw) = env_var(&["REFINE_ENV", "APP_ENV"]) else {
        return false;
    };

    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "prod" | "production"
    )
}
