use refine_core::infra::{ClaudeClient, LlmClient, OpenAIClient, SqliteStore};
use refine_core::knowledge::ItemRepository;
use refine_core::search::SearchEngine;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::models::{ConversationRecord, ExtractionJobRecord};
use crate::persistence::ServerPersistence;

pub struct AppState {
    pub store: Arc<SqliteStore>,
    pub engine: Arc<SearchEngine>,
    pub llm_client: Option<Arc<dyn LlmClient>>,
    pub api_token: Option<String>,
    pub persistence: Arc<ServerPersistence>,
    pub conversations: Arc<RwLock<HashMap<String, ConversationRecord>>>,
    pub idempotency: Arc<RwLock<HashMap<String, String>>>,
    pub jobs: Arc<RwLock<HashMap<String, ExtractionJobRecord>>>,
}

impl AppState {
    pub fn build() -> Result<Self, String> {
        let db_path = get_db_path();
        ensure_db_dir(&db_path)?;
        let persistence = Arc::new(ServerPersistence::new(db_path.clone())?);

        let store = Arc::new(SqliteStore::open(&db_path).map_err(|e| e.to_string())?);
        let repo: Arc<dyn ItemRepository> = store.clone();
        let engine = Arc::new(SearchEngine::new(repo));

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

fn build_llm_client_from_env() -> Option<Arc<dyn LlmClient>> {
    if let Some(api_key) = env_var(&["REFINE_ANTHROPIC_API_KEY", "ANTHROPIC_API_KEY"]) {
        let mut client = ClaudeClient::new(&api_key);
        if let Some(model) = env_var(&["REFINE_ANTHROPIC_MODEL"]) {
            client = client.with_model(&model);
        }
        if let Some(base_url) = env_var(&["REFINE_ANTHROPIC_BASE_URL"]) {
            client = client.with_base_url(&base_url);
        }
        return Some(Arc::new(client));
    }

    if let Some(api_key) = env_var(&["REFINE_OPENAI_API_KEY", "OPENAI_API_KEY"]) {
        let mut client = OpenAIClient::new(&api_key);
        if let Some(model) = env_var(&["REFINE_OPENAI_MODEL"]) {
            client = client.with_model(&model);
        }
        if let Some(base_url) = env_var(&["REFINE_OPENAI_BASE_URL"]) {
            client = client.with_base_url(&base_url);
        }
        return Some(Arc::new(client));
    }

    None
}

fn env_var(keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| std::env::var(key).ok())
        .filter(|value| !value.trim().is_empty())
}
