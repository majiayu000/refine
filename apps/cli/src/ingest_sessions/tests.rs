use super::*;
use async_trait::async_trait;
use chrono::TimeZone;
use refine_core::error::{InfraError, InfraResult};
use refine_core::infra::SqliteStore;
use refine_core::knowledge::{DocumentId, DocumentRepository, Item, ItemRepository, ItemType, Tag};
use refine_core::session::SessionMode;
use refine_core::session::{discover_sessions_in, parse_session_content};
use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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

struct AppendingLlmClient {
    path: PathBuf,
    record: String,
    response: String,
    appended: AtomicBool,
}

#[async_trait]
impl LlmClient for AppendingLlmClient {
    async fn complete(&self, _prompt: &str, _system: Option<&str>) -> InfraResult<String> {
        if !self.appended.swap(true, Ordering::SeqCst) {
            use std::io::Write as _;
            let mut file = fs::OpenOptions::new()
                .append(true)
                .open(&self.path)
                .expect("active transcript should remain appendable");
            file.write_all(self.record.as_bytes())
                .expect("active writer should append a complete JSONL record");
            file.sync_all()
                .expect("appended transcript record should reach the file");
        }
        Ok(self.response.clone())
    }
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
            backfill_session_metadata: false,
        },
        Path::new("/tmp/refine-test.db"),
        doc_store,
        None,
    )
    .await
    .expect_err("source filtering must not guess a platform in remem mode");

    assert!(error.to_string().contains("--provider local"));
}

#[tokio::test]
async fn metadata_backfill_requires_local_codex_source() {
    let store = Arc::new(SqliteStore::in_memory().expect("in-memory sqlite store"));
    let doc_store: Arc<dyn DocumentRepository> = store;
    let error = handle_ingest_sessions(
        IngestOptions {
            source: Some(SessionSource::ClaudeCode),
            provider: IngestProvider::Local,
            limit: None,
            latest: None,
            dry_run: true,
            retry_quarantined: false,
            backfill_session_metadata: true,
        },
        Path::new("/tmp/refine-test.db"),
        doc_store,
        None,
    )
    .await
    .expect_err("metadata backfill must not classify non-Codex transcripts");

    assert!(error.to_string().contains("--source codex"));
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
fn project_identity_for_ingest_prefers_raw_parser_evidence() {
    assert_eq!(
        project_identity_for_ingest(Some("bar"), Some("/r/Foo/bar")).as_deref(),
        Some("/r/Foo/bar")
    );
    assert_eq!(
        project_identity_for_ingest(Some("-r-Foo-bar"), None).as_deref(),
        Some("-r-Foo-bar")
    );
    assert_eq!(project_identity_for_ingest(None, None), None);
}

#[tokio::test]
async fn parser_to_sqlite_portrait_preserves_case_sensitive_cwd_collisions() {
    let store = Arc::new(SqliteStore::in_memory().expect("in-memory sqlite store"));
    let doc_store: Arc<dyn DocumentRepository> = store.clone();
    let client: Arc<dyn LlmClient> = Arc::new(StaticLlmClient {
        response: r#"{
            "session_summary": "project identity fixture",
            "cognitive_level": "competent",
            "collaboration_mode": "review",
            "decisions": [], "bugs_fixed": [], "patterns": [], "friction": [],
            "project_progress": [], "questions": [], "knowledge_gained": [],
            "tools_discovered": [], "architecture": [], "code_artifacts": []
        }"#
        .to_string(),
    });
    let cutoff = Utc.with_ymd_and_hms(2026, 8, 28, 0, 0, 0).unwrap();
    let captured_at = cutoff - chrono::Duration::days(1);

    for (index, cwd) in ["/r/Foo/bar", "/r/foo/bar", "bar"].into_iter().enumerate() {
        let transcript = format!("{{\"type\":\"turn_context\",\"payload\":{{\"cwd\":{cwd:?}}}}}\n");
        let session = parse_session_content(
            &transcript,
            &PathBuf::from(format!("/tmp/project-identity-{index}.jsonl")),
            SessionSource::Codex,
        )
        .expect("real Codex cwd metadata should parse");
        let project = project_for_ingest(None, session.meta.project.as_deref());
        let project_identity = project_identity_for_ingest(
            project.as_deref(),
            session.meta.project_identity.as_deref(),
        );
        let pending = PendingSession {
            idx: index,
            total: 3,
            url: format!("file:///tmp/project-identity-{index}.jsonl"),
            source: session.source.clone(),
            project,
            project_identity,
            mode: session.meta.mode,
            captured_at,
            has_embedded_timestamp: false,
            raw_content: session.to_document_content(),
            source_version: None,
            needs_chunk: false,
            chunks: Vec::new(),
            existing_document: None,
            legacy_documents_to_delete: Vec::new(),
        };
        process_single_session(
            &pending,
            &client,
            &doc_store,
            &Arc::new(AtomicBool::new(false)),
        )
        .await
        .expect("parsed session should persist facets");
    }

    let bundle = crate::cognitive_portrait_data::collect_bundle(store.as_ref(), cutoff, 90)
        .await
        .expect("persisted parser evidence should reach the portrait");
    let ranking: std::collections::BTreeMap<_, _> = bundle
        .current
        .metrics
        .project_ranking
        .entries
        .iter()
        .map(|entry| (entry.value.as_str(), entry.count))
        .collect();
    assert_eq!(ranking["path:/r/Foo/bar"], 1);
    assert_eq!(ranking["path:/r/foo/bar"], 1);
    assert_eq!(ranking["other"], 1);
    assert!(!ranking.contains_key("bar"));
    assert_eq!(
        bundle
            .manifest
            .current_window
            .ambiguous_project_alias_observations,
        1
    );
    assert_eq!(bundle.manifest.current_window.ambiguous_project_aliases, 1);
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
fn session_refresh_uses_source_content_instead_of_ingest_timestamp() {
    let old_raw = "User: first message\n";
    let old_version = content_source_version("local", old_raw);
    let mut doc = Document::new("codex-session", old_raw);
    doc.set_url("file:///tmp/session.jsonl");
    doc.set_source_version(Some(&old_version));

    assert!(!session_needs_refresh(&doc, &old_version, old_raw));

    // Model a complete JSONL record appended while the previous snapshot is
    // being processed. The database write can be newer than the append, so
    // mtime compared with Document.updated_at is not a valid freshness check.
    let appended_raw = "User: first message\nAssistant: appended while LLM was running\n";
    let appended_version = content_source_version("local", appended_raw);
    assert!(session_needs_refresh(&doc, &appended_version, appended_raw));

    // Legacy documents without a source_version still use an exact content
    // comparison, never the later database ingestion timestamp.
    let legacy_doc = Document::new("codex-session", old_raw);
    assert!(!session_needs_refresh(&legacy_doc, &old_version, old_raw));
    assert!(session_needs_refresh(
        &legacy_doc,
        &appended_version,
        appended_raw
    ));
}

#[tokio::test]
async fn complete_record_appended_during_llm_is_ingested_on_next_pass() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("active.jsonl");
    fs::write(
        &path,
        "{\"type\":\"user\",\"message\":{\"content\":\"first\"}}\n",
    )
    .unwrap();

    let first_session = parse_session_file(&path, SessionSource::ClaudeCode).unwrap();
    assert!(!first_session.meta.truncated_tail);
    let first_raw = first_session.to_document_content();
    let first_version = content_source_version("local", &first_raw);
    let url = path.to_string_lossy().to_string();
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let doc_store: Arc<dyn DocumentRepository> = store.clone();
    let response = r#"{
        "session_summary": "active snapshot",
        "cognitive_level": "proficient",
        "collaboration_mode": "pair_programming",
        "decisions": [], "bugs_fixed": [], "patterns": [], "friction": [],
        "project_progress": [], "questions": [], "knowledge_gained": [],
        "tools_discovered": [], "architecture": [], "code_artifacts": []
    }"#
    .to_string();
    let appending_client: Arc<dyn LlmClient> = Arc::new(AppendingLlmClient {
        path: path.clone(),
        record: concat!(
            "{\"type\":\"assistant\",\"message\":{\"content\":[",
            "{\"type\":\"text\",\"text\":\"appended\"}]}}\n"
        )
        .to_string(),
        response: response.clone(),
        appended: AtomicBool::new(false),
    });
    let first_pending = PendingSession {
        idx: 0,
        total: 1,
        url: url.clone(),
        source: SessionSource::ClaudeCode,
        project: None,
        project_identity: None,
        mode: SessionMode::Unknown,
        captured_at: Utc::now(),
        has_embedded_timestamp: false,
        raw_content: first_raw.clone(),
        source_version: Some(first_version.clone()),
        needs_chunk: false,
        chunks: Vec::new(),
        existing_document: None,
        legacy_documents_to_delete: Vec::new(),
    };

    process_single_session(
        &first_pending,
        &appending_client,
        &doc_store,
        &Arc::new(AtomicBool::new(false)),
    )
    .await
    .unwrap();
    let first_saved = doc_store.find_by_url(&url).await.unwrap().unwrap();
    assert!(first_saved.raw_content().is_empty());
    assert_eq!(first_saved.source_version(), Some(first_version.as_str()));

    let appended_session = parse_session_file(&path, SessionSource::ClaudeCode).unwrap();
    assert!(!appended_session.meta.truncated_tail);
    assert_eq!(appended_session.messages.len(), 2);
    let appended_raw = appended_session.to_document_content();
    let appended_version = content_source_version("local", &appended_raw);
    assert!(session_needs_refresh(
        &first_saved,
        &appended_version,
        &appended_raw
    ));

    let second_pending = PendingSession {
        idx: 0,
        total: 1,
        url: url.clone(),
        source: SessionSource::ClaudeCode,
        project: None,
        project_identity: None,
        mode: SessionMode::Unknown,
        captured_at: Utc::now(),
        has_embedded_timestamp: false,
        raw_content: appended_raw.clone(),
        source_version: Some(appended_version.clone()),
        needs_chunk: false,
        chunks: Vec::new(),
        existing_document: Some(first_saved),
        legacy_documents_to_delete: Vec::new(),
    };
    let static_client: Arc<dyn LlmClient> = Arc::new(StaticLlmClient { response });
    process_single_session(
        &second_pending,
        &static_client,
        &doc_store,
        &Arc::new(AtomicBool::new(false)),
    )
    .await
    .unwrap();

    let refreshed = doc_store.find_by_url(&url).await.unwrap().unwrap();
    assert!(refreshed.raw_content().is_empty());
    assert_eq!(refreshed.source_version(), Some(appended_version.as_str()));
}

#[test]
fn referenced_session_preserves_identity_and_time_without_duplicate_content() {
    let mut versioned = Document::new("remem-raw-session", "raw");
    let raw_version = content_source_version("remem", "raw");
    versioned.set_source_version(Some(&raw_version));
    assert_eq!(versioned.source_version(), Some(raw_version.as_str()));
    assert_ne!(
        raw_version,
        content_source_version("remem", "raw corrected")
    );

    let mut unversioned = Document::new("remem-raw-session", "raw");
    unversioned.set_url("remem://raw-session/v2/test");
    let original_updated_at = unversioned.updated_at();
    let backfilled = referenced_session_document(
        &unversioned,
        SessionSource::Codex,
        "remem://raw-session/v2/test",
        &raw_version,
    );
    assert_eq!(backfilled.id(), unversioned.id());
    assert_eq!(backfilled.updated_at(), original_updated_at);
    assert_eq!(backfilled.source_version(), Some(raw_version.as_str()));
    assert!(backfilled.raw_content().is_empty());
}

#[tokio::test]
async fn metadata_backfill_preserves_non_provenance_tags_and_is_idempotent() {
    let store = Arc::new(SqliteStore::in_memory().expect("in-memory sqlite store"));
    let doc_store: Arc<dyn DocumentRepository> = store.clone();
    let item_store: Arc<dyn ItemRepository> = store;
    let mut document = Document::new("remem-raw-session", "raw");
    document.set_url("remem-raw://existing");
    doc_store.save(&document).await.unwrap();

    let mut item = Item::new_observation("existing", "existing");
    item.set_document_id(document.id().clone());
    item.set_tags(vec![
        Tag::new("custom-user-tag").unwrap(),
        Tag::new("debugging").unwrap(),
        Tag::new("session_mode_unknown").unwrap(),
    ])
    .unwrap();
    item_store.save(&item).await.unwrap();

    assert!(
        backfill_session_metadata(&doc_store, &document, SessionMode::Unattended, false)
            .await
            .unwrap()
    );
    let dry_run_items = doc_store
        .find_items_by_document_id(document.id())
        .await
        .unwrap();
    assert!(dry_run_items[0]
        .tags()
        .iter()
        .any(|tag| tag.as_str() == "session_mode_unknown"));

    assert!(
        backfill_session_metadata(&doc_store, &document, SessionMode::Unattended, true)
            .await
            .unwrap()
    );
    let items = doc_store
        .find_items_by_document_id(document.id())
        .await
        .unwrap();
    let tags: Vec<_> = items[0].tags().iter().map(|tag| tag.as_str()).collect();
    assert_eq!(
        tags,
        ["custom-user-tag", "debugging", "session_mode_unattended"]
    );

    assert!(
        !backfill_session_metadata(&doc_store, &document, SessionMode::Unattended, true)
            .await
            .unwrap()
    );
    assert!(
        !backfill_session_metadata(&doc_store, &document, SessionMode::Unknown, true)
            .await
            .unwrap()
    );
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
        incremental_cursor_path(
            home,
            Some(&SessionSource::Codex),
            &db_a,
            CursorPurpose::Ingest,
        ),
        incremental_cursor_path(
            home,
            Some(&SessionSource::Codex),
            &db_b,
            CursorPurpose::Ingest,
        )
    );
}

#[test]
fn metadata_cursor_is_independent_and_stops_before_missing_document() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let db_path = tmp.path().join("refine.db");
    fs::write(&db_path, "").unwrap();

    let ingest_path = incremental_cursor_path(
        home,
        Some(&SessionSource::Codex),
        &db_path,
        CursorPurpose::Ingest,
    );
    let metadata_path = incremental_cursor_path(
        home,
        Some(&SessionSource::Codex),
        &db_path,
        CursorPurpose::Metadata,
    );
    assert_ne!(ingest_path, metadata_path);

    let scan_start = UNIX_EPOCH + Duration::from_secs(20_000);
    let missing_mtime = UNIX_EPOCH + Duration::from_secs(12_000);
    let failure = cursor_failure(
        Path::new("/tmp/missing.jsonl"),
        missing_mtime,
        "missing_document",
    );
    let watermark = safe_cursor_watermark(scan_start, &[missing_mtime]);
    let state = IngestCursorState {
        version: INGEST_CURSOR_VERSION,
        watermark_secs: unix_seconds(watermark),
        failures: vec![failure],
    };
    write_ingest_cursor_at(&metadata_path, &state).unwrap();

    let loaded: IngestCursorState =
        serde_json::from_str(&fs::read_to_string(&metadata_path).unwrap()).unwrap();
    assert_eq!(loaded.watermark_secs, 11_999);
    assert_eq!(loaded.failures[0].reason, "missing_document");
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
    let path = incremental_cursor_path(home, source, db_path, CursorPurpose::Ingest);
    let secs: u64 = std::fs::read_to_string(path).ok()?.trim().parse().ok()?;
    Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
}

fn write_last_ingest_mtime_at(
    home: &Path,
    source: Option<&SessionSource>,
    db_path: &Path,
    t: SystemTime,
) {
    let path = incremental_cursor_path(home, source, db_path, CursorPurpose::Ingest);
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
        project_identity: Some("refine".to_string()),
        mode: SessionMode::Interactive,
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
    assert!(linked_items.iter().all(|item| item
        .tags()
        .iter()
        .any(|tag| tag.as_str() == "session_mode_interactive")));
}

#[tokio::test]
async fn process_single_session_refresh_replaces_old_items_without_duplicate_transcript() {
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
        project_identity: Some("refine".to_string()),
        mode: SessionMode::Unknown,
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
    assert!(saved_doc.raw_content().is_empty());
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
async fn remem_save_reparents_superseded_legacy_facets() {
    let store = Arc::new(SqliteStore::in_memory().expect("in-memory sqlite store"));
    let item_store: Arc<dyn ItemRepository> = store.clone();
    let doc_store: Arc<dyn DocumentRepository> = store.clone();

    let mut legacy_doc = Document::new("codex-session", "User: old\n");
    legacy_doc.set_url("/tmp/rollout-2026-session-1.jsonl");
    doc_store.save(&legacy_doc).await.unwrap();
    let mut legacy_item = Item::new_observation("legacy", "legacy");
    legacy_item.set_document_id(legacy_doc.id().clone());
    legacy_item
        .set_tags(vec![
            Tag::new("custom-user-tag").unwrap(),
            Tag::new("session_mode_unknown").unwrap(),
        ])
        .unwrap();
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
        project_identity: Some("refine".to_string()),
        mode: SessionMode::Unattended,
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
    let replacement_items = item_store
        .find_by_document_id(replacement.id())
        .await
        .unwrap();
    assert!(replacement_items
        .iter()
        .any(|item| item.id() == &legacy_item_id));
    let migrated_legacy_item = replacement_items
        .iter()
        .find(|item| item.id() == &legacy_item_id)
        .expect("legacy item should be reparented");
    assert!(migrated_legacy_item
        .tags()
        .iter()
        .any(|tag| tag.as_str() == "custom-user-tag"));
    assert_eq!(
        migrated_legacy_item
            .tags()
            .iter()
            .filter(|tag| tag.as_str().starts_with("session_mode_"))
            .map(|tag| tag.as_str())
            .collect::<Vec<_>>(),
        vec!["session_mode_unattended"]
    );
    assert!(replacement_items.iter().all(|item| item
        .tags()
        .iter()
        .any(|tag| tag.as_str() == "session_mode_unattended")));
    let observations = item_store
        .find_by_type(ItemType::Observation)
        .await
        .unwrap();
    assert_eq!(observations.len(), replacement_item_count + 1);
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
            &[],
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

#[tokio::test]
async fn same_snapshot_mode_change_retags_without_llm() {
    let store = Arc::new(SqliteStore::in_memory().expect("in-memory sqlite store"));
    let doc_store: Arc<dyn DocumentRepository> = store.clone();
    let item_store: Arc<dyn ItemRepository> = store;
    let summary = RememSessionSummary {
        session_ref: concat!(
            "remem://raw-session/v2/",
            "636f6465782d636c69/6c6f63616c/2f7265706f/7331"
        )
        .to_string(),
        host: "codex-cli".to_string(),
        session_mode: "unattended".to_string(),
        source_root: "local".to_string(),
        project: "/repo".to_string(),
        session_id: "s1".to_string(),
        first_epoch: 10,
        last_epoch: 20,
        message_count: 2,
        user_message_count: 1,
        assistant_message_count: 1,
        content_hash: format!("sha256:{}", "a".repeat(64)),
        user_message_samples: vec!["question".to_string()],
        legacy_identity_is_unique: true,
    };
    let mut existing = Document::new("codex-session", "duplicated transcript body");
    existing.set_url(&summary.stable_document_url());
    existing.set_source_version(Some(&format!("{}:interactive", summary.content_hash)));
    doc_store
        .save(&existing)
        .await
        .expect("seed existing document");
    let mut existing_item = Item::new_observation("existing", "existing");
    existing_item.set_document_id(existing.id().clone());
    existing_item
        .set_tags(vec![
            Tag::new("custom-user-tag").unwrap(),
            Tag::new("session_mode_interactive").unwrap(),
        ])
        .unwrap();
    item_store.save(&existing_item).await.unwrap();
    let expected_version = summary.projection_version();
    let temp = tempfile::tempdir().expect("temporary lock directory");

    let quarantine = QuarantineStore::load_from(temp.path().join("quarantine.jsonl")).unwrap();
    handle_remem_ingest_sessions_with_loader(
        IngestOptions {
            source: None,
            provider: IngestProvider::Remem,
            limit: None,
            latest: Some(1),
            dry_run: false,
            retry_quarantined: false,
            backfill_session_metadata: false,
        },
        &temp.path().join("refine.db"),
        vec![summary],
        Some(quarantine),
        |summary| {
            Ok(loaded_remem_session(
                &summary,
                "ordinary user question with enough useful detail",
            ))
        },
        doc_store.clone(),
        None,
    )
    .await
    .expect("mode-only change must not require an LLM");

    let saved = doc_store
        .find_by_url(existing.url())
        .await
        .expect("query referenced document")
        .expect("referenced document remains present");
    assert_eq!(saved.id(), existing.id());
    assert!(saved.raw_content().is_empty());
    assert_eq!(saved.source(), "codex-session");
    assert_eq!(saved.source_version(), Some(expected_version.as_str()));
    let saved_item = item_store
        .find_by_document_id(saved.id())
        .await
        .unwrap()
        .into_iter()
        .find(|item| item.id() == existing_item.id())
        .expect("existing observation should survive mode retagging");
    assert!(saved_item
        .tags()
        .iter()
        .any(|tag| tag.as_str() == "custom-user-tag"));
    assert_eq!(
        saved_item
            .tags()
            .iter()
            .filter(|tag| tag.as_str().starts_with("session_mode_"))
            .map(|tag| tag.as_str())
            .collect::<Vec<_>>(),
        vec!["session_mode_unattended"]
    );
}

#[tokio::test]
async fn changed_remem_projection_does_not_delete_its_canonical_document() {
    let store = Arc::new(SqliteStore::in_memory().expect("in-memory sqlite store"));
    let doc_store: Arc<dyn DocumentRepository> = store.clone();
    let item_store: Arc<dyn ItemRepository> = store;
    let summary = remem_summary("refresh", 20, 'b');
    let expected_url = summary.stable_document_url();
    let expected_version = summary.projection_version();
    let mut existing = Document::new("codex-session", "");
    existing.set_url(&expected_url);
    existing.set_source_version(Some(&format!("sha256:{}:interactive", "a".repeat(64))));
    existing.set_captured_at(Utc.timestamp_opt(summary.first_epoch, 0).unwrap());
    doc_store.save(&existing).await.unwrap();

    let client: Arc<dyn LlmClient> = Arc::new(StaticLlmClient {
        response: r#"{
            "session_summary": "refreshed projection",
            "cognitive_level": "proficient",
            "collaboration_mode": "pair_programming",
            "decisions": ["keep canonical document"],
            "bugs_fixed": [], "patterns": [], "friction": [],
            "project_progress": [], "questions": [], "knowledge_gained": [],
            "tools_discovered": [], "architecture": [], "code_artifacts": []
        }"#
        .to_string(),
    });
    let temp = tempfile::tempdir().expect("temporary quarantine directory");
    let quarantine = QuarantineStore::load_from(temp.path().join("quarantine.jsonl")).unwrap();

    handle_remem_ingest_sessions_with_loader(
        IngestOptions {
            source: None,
            provider: IngestProvider::Remem,
            limit: None,
            latest: Some(1),
            dry_run: false,
            retry_quarantined: false,
            backfill_session_metadata: false,
        },
        &temp.path().join("refine.db"),
        vec![summary],
        Some(quarantine),
        |summary| {
            Ok(loaded_remem_session(
                &summary,
                "ordinary user question with enough useful detail",
            ))
        },
        doc_store.clone(),
        Some(client),
    )
    .await
    .expect("changed projection should refresh in place");

    let refreshed = doc_store
        .find_by_url(existing.url())
        .await
        .unwrap()
        .expect("canonical Remem document must remain present");
    assert_eq!(refreshed.id(), existing.id());
    assert_eq!(refreshed.source_version(), Some(expected_version.as_str()));
    assert!(!item_store
        .find_by_document_id(refreshed.id())
        .await
        .unwrap()
        .is_empty());
}

fn remem_summary(session_id: &str, last_epoch: i64, hash_byte: char) -> RememSessionSummary {
    RememSessionSummary {
        session_ref: format!("remem://raw-session/v2/codex/local/repo/{session_id}"),
        host: "codex-cli".to_string(),
        session_mode: "interactive".to_string(),
        source_root: "local".to_string(),
        project: "/repo".to_string(),
        session_id: session_id.to_string(),
        first_epoch: last_epoch - 1,
        last_epoch,
        message_count: 2,
        user_message_count: 1,
        assistant_message_count: 1,
        content_hash: format!("sha256:{}", hash_byte.to_string().repeat(64)),
        user_message_samples: vec!["ordinary user question".to_string()],
        legacy_identity_is_unique: true,
    }
}

fn loaded_remem_session(summary: &RememSessionSummary, first_user_message: &str) -> RememSession {
    RememSession {
        session_ref: summary.session_ref.clone(),
        source_root: summary.source_root.clone(),
        project: summary.project.clone(),
        session_id: summary.session_id.clone(),
        first_epoch: summary.first_epoch,
        session: refine_core::session::Session {
            source: SessionSource::Codex,
            file_path: PathBuf::from(&summary.session_ref),
            messages: vec![
                refine_core::session::SessionMessage {
                    role: refine_core::session::MessageRole::User,
                    content: first_user_message.to_string(),
                },
                refine_core::session::SessionMessage {
                    role: refine_core::session::MessageRole::Assistant,
                    content: "answer ".repeat(100),
                },
            ],
            meta: refine_core::session::SessionMeta {
                mode: match summary.session_mode.as_str() {
                    "interactive" => SessionMode::Interactive,
                    "unattended" => SessionMode::Unattended,
                    "subagent" => SessionMode::Subagent,
                    "unknown" => SessionMode::Unknown,
                    other => panic!("unsupported test session mode {other:?}"),
                },
                ..Default::default()
            },
        },
    }
}

#[tokio::test]
async fn latest_counts_only_eligible_pending_sessions_and_stops_loading_older_bodies() {
    let store = Arc::new(SqliteStore::in_memory().expect("in-memory sqlite store"));
    let doc_store: Arc<dyn DocumentRepository> = store.clone();
    let duplicate = remem_summary("duplicate", 500, 'a');
    let mut looper = remem_summary("looper", 400, 'b');
    looper.user_message_samples =
        vec!["You are executing Looper scheduled skill \"daily\". Follow the spec.".to_string()];
    let quarantined = remem_summary("quarantined", 300, 'c');
    let eligible = remem_summary("eligible", 200, 'd');
    let older = remem_summary("older", 100, 'e');

    let mut existing = Document::new("codex-session", "");
    existing.set_url(&duplicate.stable_document_url());
    existing.set_source_version(Some(&duplicate.projection_version()));
    doc_store.save(&existing).await.expect("seed duplicate");

    let temp = tempfile::tempdir().expect("temporary ingest paths");
    let mut quarantine = QuarantineStore::load_from(temp.path().join("quarantine.jsonl")).unwrap();
    quarantine.record(
        &quarantined.stable_document_url(),
        Some(&quarantined.projection_version()),
        "provider_rejected",
        "fixture",
    );
    let loaded_ids = Arc::new(Mutex::new(Vec::new()));
    let observed_ids = loaded_ids.clone();

    handle_remem_ingest_sessions_with_loader(
        IngestOptions {
            source: None,
            provider: IngestProvider::Remem,
            limit: None,
            latest: Some(1),
            dry_run: true,
            retry_quarantined: false,
            backfill_session_metadata: false,
        },
        &temp.path().join("refine.db"),
        // Deliberately unsorted: Refine owns newest-first selection.
        vec![older, eligible, quarantined, looper, duplicate],
        Some(quarantine),
        move |summary| {
            observed_ids
                .lock()
                .expect("loaded id lock")
                .push(summary.session_id.clone());
            let user_message = if summary.session_id == "looper" {
                "You are executing Looper scheduled skill \"daily\".\nFollow the spec."
            } else {
                "ordinary user question with enough useful detail"
            };
            Ok(loaded_remem_session(&summary, user_message))
        },
        doc_store.clone(),
        None,
    )
    .await
    .expect("dry-run should select one eligible pending session");

    assert_eq!(
        *loaded_ids.lock().expect("loaded id lock"),
        vec![
            "looper".to_string(),
            "eligible".to_string()
        ],
        "unchanged rows skip full loading, while Looper and quarantine do not consume the bound and older bodies stop after it is full"
    );
    assert_eq!(
        doc_store.count().await.unwrap(),
        1,
        "dry-run must not mutate"
    );
}

#[tokio::test]
async fn quarantined_looper_clears_stable_and_legacy_items_without_consuming_latest() {
    let store = Arc::new(SqliteStore::in_memory().expect("in-memory sqlite store"));
    let doc_store: Arc<dyn DocumentRepository> = store.clone();
    let item_store: Arc<dyn ItemRepository> = store.clone();
    let mut looper = remem_summary("looper", 300, 'c');
    looper.user_message_samples =
        vec!["You are executing Looper scheduled skill \"daily\". Follow the spec.".to_string()];
    let mut eligible = remem_summary("eligible", 200, 'd');
    eligible.session_mode = "unattended".to_string();
    let eligible_url = eligible.stable_document_url();

    let mut stable = Document::new("codex-session", "");
    stable.set_url(&looper.stable_document_url());
    stable.set_source_version(Some(&looper.projection_version()));
    doc_store.save(&stable).await.unwrap();
    let mut stable_item = Item::new_observation("stale stable", "stale stable");
    stable_item.set_document_id(stable.id().clone());
    item_store.save(&stable_item).await.unwrap();

    let mut legacy = Document::new("codex-session", "old Looper body");
    legacy.set_url(&looper.legacy_document_url());
    doc_store.save(&legacy).await.unwrap();
    let mut legacy_item = Item::new_observation("stale legacy", "stale legacy");
    legacy_item.set_document_id(legacy.id().clone());
    item_store.save(&legacy_item).await.unwrap();

    let mut unrelated = Document::new("codex-session", "unrelated");
    unrelated.set_url("remem://raw-session/v2/unrelated");
    doc_store.save(&unrelated).await.unwrap();
    let mut unrelated_item = Item::new_observation("keep", "keep");
    unrelated_item.set_document_id(unrelated.id().clone());
    item_store.save(&unrelated_item).await.unwrap();

    let temp = tempfile::tempdir().expect("temporary ingest paths");
    let quarantine_path = temp.path().join("quarantine.jsonl");
    let mut quarantine = QuarantineStore::load_from(quarantine_path.clone()).unwrap();
    quarantine.record(
        &looper.stable_document_url(),
        Some(&looper.projection_version()),
        "provider_rejected",
        "fixture",
    );
    quarantine.save_if_dirty().unwrap();
    drop(quarantine);
    let quarantine = QuarantineStore::load_from(quarantine_path.clone()).unwrap();
    let loaded_ids = Arc::new(Mutex::new(Vec::new()));
    let observed_ids = loaded_ids.clone();
    let client: Arc<dyn LlmClient> = Arc::new(StaticLlmClient {
        response: r#"{
            "session_summary": "eligible replacement",
            "cognitive_level": "competent", "collaboration_mode": "review",
            "decisions": [], "bugs_fixed": [], "patterns": [], "friction": [],
            "project_progress": [], "questions": [], "knowledge_gained": [],
            "tools_discovered": [], "architecture": [], "code_artifacts": []
        }"#
        .to_string(),
    });

    handle_remem_ingest_sessions_with_loader(
        IngestOptions {
            source: None,
            provider: IngestProvider::Remem,
            limit: None,
            latest: Some(1),
            dry_run: false,
            retry_quarantined: false,
            backfill_session_metadata: false,
        },
        &temp.path().join("refine.db"),
        vec![eligible, looper],
        Some(quarantine),
        move |summary| {
            observed_ids
                .lock()
                .expect("loaded id lock")
                .push(summary.session_id.clone());
            let first_user = if summary.session_id == "looper" {
                "You are executing Looper scheduled skill \"daily\".\nFollow the spec."
            } else {
                "ordinary user question with enough useful detail"
            };
            Ok(loaded_remem_session(&summary, first_user))
        },
        doc_store.clone(),
        Some(client),
    )
    .await
    .expect("a quarantined Looper cleanup must resolve its obsolete rejection");

    assert_eq!(
        *loaded_ids.lock().expect("loaded id lock"),
        vec!["looper".to_string(), "eligible".to_string()]
    );
    assert!(item_store
        .find_by_document_id(stable.id())
        .await
        .unwrap()
        .is_empty());
    assert!(doc_store.find_by_id(legacy.id()).await.unwrap().is_none());
    assert!(!item_store.exists(legacy_item.id()).await.unwrap());
    assert!(item_store.exists(unrelated_item.id()).await.unwrap());
    let eligible_document = doc_store
        .find_by_url(&eligible_url)
        .await
        .unwrap()
        .expect("eligible Remem document");
    let eligible_items = item_store
        .find_by_document_id(eligible_document.id())
        .await
        .unwrap();
    assert!(!eligible_items.is_empty());
    assert!(eligible_items.iter().all(|item| item
        .tags()
        .iter()
        .any(|tag| tag.as_str() == "session_mode_unattended")));
    let quarantine = QuarantineStore::load_from(quarantine_path).unwrap();
    assert!(!quarantine.contains(stable.url(), stable.source_version()));
    assert_eq!(quarantine.len(), 0);
}

#[tokio::test]
async fn looper_cleanup_rolls_back_stable_and_legacy_item_changes_together() {
    let store = Arc::new(SqliteStore::in_memory().expect("in-memory sqlite store"));
    let doc_store: Arc<dyn DocumentRepository> = store.clone();
    let item_store: Arc<dyn ItemRepository> = store;
    let mut summary = remem_summary("looper", 300, 'c');
    summary.user_message_samples =
        vec!["You are executing Looper scheduled skill \"daily\". Follow the spec.".to_string()];
    let mut stable = Document::new("codex-session", "");
    stable.set_url(&summary.stable_document_url());
    stable.set_source_version(Some(&summary.projection_version()));
    doc_store.save(&stable).await.unwrap();
    let mut stable_item = Item::new_observation("stable", "stable");
    stable_item.set_document_id(stable.id().clone());
    item_store.save(&stable_item).await.unwrap();
    let mut legacy = Document::new("codex-session", "legacy");
    legacy.set_url(&summary.legacy_document_url());
    doc_store.save(&legacy).await.unwrap();
    let mut legacy_item = Item::new_observation("legacy", "legacy");
    legacy_item.set_document_id(legacy.id().clone());
    item_store.save(&legacy_item).await.unwrap();

    let error = legacy_convergence::exclude_scheduled_session_documents(
        &doc_store,
        Some(&stable),
        SessionSource::Codex,
        &summary.stable_document_url(),
        &summary.content_hash,
        &[
            legacy.id().clone(),
            DocumentId::from("missing-legacy-document"),
        ],
    )
    .await
    .expect_err("a failed obsolete delete must roll back the whole Looper cleanup");

    assert!(error.to_string().contains("does not exist"));
    assert!(item_store.exists(stable_item.id()).await.unwrap());
    assert!(item_store.exists(legacy_item.id()).await.unwrap());
    assert!(doc_store.find_by_id(legacy.id()).await.unwrap().is_some());
}

#[tokio::test]
async fn quarantined_latest_does_not_consume_quota_but_keeps_ingest_incomplete() {
    let store = Arc::new(SqliteStore::in_memory().expect("in-memory sqlite store"));
    let doc_store: Arc<dyn DocumentRepository> = store.clone();
    let quarantined = remem_summary("quarantined", 300, 'c');
    let eligible = remem_summary("eligible", 200, 'd');
    let eligible_url = eligible.stable_document_url();
    let temp = tempfile::tempdir().expect("temporary ingest paths");
    let mut quarantine = QuarantineStore::load_from(temp.path().join("quarantine.jsonl")).unwrap();
    quarantine.record(
        &quarantined.stable_document_url(),
        Some(&quarantined.projection_version()),
        "provider_rejected",
        "fixture",
    );
    let loaded_ids = Arc::new(Mutex::new(Vec::new()));
    let observed_ids = loaded_ids.clone();
    let client: Arc<dyn LlmClient> = Arc::new(StaticLlmClient {
        response: r#"{
            "session_summary": "eligible replacement",
            "cognitive_level": "competent",
            "collaboration_mode": "review",
            "decisions": [], "bugs_fixed": [], "patterns": [], "friction": [],
            "project_progress": [], "questions": [], "knowledge_gained": [],
            "tools_discovered": [], "architecture": [], "code_artifacts": []
        }"#
        .to_string(),
    });

    let error = handle_remem_ingest_sessions_with_loader(
        IngestOptions {
            source: None,
            provider: IngestProvider::Remem,
            limit: None,
            latest: Some(1),
            dry_run: false,
            retry_quarantined: false,
            backfill_session_metadata: false,
        },
        &temp.path().join("refine.db"),
        vec![eligible, quarantined],
        Some(quarantine),
        move |summary| {
            observed_ids
                .lock()
                .expect("loaded id lock")
                .push(summary.session_id.clone());
            Ok(loaded_remem_session(
                &summary,
                "ordinary user question with enough useful detail",
            ))
        },
        doc_store.clone(),
        Some(client),
    )
    .await
    .expect_err("a selected quarantined identity must keep scheduled ingestion incomplete");

    assert_eq!(
        *loaded_ids.lock().expect("loaded id lock"),
        vec!["eligible".to_string()],
        "the quarantined latest identity must not be retried or consume latest=1"
    );
    assert!(error.to_string().contains("摄入不完整"));
    assert!(error.to_string().contains("本次相关隔离 1"));
    assert!(
        doc_store
            .find_by_url(&eligible_url)
            .await
            .unwrap()
            .is_some(),
        "the eligible replacement must still be processed before final failure"
    );
}

#[tokio::test]
async fn omitting_latest_scans_every_eligible_session_body() {
    let store = Arc::new(SqliteStore::in_memory().expect("in-memory sqlite store"));
    let doc_store: Arc<dyn DocumentRepository> = store;
    let temp = tempfile::tempdir().expect("temporary ingest paths");
    let quarantine = QuarantineStore::load_from(temp.path().join("quarantine.jsonl")).unwrap();
    let loaded_ids = Arc::new(Mutex::new(Vec::new()));
    let observed_ids = loaded_ids.clone();

    handle_remem_ingest_sessions_with_loader(
        IngestOptions {
            source: None,
            provider: IngestProvider::Remem,
            limit: None,
            latest: None,
            dry_run: true,
            retry_quarantined: false,
            backfill_session_metadata: false,
        },
        &temp.path().join("refine.db"),
        vec![
            remem_summary("older", 100, 'a'),
            remem_summary("newer", 200, 'b'),
        ],
        Some(quarantine),
        move |summary| {
            observed_ids
                .lock()
                .expect("loaded id lock")
                .push(summary.session_id.clone());
            Ok(loaded_remem_session(
                &summary,
                "ordinary user question with enough useful detail",
            ))
        },
        doc_store,
        None,
    )
    .await
    .expect("unbounded manual dry-run should scan full history");

    assert_eq!(
        *loaded_ids.lock().expect("loaded id lock"),
        vec!["newer".to_string(), "older".to_string()]
    );
}
