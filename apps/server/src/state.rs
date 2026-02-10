use refine_core::infra::{build_llm_client_from_env, LlmClient, SqliteStore};
use refine_core::knowledge::ItemRepository;
use refine_core::search::SearchEngine;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::models::{ConversationRecord, ExtractionJobRecord};
use crate::persistence::ServerPersistence;
use crate::vector_search::InMemoryVectorSearch;

pub struct AppState {
    pub store: Arc<dyn ItemRepository>,
    pub engine: Arc<SearchEngine>,
    pub semantic_search_enabled: bool,
    pub llm_client: Option<Arc<dyn LlmClient>>,
    pub api_token: Option<String>,
    pub persistence: Arc<ServerPersistence>,
    pub conversations: Arc<RwLock<HashMap<String, ConversationRecord>>>,
    pub idempotency: Arc<RwLock<HashMap<String, String>>>,
    pub jobs: Arc<RwLock<HashMap<String, ExtractionJobRecord>>>,
}

impl AppState {
    pub async fn build() -> Result<Self, String> {
        let db_path = get_db_path();
        ensure_db_dir(&db_path)?;
        let persistence = Arc::new(ServerPersistence::new(db_path.clone())?);

        let sqlite_store = Arc::new(SqliteStore::open(&db_path).map_err(|e| e.to_string())?);
        let store: Arc<dyn ItemRepository> = sqlite_store;
        let semantic_search_enabled = env_flag(&["REFINE_ENABLE_SEMANTIC_SEARCH"]);
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

        let conversation_vec = persistence.load_conversations()?;
        let job_vec = persistence.load_jobs()?;

        let mut conversations = HashMap::new();
        let mut idempotency = HashMap::new();
        for conversation in conversation_vec {
            idempotency.insert(
                conversation.idempotency_key.clone(),
                conversation.id.clone(),
            );
            conversations.insert(conversation.id.clone(), conversation);
        }

        let mut jobs = HashMap::new();
        for job in job_vec {
            jobs.insert(job.id.clone(), job);
        }

        Ok(Self {
            store,
            engine,
            semantic_search_enabled,
            llm_client: build_llm_client_from_env(),
            api_token: env_var(&["REFINE_API_TOKEN"]),
            persistence,
            conversations: Arc::new(RwLock::new(conversations)),
            idempotency: Arc::new(RwLock::new(idempotency)),
            jobs: Arc::new(RwLock::new(jobs)),
        })
    }
}

fn get_db_path() -> PathBuf {
    if let Some(raw) = env_var(&["REFINE_SERVER_DB_PATH"]) {
        return PathBuf::from(raw);
    }

    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("refine")
        .join("server.db")
}

fn ensure_db_dir(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn env_var(keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| std::env::var(key).ok())
        .filter(|value| !value.trim().is_empty())
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
