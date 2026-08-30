use refine_core::infra::{DocumentDetailDto, DocumentDto};
use refine_core::search::SearchQuery as CoreSearchQuery;
use serde::Serialize;
use std::sync::Arc;

use crate::application::error::ApplicationErrorCode;
use crate::models::{
    ConversationDto, ItemDto, ListConversationsQuery, ListItemsQuery, SearchQuery,
};
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct ListConversationsResult {
    pub conversations: Vec<ConversationDto>,
    pub total: usize,
    pub next_cursor: Option<usize>,
}

pub async fn list_conversations(
    state: Arc<AppState>,
    query: ListConversationsQuery,
) -> Result<ListConversationsResult, QueryError> {
    let cursor = query.cursor.unwrap_or(0);
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let status_filter = query
        .status
        .map(|status| status.trim().to_ascii_lowercase())
        .filter(|status| !status.is_empty());

    let total = state
        .conversation_repo
        .count_conversations(status_filter.as_deref())
        .await
        .map_err(|err| QueryError::Internal(err.to_string()))?;
    let conversations = state
        .conversation_repo
        .list_conversations(status_filter.as_deref(), cursor, limit)
        .await
        .map_err(|err| QueryError::Internal(err.to_string()))?;

    let mapped = conversations
        .iter()
        .map(ConversationDto::from)
        .collect::<Vec<_>>();
    let next_cursor = paginate_next_cursor(cursor, mapped.len(), limit);

    Ok(ListConversationsResult {
        conversations: mapped,
        total,
        next_cursor,
    })
}

#[derive(Debug, Serialize)]
pub struct ListItemsResult {
    pub items: Vec<ItemDto>,
    pub total: usize,
    pub next_cursor: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct SearchItemsResult {
    pub items: Vec<ItemDto>,
}

#[derive(Debug, Serialize)]
pub struct QuotaResult {
    pub limit: Option<usize>,
    pub used: usize,
    pub remaining: Option<usize>,
    pub exceeded: bool,
}

#[derive(Debug, Clone)]
pub enum QueryError {
    NotFound(String),
    Internal(String),
}

impl QueryError {
    pub fn code(&self) -> ApplicationErrorCode {
        match self {
            Self::NotFound(_) => ApplicationErrorCode::NotFound,
            Self::Internal(_) => ApplicationErrorCode::Internal,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::NotFound(message) => message,
            Self::Internal(message) => message,
        }
    }
}

pub async fn list_items(
    state: Arc<AppState>,
    query: ListItemsQuery,
) -> Result<ListItemsResult, QueryError> {
    let cursor = query.cursor.unwrap_or(0);
    let limit = query.limit.unwrap_or(20).clamp(1, 100);

    let total = state
        .store
        .count_items(None)
        .await
        .map_err(|err| QueryError::Internal(err.to_string()))?;
    let items = state
        .store
        .find_recent(None, cursor, limit)
        .await
        .map_err(|err| QueryError::Internal(err.to_string()))?;
    let next_cursor = paginate_next_cursor(cursor, items.len(), limit);

    Ok(ListItemsResult {
        items: items.iter().map(ItemDto::from).collect::<Vec<_>>(),
        total,
        next_cursor,
    })
}

pub async fn search_items(
    state: Arc<AppState>,
    query: SearchQuery,
) -> Result<SearchItemsResult, QueryError> {
    let keyword = query.q.unwrap_or_default().trim().to_string();
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    if keyword.is_empty() {
        return Ok(SearchItemsResult { items: Vec::new() });
    }

    let result = state
        .engine
        .search(CoreSearchQuery::new(&keyword).with_limit(limit))
        .await
        .map_err(|err| QueryError::Internal(err.to_string()))?;

    Ok(SearchItemsResult {
        items: result
            .items
            .iter()
            .map(|hit| ItemDto::from(&hit.item))
            .collect::<Vec<_>>(),
    })
}

pub async fn get_quota(state: Arc<AppState>, user_id: &str) -> Result<QuotaResult, QueryError> {
    let used = state
        .store
        .count_items(None)
        .await
        .map_err(|err| QueryError::Internal(err.to_string()))?;

    if state.free_quota_items == 0 || state.is_premium_user(user_id) {
        return Ok(QuotaResult {
            limit: None,
            used,
            remaining: None,
            exceeded: false,
        });
    }

    Ok(QuotaResult {
        limit: Some(state.free_quota_items),
        used,
        remaining: Some(state.free_quota_items.saturating_sub(used)),
        exceeded: used >= state.free_quota_items,
    })
}

#[derive(Debug, Serialize)]
pub struct ListDocumentsResult {
    pub documents: Vec<DocumentDto>,
    pub total: usize,
    pub next_cursor: Option<usize>,
}

pub async fn list_documents(
    state: Arc<AppState>,
    cursor: usize,
    limit: usize,
) -> Result<ListDocumentsResult, QueryError> {
    let limit = limit.clamp(1, 100);
    let total = state
        .doc_store
        .count()
        .await
        .map_err(|e| QueryError::Internal(e.to_string()))?;
    let docs = state
        .doc_store
        .find_recent(cursor, limit)
        .await
        .map_err(|e| QueryError::Internal(e.to_string()))?;
    let item_counts = count_items_per_document(&state, &docs).await?;
    let next_cursor = paginate_next_cursor(cursor, docs.len(), limit);

    Ok(ListDocumentsResult {
        documents: docs
            .iter()
            .enumerate()
            .map(|(i, doc)| DocumentDto {
                id: doc.id().to_string(),
                title: doc.title().map(ToString::to_string),
                source: doc.source().to_string(),
                url: doc.url().to_string(),
                item_count: item_counts.get(i).copied().unwrap_or(0),
                captured_at: doc.captured_at().to_rfc3339(),
                created_at: doc.created_at().to_rfc3339(),
            })
            .collect(),
        total,
        next_cursor,
    })
}

pub async fn get_document(
    state: Arc<AppState>,
    doc_id: &str,
) -> Result<DocumentDetailDto, QueryError> {
    let doc = state
        .doc_store
        .find_by_id(&refine_core::knowledge::DocumentId::from(doc_id))
        .await
        .map_err(|e| QueryError::Internal(e.to_string()))?
        .ok_or_else(|| QueryError::NotFound("Document not found".to_string()))?;

    let items = state
        .store
        .find_by_document_id(&refine_core::knowledge::DocumentId::from(doc_id))
        .await
        .map_err(|e| QueryError::Internal(e.to_string()))?;

    let raw_content = if doc.raw_content().is_empty()
        && doc.url().starts_with("remem://raw-session/v2/")
    {
        let session_ref = doc.url().to_string();
        let projection_version = doc
            .source_version()
            .ok_or_else(|| QueryError::Internal("Remem document is missing snapshot hash".into()))?
            .to_string();
        let expected_hash = remem_snapshot_hash(&projection_version)?.to_string();
        tokio::task::spawn_blocking(move || {
            refine_core::session::load_remem_document_content(&session_ref, &expected_hash)
        })
        .await
        .map_err(|error| QueryError::Internal(format!("Remem hydration task failed: {error}")))?
        .map_err(|error| QueryError::Internal(format!("Remem hydration failed: {error}")))?
    } else {
        doc.raw_content().to_string()
    };

    Ok(DocumentDetailDto {
        id: doc.id().to_string(),
        title: doc.title().map(ToString::to_string),
        raw_content,
        source: doc.source().to_string(),
        url: doc.url().to_string(),
        captured_at: doc.captured_at().to_rfc3339(),
        created_at: doc.created_at().to_rfc3339(),
        items: items.iter().map(ItemDto::from).collect(),
    })
}

fn remem_snapshot_hash(projection_version: &str) -> Result<&str, QueryError> {
    let (hash, mode) = projection_version.rsplit_once(':').ok_or_else(|| {
        QueryError::Internal("Remem document has an invalid projection version".into())
    })?;
    if !matches!(mode, "interactive" | "unattended" | "subagent" | "unknown")
        || !hash.starts_with("sha256:")
        || hash.len() != 71
        || !hash[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(QueryError::Internal(
            "Remem document has an invalid projection version".into(),
        ));
    }
    Ok(hash)
}

async fn count_items_per_document(
    state: &Arc<AppState>,
    docs: &[refine_core::knowledge::Document],
) -> Result<Vec<usize>, QueryError> {
    let mut counts = Vec::with_capacity(docs.len());
    for doc in docs {
        let items = state
            .store
            .find_by_document_id(doc.id())
            .await
            .map_err(|e| QueryError::Internal(e.to_string()))?;
        counts.push(items.len());
    }
    Ok(counts)
}

fn paginate_next_cursor(cursor: usize, returned: usize, limit: usize) -> Option<usize> {
    if returned == limit {
        Some(cursor + returned)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::get_document;
    use super::paginate_next_cursor;
    #[cfg(unix)]
    use crate::state::{AppState, AuthConfig};
    #[cfg(unix)]
    use refine_core::knowledge::Document;
    #[cfg(unix)]
    use std::ffi::OsString;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    #[cfg(unix)]
    use std::path::Path;
    #[cfg(unix)]
    use std::sync::{Arc, Mutex, MutexGuard};

    #[cfg(unix)]
    static REMEM_BIN_LOCK: Mutex<()> = Mutex::new(());

    #[cfg(unix)]
    struct RememBinGuard {
        previous: Option<OsString>,
        _lock: MutexGuard<'static, ()>,
    }

    #[cfg(unix)]
    impl RememBinGuard {
        fn install(path: &Path) -> Self {
            let lock = REMEM_BIN_LOCK.lock().expect("lock REFINE_REMEM_BIN");
            let previous = std::env::var_os("REFINE_REMEM_BIN");
            std::env::set_var("REFINE_REMEM_BIN", path);
            Self {
                previous,
                _lock: lock,
            }
        }
    }

    #[cfg(unix)]
    impl Drop for RememBinGuard {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(previous) => std::env::set_var("REFINE_REMEM_BIN", previous),
                None => std::env::remove_var("REFINE_REMEM_BIN"),
            }
        }
    }

    #[test]
    fn paginate_next_cursor_returns_none_on_last_page() {
        assert_eq!(paginate_next_cursor(10, 5, 20), None);
        assert_eq!(paginate_next_cursor(0, 0, 20), None);
    }

    #[test]
    fn paginate_next_cursor_returns_next_for_full_page() {
        assert_eq!(paginate_next_cursor(0, 20, 20), Some(20));
        assert_eq!(paginate_next_cursor(20, 20, 20), Some(40));
    }

    #[test]
    fn paginate_next_cursor_ignores_stale_total_after_delete() {
        assert_eq!(paginate_next_cursor(20, 0, 20), None);
        assert_eq!(paginate_next_cursor(20, 5, 20), None);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn get_document_hydrates_remem_reference_without_persisting_the_body() {
        let temp = tempfile::tempdir().expect("create test directory");
        let binary = temp.path().join("fake-remem");
        std::fs::write(
            &binary,
            concat!(
                "#!/bin/sh\n",
                "case \"$2\" in\n",
                "sessions) printf '%s\\n' '",
                r#"{"since_epoch":null,"until_epoch":null,"project":"/repo","sample":0,"latest":null,"count":1,"sessions":[{"session_ref":"remem://raw-session/v2/636f6465782d636c69/6c6f63616c/2f7265706f/7331","host":"codex-cli","session_mode":"interactive","source_root":"local","project":"/repo","session_id":"s1","first_epoch":10,"last_epoch":20,"message_count":2,"user_message_count":1,"assistant_message_count":1,"content_hash":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","user_message_samples":[]}]}"#,
                "' ;;\n",
                "messages) printf '%s\\n' '",
                r#"{"source_type":"raw_archive","host":"codex-cli","source_root":"local","project":"/repo","session_id":"s1","order":"created_at_epoch_asc_id_asc","limit":2000,"count":2,"has_more":false,"next_cursor":null,"content_hash":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","messages":[{"id":1,"role":"user","content":"question","source":"codex","branch":null,"cwd":"/repo","created_at_epoch":10},{"id":2,"role":"assistant","content":"answer","source":"codex","branch":null,"cwd":"/repo","created_at_epoch":20}]}"#,
                "' ;;\n",
                "*) exit 2 ;;\n",
                "esac\n",
            ),
        )
        .expect("write fake remem");
        let mut permissions = std::fs::metadata(&binary).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&binary, permissions).expect("make fake remem executable");
        let _remem_bin = RememBinGuard::install(&binary);

        let state = Arc::new(
            AppState::build_for_test(
                temp.path().join("refine.sqlite"),
                AuthConfig {
                    api_token: None,
                    dev_anon: true,
                },
            )
            .await
            .expect("build app state"),
        );
        let mut document = Document::new("codex-session", "");
        document.set_url("remem://raw-session/v2/636f6465782d636c69/6c6f63616c/2f7265706f/7331");
        document.set_source_version(Some(
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:interactive",
        ));
        state
            .doc_store
            .save(&document)
            .await
            .expect("save referenced document");

        let detail = get_document(state.clone(), document.id().as_str())
            .await
            .expect("hydrate document detail");
        assert_eq!(detail.raw_content, "User: question\nAssistant: answer\n");
        let persisted = state
            .doc_store
            .find_by_id(document.id())
            .await
            .unwrap()
            .unwrap();
        assert!(persisted.raw_content().is_empty());
    }
}
