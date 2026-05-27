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
        let persistence = Arc::new(ServerPersistence::new(db_path.clone())?);
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
                    return Err(format!(
                        "failed to bootstrap semantic index for item {}: {}",
                        item.id(),
                        err
                    ));
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

#[cfg(test)]
mod tests {
    use super::AppState;
    use rusqlite::Connection;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use uuid::Uuid;

    // These tests mutate process-global env vars, so they must run one at a time.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard(Vec<(&'static str, Option<String>)>);

    impl EnvGuard {
        fn capture(keys: &[&'static str]) -> Self {
            Self(
                keys.iter()
                    .map(|key| (*key, std::env::var(key).ok()))
                    .collect(),
            )
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in &self.0 {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    fn make_temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("refine-app-state-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn cleanup_dir(path: &Path) {
        let _ = std::fs::remove_dir_all(path);
    }

    fn seed_legacy_server_db(path: &Path) {
        let conn = Connection::open(path).expect("open legacy db");
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS conversations (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                source TEXT NOT NULL,
                url TEXT NOT NULL,
                title TEXT,
                raw_content TEXT NOT NULL,
                metadata_json TEXT NOT NULL DEFAULT '{}',
                captured_at TEXT NOT NULL,
                created_at TEXT NOT NULL,
                status TEXT NOT NULL,
                idempotency_key TEXT NOT NULL UNIQUE,
                item_ids TEXT NOT NULL,
                last_error TEXT
            );

            CREATE TABLE IF NOT EXISTS events (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                event_name TEXT NOT NULL,
                source TEXT NOT NULL,
                properties_json TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL
            );
            "#,
        )
        .expect("create legacy schema");
        conn.execute(
            r#"
            INSERT INTO conversations
              (id, user_id, source, url, title, raw_content, metadata_json,
               captured_at, created_at, status, idempotency_key, item_ids, last_error)
            VALUES
              (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            "#,
            (
                "conv-1",
                "user-1",
                "extension",
                "https://example.com/conversation",
                "Legacy conversation",
                "legacy content",
                "{}",
                "2026-01-01T00:00:00Z",
                "2026-01-01T00:00:00Z",
                "captured",
                "legacy-key",
                "[]",
                Option::<String>::None,
            ),
        )
        .expect("insert legacy conversation");
        conn.execute(
            r#"
            INSERT INTO events
              (id, user_id, event_name, source, properties_json, created_at)
            VALUES
              (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            (
                "event-1",
                "user-1",
                "conversation_captured",
                "extension",
                "{}",
                "2026-01-01T00:00:00Z",
            ),
        )
        .expect("insert legacy event");
    }

    #[test]
    fn build_migrates_server_owned_tables_after_schema_init() {
        let _env_lock = ENV_LOCK.lock().expect("lock env");
        let _env_guard = EnvGuard::capture(&[
            "REFINE_DB_PATH",
            "REFINE_SERVER_DB_PATH",
            "REFINE_ENV",
            "APP_ENV",
            "REFINE_API_TOKEN",
        ]);

        let dir = make_temp_dir();
        let db_path = dir.join("refine.db");
        let legacy_path = dir.join("server.db");
        seed_legacy_server_db(&legacy_path);

        std::env::remove_var("REFINE_DB_PATH");
        std::env::set_var("REFINE_SERVER_DB_PATH", db_path.to_string_lossy().as_ref());
        std::env::remove_var("REFINE_ENV");
        std::env::remove_var("APP_ENV");
        std::env::remove_var("REFINE_API_TOKEN");

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");
        let state = runtime
            .block_on(AppState::build())
            .expect("build app state");
        drop(state);

        let conn = Connection::open(&db_path).expect("open migrated db");
        let conversation_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM conversations WHERE id = 'conv-1'",
                [],
                |row| row.get(0),
            )
            .expect("count migrated conversations");
        let event_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM events WHERE id = 'event-1'",
                [],
                |row| row.get(0),
            )
            .expect("count migrated events");

        assert_eq!(conversation_count, 1);
        assert_eq!(event_count, 1);
        assert!(legacy_path.with_extension("db.migrated").exists());
        assert!(!legacy_path.exists());

        cleanup_dir(&dir);
    }
}
