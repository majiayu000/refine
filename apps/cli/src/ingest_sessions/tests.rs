use super::*;
use async_trait::async_trait;
use chrono::TimeZone;
use refine_core::error::InfraResult;
use refine_core::infra::SqliteStore;
use refine_core::knowledge::{DocumentId, DocumentRepository, Item, ItemRepository, ItemType};
use refine_core::session::discover_sessions_in;
use std::collections::VecDeque;
use std::fs;
use std::sync::Mutex;

#[test]
fn auto_provider_prefers_remem_when_the_probe_succeeds() {
    let selection = select_auto_provider(|| Ok(vec!["summary"])).unwrap();
    assert!(matches!(
        selection,
        AutoProviderSelection::Remem(values) if values == vec!["summary"]
    ));
}

#[test]
fn auto_provider_falls_back_only_for_a_missing_executable() {
    let selection = select_auto_provider::<Vec<()>, _>(|| {
        Err(std::io::Error::new(std::io::ErrorKind::NotFound, "remem").into())
    })
    .unwrap();
    assert!(matches!(selection, AutoProviderSelection::LocalFallback));
}

#[test]
fn auto_provider_keeps_remem_provider_errors_strict() {
    let result =
        select_auto_provider::<Vec<()>, _>(|| Err(anyhow::anyhow!("raw sessions contract drift")));
    let error = result.expect_err("contract errors must not fall back to local");
    assert!(error.to_string().contains("contract drift"));
}

struct StaticLlmClient {
    response: String,
}

struct SequenceLlmClient {
    responses: Mutex<VecDeque<String>>,
    calls: AtomicUsize,
}

impl SequenceLlmClient {
    fn new(responses: Vec<String>) -> Self {
        Self {
            responses: Mutex::new(VecDeque::from(responses)),
            calls: AtomicUsize::new(0),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::Relaxed)
    }
}

#[async_trait]
impl LlmClient for SequenceLlmClient {
    async fn complete(&self, _prompt: &str, _system: Option<&str>) -> InfraResult<String> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(self
            .responses
            .lock()
            .expect("responses lock")
            .pop_front()
            .unwrap_or_else(|| "{}".to_string()))
    }
}

#[tokio::test]
async fn source_filter_requires_explicit_local_provider() {
    let store = Arc::new(SqliteStore::in_memory().expect("in-memory sqlite store"));
    let doc_store: Arc<dyn DocumentRepository> = store;
    let error = handle_ingest_sessions(
        IngestOptions {
            source: Some(SessionSource::ClaudeCode),
            provider: IngestProvider::Auto,
            limit: None,
            latest: None,
            dry_run: true,
            retry_quarantined: false,
        },
        Path::new("/tmp/refine-test.db"),
        doc_store,
        None,
    )
    .await
    .expect_err("source filtering must not guess a platform in remem mode");

    assert!(error.to_string().contains("--provider local"));
}

#[async_trait]
impl LlmClient for StaticLlmClient {
    async fn complete(&self, _prompt: &str, _system: Option<&str>) -> InfraResult<String> {
        Ok(self.response.clone())
    }
}

#[test]
fn project_for_ingest_prefers_discovered_project_then_session_metadata() {
    assert_eq!(
        project_for_ingest(Some("claude-project"), Some("codex-cwd")).as_deref(),
        Some("claude-project")
    );
    assert_eq!(
        project_for_ingest(None, Some("codex-cwd")).as_deref(),
        Some("codex-cwd")
    );
    assert_eq!(project_for_ingest(None, None), None);
}

#[test]
fn session_captured_at_prefers_session_started_at_then_file_mtime() {
    let started_at = Utc.with_ymd_and_hms(2026, 5, 20, 12, 0, 0).unwrap();
    let mtime = SystemTime::UNIX_EPOCH + Duration::from_secs(42);
    assert_eq!(session_captured_at(Some(started_at), mtime), started_at);

    let fallback = session_captured_at(None, mtime);
    assert_eq!(fallback, DateTime::<Utc>::from(mtime));
}

#[test]
fn log_preview_truncates_on_char_boundary() {
    let message = "无法解析 facet 响应: 回回回";
    let preview = log_preview(message, 10);

    assert!(preview.ends_with("..."));
    assert!(preview.is_char_boundary(preview.len()));
}

#[tokio::test]
async fn facet_parse_error_retries_then_succeeds() {
    let client = Arc::new(SequenceLlmClient::new(vec![
        "not json".to_string(),
        r#"{
            "session_summary": "重试后成功",
            "cognitive_level": "proficient",
            "collaboration_mode": "pair_programming",
            "decisions": [], "bugs_fixed": [], "patterns": [], "friction": [],
            "project_progress": [], "questions": [], "knowledge_gained": [],
            "tools_discovered": [], "architecture": [], "code_artifacts": []
        }"#
        .to_string(),
    ]));
    let quota_hit = Arc::new(AtomicBool::new(false));

    let result = extract_and_parse_facets_with_retry_policy(
        "content",
        &(client.clone() as Arc<dyn LlmClient>),
        &quota_hit,
        2,
        0,
    )
    .await
    .expect("parse retry should recover");

    assert_eq!(result.session_summary, "重试后成功");
    assert_eq!(client.calls(), 2);
}

#[test]
fn session_needs_refresh_when_file_mtime_is_newer_than_saved_document() {
    let mut doc = Document::new("codex-session", "raw");
    doc.set_url("file:///tmp/session.jsonl");

    let old_mtime = SystemTime::now()
        .checked_sub(Duration::from_secs(60))
        .unwrap();
    assert!(!session_needs_refresh(&doc, old_mtime));

    let new_mtime = SystemTime::now() + Duration::from_millis(200);
    assert!(session_needs_refresh(&doc, new_mtime));
}

#[test]
fn source_snapshot_uses_full_content_and_preserves_document_time() {
    let mut versioned = Document::new("remem-raw-session", "raw");
    let raw_version = content_source_version("remem", "raw");
    versioned.set_source_version(Some(&raw_version));
    assert_eq!(versioned.source_version(), Some(raw_version.as_str()));
    assert_ne!(
        raw_version,
        content_source_version("remem", "raw corrected")
    );

    let unversioned = Document::new("remem-raw-session", "raw");
    let original_updated_at = unversioned.updated_at();
    let backfilled = document_with_source_version(&unversioned, &raw_version);
    assert_eq!(backfilled.updated_at(), original_updated_at);
    assert_eq!(backfilled.source_version(), Some(raw_version.as_str()));
}

#[test]
fn scoped_cursor_keeps_other_sources_discoverable() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let db_path = tmp.path().join("refine.db");
    fs::write(&db_path, "").unwrap();

    let claude_dir = home.join(".claude/projects/proj");
    fs::create_dir_all(&claude_dir).unwrap();
    let claude_path = claude_dir.join("claude.jsonl");
    fs::write(&claude_path, "{}").unwrap();
    filetime::set_file_mtime(&claude_path, filetime::FileTime::from_unix_time(20_000, 0)).unwrap();

    let codex_dir = home.join(".codex/sessions");
    fs::create_dir_all(&codex_dir).unwrap();
    let codex_path = codex_dir.join("codex.jsonl");
    fs::write(&codex_path, "{}").unwrap();
    filetime::set_file_mtime(&codex_path, filetime::FileTime::from_unix_time(1_000, 0)).unwrap();

    write_last_ingest_mtime_at(
        home,
        Some(&SessionSource::ClaudeCode),
        &db_path,
        SystemTime::UNIX_EPOCH + Duration::from_secs(10_000),
    );

    let claude_cutoff = read_last_ingest_mtime_at(home, Some(&SessionSource::ClaudeCode), &db_path)
        .unwrap()
        .checked_sub(Duration::from_secs(3600))
        .unwrap();
    let claude_discovered =
        discover_sessions_in(home, Some(SessionSource::ClaudeCode), Some(claude_cutoff));
    assert_eq!(claude_discovered.len(), 1);

    let codex_cutoff = read_last_ingest_mtime_at(home, Some(&SessionSource::Codex), &db_path)
        .map(|last| last.checked_sub(Duration::from_secs(3600)).unwrap());
    assert!(codex_cutoff.is_none());

    let codex_discovered = discover_sessions_in(home, Some(SessionSource::Codex), codex_cutoff);
    assert_eq!(codex_discovered.len(), 1);
    assert_eq!(codex_discovered[0].path, codex_path);
}

#[test]
fn cursor_is_partitioned_by_database_path() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let db_a = tmp.path().join("a.db");
    let db_b = tmp.path().join("b.db");
    fs::write(&db_a, "").unwrap();
    fs::write(&db_b, "").unwrap();

    let when = SystemTime::UNIX_EPOCH + Duration::from_secs(42);
    write_last_ingest_mtime_at(home, Some(&SessionSource::Codex), &db_a, when);

    assert_eq!(
        read_last_ingest_mtime_at(home, Some(&SessionSource::Codex), &db_a),
        Some(when)
    );
    assert_eq!(
        read_last_ingest_mtime_at(home, Some(&SessionSource::Codex), &db_b),
        None
    );
    assert_ne!(
        incremental_cursor_path(home, Some(&SessionSource::Codex), &db_a),
        incremental_cursor_path(home, Some(&SessionSource::Codex), &db_b)
    );
}

#[test]
fn safe_cursor_stays_before_oldest_failed_file() {
    let scan_start = SystemTime::UNIX_EPOCH + Duration::from_secs(20_000);
    let failures = [
        SystemTime::UNIX_EPOCH + Duration::from_secs(15_000),
        SystemTime::UNIX_EPOCH + Duration::from_secs(12_000),
    ];
    assert_eq!(
        safe_cursor_watermark(scan_start, &failures),
        SystemTime::UNIX_EPOCH + Duration::from_secs(11_999)
    );
    assert_eq!(safe_cursor_watermark(scan_start, &[]), scan_start);
}

#[test]
fn cursor_write_atomically_replaces_previous_value() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("cursor/state");
    let first = SystemTime::UNIX_EPOCH + Duration::from_secs(10);
    let second = SystemTime::UNIX_EPOCH + Duration::from_secs(20);
    let first = IngestCursorState {
        version: INGEST_CURSOR_VERSION,
        watermark_secs: unix_seconds(first),
        failures: Vec::new(),
    };
    let second = IngestCursorState {
        version: INGEST_CURSOR_VERSION,
        watermark_secs: unix_seconds(second),
        failures: vec![IngestCursorFailure {
            path_sha256: "abc".to_string(),
            modified_at_secs: 19,
            reason: "parse_error".to_string(),
        }],
    };
    write_ingest_cursor_at(&path, &first).expect("first cursor write");
    write_ingest_cursor_at(&path, &second).expect("replace cursor");
    let loaded: IngestCursorState =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(loaded, second);
    assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
}

#[test]
fn cursor_reader_accepts_legacy_seconds_and_v2_state() {
    assert_eq!(parse_ingest_cursor("42\n"), Some(42));
    let state = IngestCursorState {
        version: INGEST_CURSOR_VERSION,
        watermark_secs: 84,
        failures: Vec::new(),
    };
    assert_eq!(
        parse_ingest_cursor(&serde_json::to_string(&state).unwrap()),
        Some(84)
    );
    assert_eq!(parse_ingest_cursor("not a cursor"), None);
}

fn read_last_ingest_mtime_at(
    home: &Path,
    source: Option<&SessionSource>,
    db_path: &Path,
) -> Option<SystemTime> {
    let path = incremental_cursor_path(home, source, db_path);
    let secs: u64 = std::fs::read_to_string(path).ok()?.trim().parse().ok()?;
    Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
}

fn write_last_ingest_mtime_at(
    home: &Path,
    source: Option<&SessionSource>,
    db_path: &Path,
    t: SystemTime,
) {
    let path = incremental_cursor_path(home, source, db_path);
    let dir = path.parent().unwrap();
    fs::create_dir_all(dir).unwrap();
    let dur = t.duration_since(SystemTime::UNIX_EPOCH).unwrap();
    fs::write(path, dur.as_secs().to_string()).unwrap();
}

#[tokio::test]
async fn process_single_session_links_items_to_saved_document_id() {
    let store = Arc::new(SqliteStore::in_memory().expect("in-memory sqlite store"));
    let doc_store: Arc<dyn DocumentRepository> = store.clone();
    let client: Arc<dyn LlmClient> = Arc::new(StaticLlmClient {
        response: r#"{
            "session_summary": "修复文档关联",
            "cognitive_level": "proficient",
            "collaboration_mode": "pair_programming",
            "decisions": ["使用保存后的文档 ID"],
            "bugs_fixed": ["修复会话导入的 shadow document id"],
            "patterns": [],
            "friction": [],
            "project_progress": [],
            "questions": [],
            "knowledge_gained": [],
            "tools_discovered": [],
            "architecture": [],
            "code_artifacts": []
        }"#
        .to_string(),
    });
    let quota_hit = Arc::new(AtomicBool::new(false));
    let pending = PendingSession {
        idx: 0,
        total: 1,
        url: "file:///tmp/session.jsonl".to_string(),
        source: SessionSource::Codex,
        project: Some("refine".to_string()),
        captured_at: Utc.with_ymd_and_hms(2026, 5, 20, 12, 0, 0).unwrap(),
        has_embedded_timestamp: true,
        raw_content: "User: fix the ingest bug".to_string(),
        source_version: None,
        needs_chunk: false,
        chunks: Vec::new(),
        existing_document: None,
        legacy_documents_to_delete: Vec::new(),
    };

    let item_count = process_single_session(&pending, &client, &doc_store, &quota_hit)
        .await
        .expect("session ingest should succeed");
    assert_eq!(item_count, 3);

    let docs = DocumentRepository::find_recent(store.as_ref(), 0, 10)
        .await
        .expect("documents should load");
    assert_eq!(docs.len(), 1);
    let saved_doc = &docs[0];
    assert_eq!(saved_doc.captured_at(), pending.captured_at);

    let linked_items = store
        .find_by_document_id(saved_doc.id())
        .await
        .expect("items should be queryable by saved document id");
    assert_eq!(linked_items.len(), item_count);
    assert!(linked_items
        .iter()
        .all(|item| item.document_id() == Some(saved_doc.id())));
}

#[tokio::test]
async fn process_single_session_refresh_replaces_old_items_and_preserves_raw_transcript() {
    let store = Arc::new(SqliteStore::in_memory().expect("in-memory sqlite store"));
    let item_store: Arc<dyn ItemRepository> = store.clone();
    let doc_store: Arc<dyn DocumentRepository> = store.clone();

    let mut existing_doc = Document::new("codex-session", "old raw");
    existing_doc.set_url("file:///tmp/session.jsonl");
    doc_store
        .save(&existing_doc)
        .await
        .expect("existing document should save");

    let mut old_item = Item::new_observation("old", "old");
    old_item.set_document_id(existing_doc.id().clone());
    item_store
        .save(&old_item)
        .await
        .expect("old item should save");

    let client: Arc<dyn LlmClient> = Arc::new(StaticLlmClient {
        response: r#"{
            "session_summary": "刷新后的会话",
            "cognitive_level": "proficient",
            "collaboration_mode": "pair_programming",
            "decisions": ["保留原始 transcript"],
            "bugs_fixed": ["替换 stale session 的旧 items"],
            "patterns": [],
            "friction": [],
            "project_progress": [],
            "questions": [],
            "knowledge_gained": [],
            "tools_discovered": [],
            "architecture": [],
            "code_artifacts": []
        }"#
        .to_string(),
    });
    let quota_hit = Arc::new(AtomicBool::new(false));
    let pending = PendingSession {
        idx: 0,
        total: 1,
        url: existing_doc.url().to_string(),
        source: SessionSource::Codex,
        project: Some("refine".to_string()),
        captured_at: Utc.with_ymd_and_hms(2026, 5, 20, 12, 0, 0).unwrap(),
        has_embedded_timestamp: false,
        raw_content: "User: original transcript\nAssistant: final answer\n".to_string(),
        source_version: None,
        needs_chunk: true,
        chunks: vec!["chunk summary input".to_string()],
        existing_document: Some(existing_doc.clone()),
        legacy_documents_to_delete: Vec::new(),
    };

    let item_count = process_single_session(&pending, &client, &doc_store, &quota_hit)
        .await
        .expect("session refresh should succeed");
    assert_eq!(item_count, 3);

    let saved_doc = doc_store
        .find_by_url(&pending.url)
        .await
        .expect("document query should succeed")
        .expect("document should exist");
    assert_eq!(saved_doc.id(), existing_doc.id());
    assert_eq!(saved_doc.raw_content(), pending.raw_content);
    assert_eq!(saved_doc.title(), Some("刷新后的会话"));
    assert_eq!(saved_doc.captured_at(), existing_doc.captured_at());

    let linked_items = store
        .find_by_document_id(existing_doc.id())
        .await
        .expect("items should be queryable by saved document id");
    assert_eq!(linked_items.len(), item_count);
    assert!(linked_items.iter().all(|item| item.title() != "old"));
}

#[tokio::test]
async fn remem_save_removes_superseded_legacy_document_and_facets() {
    let store = Arc::new(SqliteStore::in_memory().expect("in-memory sqlite store"));
    let item_store: Arc<dyn ItemRepository> = store.clone();
    let doc_store: Arc<dyn DocumentRepository> = store.clone();

    let mut legacy_doc = Document::new("codex-session", "User: old\n");
    legacy_doc.set_url("/tmp/rollout-2026-session-1.jsonl");
    doc_store.save(&legacy_doc).await.unwrap();
    let mut legacy_item = Item::new_observation("legacy", "legacy");
    legacy_item.set_document_id(legacy_doc.id().clone());
    item_store.save(&legacy_item).await.unwrap();
    let legacy_item_id = legacy_item.id().clone();

    let client: Arc<dyn LlmClient> = Arc::new(StaticLlmClient {
        response: r#"{
            "session_summary": "remem replacement",
            "cognitive_level": "proficient",
            "collaboration_mode": "pair_programming",
            "decisions": ["use remem identity"],
            "bugs_fixed": [], "patterns": [], "friction": [],
            "project_progress": [], "questions": [], "knowledge_gained": [],
            "tools_discovered": [], "architecture": [], "code_artifacts": []
        }"#
        .to_string(),
    });
    let pending = PendingSession {
        idx: 0,
        total: 1,
        url: "remem-raw://v1/local/repo/session-1".to_string(),
        source: SessionSource::RememRaw,
        project: Some("refine".to_string()),
        captured_at: Utc.with_ymd_and_hms(2026, 7, 20, 0, 0, 0).unwrap(),
        has_embedded_timestamp: true,
        raw_content: "User: old\nAssistant: new\n".to_string(),
        source_version: Some("remem:v1:10:20:2:1:1".to_string()),
        needs_chunk: false,
        chunks: Vec::new(),
        existing_document: None,
        legacy_documents_to_delete: vec![legacy_doc.id().clone()],
    };

    let replacement_item_count = process_single_session(
        &pending,
        &client,
        &doc_store,
        &Arc::new(AtomicBool::new(false)),
    )
    .await
    .unwrap();

    assert!(doc_store
        .find_by_id(legacy_doc.id())
        .await
        .unwrap()
        .is_none());
    let replacement = doc_store
        .find_by_url(&pending.url)
        .await
        .unwrap()
        .expect("remem replacement document");
    assert_eq!(doc_store.count().await.unwrap(), 1);
    assert!(item_store
        .find_by_document_id(legacy_doc.id())
        .await
        .unwrap()
        .is_empty());
    assert!(!item_store
        .find_by_document_id(replacement.id())
        .await
        .unwrap()
        .is_empty());
    let observations = item_store
        .find_by_type(ItemType::Observation)
        .await
        .unwrap();
    assert_eq!(observations.len(), replacement_item_count);
    assert!(observations.iter().all(|item| item.id() != &legacy_item_id));
}

#[tokio::test]
async fn replacement_and_legacy_cleanup_roll_back_as_one_transaction() {
    let store = Arc::new(SqliteStore::in_memory().expect("in-memory sqlite store"));
    let doc_store: Arc<dyn DocumentRepository> = store.clone();
    let item_store: Arc<dyn ItemRepository> = store;
    let mut replacement = Document::new("remem-raw-session", "replacement");
    replacement.set_url("remem-raw://v1/local/repo/session-1");
    let mut item = Item::new_observation("replacement", "replacement");
    item.set_document_id(replacement.id().clone());

    let error = doc_store
        .save_with_replaced_items_and_delete_documents(
            &replacement,
            &[item.clone()],
            &[DocumentId::from("missing-legacy-document")],
        )
        .await
        .expect_err("missing obsolete identity must roll back the whole transaction");
    assert!(error.to_string().contains("does not exist"));
    assert!(doc_store
        .find_by_url(replacement.url())
        .await
        .unwrap()
        .is_none());
    assert!(!item_store.exists(item.id()).await.unwrap());
}

#[tokio::test]
async fn quota_hit_short_circuits_before_llm_call() {
    use refine_core::infra::ClaudeClient;
    let client: Arc<dyn LlmClient> = Arc::new(ClaudeClient::new("test-key"));
    let quota_hit = Arc::new(AtomicBool::new(true));

    let err = llm_call_with_retry(&client, "content", &quota_hit)
        .await
        .expect_err("quota flag should skip the call");

    assert!(err.to_string().contains("LLM 配额已耗尽"));
}

#[tokio::test]
async fn provider_rate_limit_sets_batch_early_stop_without_retrying() {
    let client = Arc::new(SequenceLlmClient::new(vec!["unused".to_string()]));
    let client_dyn = client.clone() as Arc<dyn LlmClient>;
    let quota_hit = Arc::new(AtomicBool::new(false));

    let first = finish_llm_call(
        Err(InfraError::RateLimited {
            retry_after_secs: Some(42),
        }),
        &quota_hit,
    )
    .expect_err("provider quota must stop the first call");
    assert!(first.to_string().contains("LLM 配额已耗尽"));
    assert!(quota_hit.load(Ordering::Relaxed));
    assert_eq!(client.calls(), 0);

    let second = llm_call_with_retry(&client_dyn, "other content", &quota_hit)
        .await
        .expect_err("later batch work must short-circuit");
    assert!(second.to_string().contains("跳过"));
    assert_eq!(client.calls(), 0);
}

#[test]
fn content_rejection_survives_anyhow_context() {
    let error = anyhow::Error::new(InfraError::LlmRejected {
        code: "sensitive_words_detected".into(),
        message: "blocked".into(),
    })
    .context("chunk 2/3 failed");

    assert_eq!(
        content_rejection(&error),
        Some(("sensitive_words_detected".into(), "blocked".into()))
    );
}
