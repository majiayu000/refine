use chrono::{Duration, TimeZone, Utc};
use refine_core::conversation::{
    now_iso, ConversationRecord, ConversationRepository, ConversationStatus, ExtractionJobRecord,
    ExtractionMode, JobRepository, JobStatus,
};
use refine_core::infra::SqliteStore;
use refine_core::knowledge::{
    Document, DocumentId, Item, ItemId, ItemRepository, ItemType, RestoreDocumentParams,
    RestoreParams, Source, Tag,
};
use serde_json::json;
use std::collections::HashSet;

#[tokio::test]
async fn read_only_store_never_runs_schema_migrations_or_accepts_writes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE marker (value TEXT NOT NULL);
             INSERT INTO marker VALUES ('unchanged');",
        )
        .unwrap();
    }
    let before = std::fs::read(&path).unwrap();
    let store = SqliteStore::open_read_only(&path).unwrap();
    let item = Item::new_knowledge("must fail", "read only");
    assert!(store.save(&item).await.is_err());
    drop(store);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let conn = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        conn.query_row("SELECT value FROM marker", [], |row| row
            .get::<_, String>(0))
            .unwrap(),
        "unchanged"
    );
    assert!(conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='documents'",
            [],
            |_| Ok(())
        )
        .is_err());
}

#[tokio::test]
async fn roundtrip_preserves_timestamps_and_content() {
    let store = SqliteStore::in_memory().expect("failed to create sqlite store");

    let created_at = Utc
        .with_ymd_and_hms(2026, 2, 6, 10, 0, 0)
        .single()
        .expect("invalid created_at");
    let updated_at = created_at + Duration::minutes(5);

    let item = Item::restore(RestoreParams {
        id: ItemId::from("item-1"),
        item_type: ItemType::Knowledge,
        title: "Rust Time Semantics".to_string(),
        summary: "Verify persisted timestamps are stable".to_string(),
        content: "Detailed content should survive roundtrip.".to_string(),
        tags: vec![Tag::new("rust").expect("invalid tag")],
        source: Some(Source::new("claude").with_url("https://claude.ai/chat/abc")),
        document_id: None,
        excerpt: None,
        created_at,
        updated_at,
    })
    .expect("failed to restore item");

    store.save(&item).await.expect("failed to save item");

    let loaded = store
        .find_by_id(item.id())
        .await
        .expect("query failed")
        .expect("item not found");

    assert_eq!(loaded.created_at(), created_at);
    assert_eq!(loaded.updated_at(), updated_at);
    assert_eq!(
        loaded.content(),
        "Detailed content should survive roundtrip."
    );

    // 读取后直接保存，不应发生时间漂移
    store
        .save(&loaded)
        .await
        .expect("failed to save loaded item");

    let loaded_again = store
        .find_by_id(item.id())
        .await
        .expect("query failed")
        .expect("item not found");

    assert_eq!(loaded_again.created_at(), created_at);
    assert_eq!(loaded_again.updated_at(), updated_at);
}

#[tokio::test]
async fn search_text_supports_offset_and_total_count() {
    let store = SqliteStore::in_memory().expect("failed to create sqlite store");

    for title in ["Rust A", "Rust B", "Rust C"] {
        let mut item = Item::new_knowledge(title, "summary");
        item.set_content("rust ownership and memory");
        store.save(&item).await.expect("save failed");
    }

    let total = store
        .count_text_hits("rust")
        .await
        .expect("count_text_hits failed");
    assert_eq!(total, 3);

    let first_page = store
        .search_text("rust", 0, 2)
        .await
        .expect("search_text first page failed");
    let second_page = store
        .search_text("rust", 2, 2)
        .await
        .expect("search_text second page failed");

    assert_eq!(first_page.len(), 2);
    assert_eq!(second_page.len(), 1);

    let first_ids: HashSet<String> = first_page
        .iter()
        .map(|item| item.id().to_string())
        .collect();
    let second_id = second_page[0].id().to_string();
    assert!(!first_ids.contains(&second_id));
}

#[tokio::test]
async fn document_save_refreshes_existing_url_content() {
    let store = SqliteStore::in_memory().expect("failed to create sqlite store");
    let url = "https://claude.ai/chat/duplicate";

    let mut first = Document::new("claude", "older content");
    first.set_title("Older title");
    first.set_url(url);
    refine_core::knowledge::DocumentRepository::save(&store, &first)
        .await
        .expect("failed to save first document");
    let first_updated_at = first.updated_at();

    let mut second = Document::new("claude", "newer content");
    second.set_title("Newer title");
    second.set_url(url);
    second.set_source_version(Some("source:v1:2"));
    let second_captured_at = second.captured_at();
    let second_updated_at = second.updated_at();
    refine_core::knowledge::DocumentRepository::save(&store, &second)
        .await
        .expect("failed to save second document");

    let by_url = refine_core::knowledge::DocumentRepository::find_by_url(&store, url)
        .await
        .expect("find_by_url failed")
        .expect("document not found by url");
    let by_id = refine_core::knowledge::DocumentRepository::find_by_id(&store, first.id())
        .await
        .expect("find_by_id failed")
        .expect("document not found by id");

    assert_eq!(by_url.id(), first.id());
    assert_eq!(by_url.title(), Some("Newer title"));
    assert_eq!(by_url.raw_content(), "newer content");
    assert_eq!(by_url.source_version(), Some("source:v1:2"));
    assert_eq!(by_url.captured_at(), second_captured_at);
    assert_eq!(by_url.updated_at(), second_updated_at);
    assert!(by_url.updated_at() >= first_updated_at);
    assert_eq!(by_id.title(), Some("Newer title"));
    assert_eq!(by_id.raw_content(), "newer content");
    assert_eq!(by_id.source_version(), Some("source:v1:2"));
    assert_eq!(by_id.captured_at(), second_captured_at);
    assert_eq!(by_id.updated_at(), second_updated_at);
}

#[tokio::test]
async fn document_save_corrects_source_and_preserves_identity_and_title_for_duplicate_url() {
    let store = SqliteStore::in_memory().expect("failed to create sqlite store");
    let url = "https://claude.ai/chat/duplicate-no-title";

    let mut first = Document::new("remem-raw-session", "older content");
    first.set_title("Canonical title");
    first.set_url(url);
    refine_core::knowledge::DocumentRepository::save(&store, &first)
        .await
        .expect("failed to save first document");

    let mut second = Document::new("codex-session", "newer content");
    second.set_url(url);
    let second_captured_at = second.captured_at();
    let second_updated_at = second.updated_at();
    refine_core::knowledge::DocumentRepository::save(&store, &second)
        .await
        .expect("failed to save second document");

    let by_url = refine_core::knowledge::DocumentRepository::find_by_url(&store, url)
        .await
        .expect("find_by_url failed")
        .expect("document not found by url");
    let by_id = refine_core::knowledge::DocumentRepository::find_by_id(&store, first.id())
        .await
        .expect("find_by_id failed")
        .expect("document not found by id");

    assert_eq!(by_url.id(), first.id());
    assert_eq!(by_url.title(), Some("Canonical title"));
    assert_eq!(by_url.raw_content(), "newer content");
    assert_eq!(by_url.source(), "codex-session");
    assert_eq!(by_url.captured_at(), second_captured_at);
    assert_eq!(by_url.updated_at(), second_updated_at);
    assert_eq!(by_id.title(), Some("Canonical title"));
    assert_eq!(by_id.raw_content(), "newer content");
    assert_eq!(by_id.source(), "codex-session");
    assert_eq!(by_id.captured_at(), second_captured_at);
    assert_eq!(by_id.updated_at(), second_updated_at);
}

#[tokio::test]
async fn document_find_recent_orders_by_latest_capture_after_duplicate_url_refresh() {
    let store = SqliteStore::in_memory().expect("failed to create sqlite store");

    let older_capture = Utc
        .with_ymd_and_hms(2026, 2, 6, 10, 0, 0)
        .single()
        .expect("invalid older capture");
    let middle_capture = older_capture + Duration::minutes(10);
    let newest_capture = older_capture + Duration::minutes(20);

    let older = Document::restore(RestoreDocumentParams {
        id: DocumentId::from("doc-older"),
        title: Some("Older".to_string()),
        raw_content: "older content".to_string(),
        source: "claude".to_string(),
        url: "https://claude.ai/chat/older".to_string(),
        source_version: None,
        captured_at: older_capture,
        created_at: older_capture,
        updated_at: older_capture,
    });
    refine_core::knowledge::DocumentRepository::save(&store, &older)
        .await
        .expect("failed to save older document");

    let newer = Document::restore(RestoreDocumentParams {
        id: DocumentId::from("doc-newer"),
        title: Some("Newer".to_string()),
        raw_content: "newer content".to_string(),
        source: "claude".to_string(),
        url: "https://claude.ai/chat/newer".to_string(),
        source_version: None,
        captured_at: middle_capture,
        created_at: middle_capture,
        updated_at: middle_capture,
    });
    refine_core::knowledge::DocumentRepository::save(&store, &newer)
        .await
        .expect("failed to save newer document");

    let recaptured = Document::restore(RestoreDocumentParams {
        id: DocumentId::from("doc-recaptured"),
        title: Some("Recaptured".to_string()),
        raw_content: "recaptured content".to_string(),
        source: "claude".to_string(),
        url: older.url().to_string(),
        source_version: None,
        captured_at: newest_capture,
        created_at: newest_capture,
        updated_at: newest_capture,
    });
    refine_core::knowledge::DocumentRepository::save(&store, &recaptured)
        .await
        .expect("failed to save recaptured document");

    let recent = refine_core::knowledge::DocumentRepository::find_recent(&store, 0, 10)
        .await
        .expect("find_recent failed");

    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].id(), older.id());
    assert_eq!(recent[0].captured_at(), newest_capture);
    assert_eq!(recent[0].raw_content(), "recaptured content");
    assert_eq!(recent[1].id(), newer.id());
}

#[tokio::test]
async fn item_document_id_requires_existing_document() {
    let store = SqliteStore::in_memory().expect("failed to create sqlite store");
    let now = Utc::now();
    let orphan = Item::restore(RestoreParams {
        id: ItemId::from("orphan-item"),
        item_type: ItemType::Knowledge,
        title: "orphan".to_string(),
        summary: "summary".to_string(),
        content: "content".to_string(),
        tags: Vec::new(),
        source: None,
        document_id: Some(DocumentId::from("missing-doc")),
        excerpt: None,
        created_at: now,
        updated_at: now,
    })
    .expect("restore orphan item");

    let err = store
        .save(&orphan)
        .await
        .expect_err("missing document FK should reject orphan item");
    assert!(
        err.to_string().contains("FOREIGN KEY constraint failed"),
        "unexpected error: {}",
        err
    );
}

#[tokio::test]
async fn direct_document_delete_rejects_linked_items_without_detaching_them() {
    let store = SqliteStore::in_memory().expect("failed to create sqlite store");
    let mut document = Document::new("claude", "raw content");
    document.set_url("https://claude.ai/chat/restrict-delete");
    let mut item = Item::new_knowledge("Restrict linked item", "summary");
    item.set_document_id(document.id().clone());
    refine_core::knowledge::DocumentRepository::save_with_replaced_items(
        &store,
        &document,
        &[item.clone()],
    )
    .await
    .expect("save linked aggregate");

    let error = refine_core::knowledge::DocumentRepository::delete(&store, document.id())
        .await
        .expect_err("direct parent deletion must fail while linked items exist");
    assert!(
        error.to_string().contains("FOREIGN KEY constraint failed"),
        "unexpected error: {error}"
    );

    assert!(
        refine_core::knowledge::DocumentRepository::find_by_id(&store, document.id())
            .await
            .expect("find preserved document")
            .is_some()
    );
    let preserved = store
        .find_by_id(item.id())
        .await
        .expect("find preserved item")
        .expect("linked item should remain");
    assert_eq!(preserved.document_id(), Some(document.id()));
    assert_eq!(
        store
            .search_text("restrict", 0, 10)
            .await
            .expect("search preserved FTS entry")
            .len(),
        1
    );
}

#[tokio::test]
async fn aggregate_document_delete_removes_items_and_conversation_references_atomically() {
    let store = SqliteStore::in_memory().expect("failed to create sqlite store");
    let mut document = Document::new("claude", "raw content");
    document.set_url("https://claude.ai/chat/aggregate-delete");
    let mut item = Item::new_knowledge("Aggregate delete item", "summary");
    item.set_document_id(document.id().clone());
    refine_core::knowledge::DocumentRepository::save_with_replaced_items(
        &store,
        &document,
        &[item.clone()],
    )
    .await
    .expect("save linked aggregate");

    let mut conversation =
        build_conversation("conv-aggregate-delete", ConversationStatus::Processed);
    conversation.item_ids = vec![item.id().to_string()];
    store
        .upsert_conversation(&conversation)
        .await
        .expect("save conversation reference");

    refine_core::knowledge::DocumentRepository::delete_documents_with_items(
        &store,
        &[document.id().clone()],
    )
    .await
    .expect("delete aggregate");

    assert!(
        refine_core::knowledge::DocumentRepository::find_by_id(&store, document.id())
            .await
            .expect("find deleted document")
            .is_none()
    );
    assert!(store
        .find_by_id(item.id())
        .await
        .expect("find deleted item")
        .is_none());
    let loaded_conversation = store
        .find_conversation_by_id(&conversation.id)
        .await
        .expect("find conversation")
        .expect("conversation should remain");
    assert!(loaded_conversation.item_ids.is_empty());
    assert!(store
        .search_text("aggregate", 0, 10)
        .await
        .expect("search deleted FTS entry")
        .is_empty());
}

#[tokio::test]
async fn aggregate_document_delete_failure_rolls_back_items_documents_and_references() {
    let store = SqliteStore::in_memory().expect("failed to create sqlite store");
    let mut document = Document::new("claude", "raw content");
    document.set_url("https://claude.ai/chat/aggregate-rollback");
    let mut item = Item::new_knowledge("Rollback linked item", "summary");
    item.set_document_id(document.id().clone());
    refine_core::knowledge::DocumentRepository::save_with_replaced_items(
        &store,
        &document,
        &[item.clone()],
    )
    .await
    .expect("save linked aggregate");

    let mut conversation =
        build_conversation("conv-aggregate-rollback", ConversationStatus::Processed);
    conversation.item_ids = vec![item.id().to_string()];
    store
        .upsert_conversation(&conversation)
        .await
        .expect("save conversation reference");

    let error = refine_core::knowledge::DocumentRepository::delete_documents_with_items(
        &store,
        &[document.id().clone(), DocumentId::from("missing-document")],
    )
    .await
    .expect_err("missing second document must roll back the whole transaction");
    assert!(error.to_string().contains("missing-document"));

    assert!(
        refine_core::knowledge::DocumentRepository::find_by_id(&store, document.id())
            .await
            .expect("find rolled back document")
            .is_some()
    );
    let preserved = store
        .find_by_id(item.id())
        .await
        .expect("find rolled back item")
        .expect("item delete should roll back");
    assert_eq!(preserved.document_id(), Some(document.id()));
    let loaded_conversation = store
        .find_conversation_by_id(&conversation.id)
        .await
        .expect("find conversation")
        .expect("conversation should remain");
    assert_eq!(loaded_conversation.item_ids, vec![item.id().to_string()]);
    assert_eq!(
        store
            .search_text("rollback", 0, 10)
            .await
            .expect("search rolled back FTS entry")
            .len(),
        1
    );
}

#[tokio::test]
async fn save_with_replaced_items_replaces_document_items_in_one_call() {
    let store = SqliteStore::in_memory().expect("failed to create sqlite store");
    let mut doc = Document::new("claude", "raw content");
    doc.set_url("https://claude.ai/chat/replace-items");

    let mut old_item = Item::new_knowledge("old", "old summary");
    old_item.set_document_id(doc.id().clone());
    let mut new_item = Item::new_knowledge("new", "new summary");
    new_item.set_document_id(doc.id().clone());

    refine_core::knowledge::DocumentRepository::save_with_replaced_items(
        &store,
        &doc,
        &[old_item.clone()],
    )
    .await
    .expect("save old item");
    refine_core::knowledge::DocumentRepository::save_with_replaced_items(
        &store,
        &doc,
        &[new_item.clone()],
    )
    .await
    .expect("replace item");

    assert!(store
        .find_by_id(old_item.id())
        .await
        .expect("find old item")
        .is_none());
    assert!(store
        .find_by_id(new_item.id())
        .await
        .expect("find new item")
        .is_some());
    let linked = store
        .find_by_document_id(doc.id())
        .await
        .expect("find linked items");
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].id(), new_item.id());
}

#[tokio::test]
async fn save_with_replaced_items_prunes_replaced_item_ids_from_conversations() {
    let store = SqliteStore::in_memory().expect("failed to create sqlite store");
    let mut doc = Document::new("claude", "raw content");
    doc.set_url("https://claude.ai/chat/prune-replaced-items");

    let mut old_item = Item::new_knowledge("old", "old summary");
    old_item.set_document_id(doc.id().clone());
    let mut new_item = Item::new_knowledge("new", "new summary");
    new_item.set_document_id(doc.id().clone());
    let kept_item = Item::new_knowledge("keep", "keep summary");

    store.save(&kept_item).await.expect("save kept item");
    refine_core::knowledge::DocumentRepository::save_with_replaced_items(
        &store,
        &doc,
        &[old_item.clone()],
    )
    .await
    .expect("save old document item");

    let mut conversation = build_conversation("conv-prune-replaced", ConversationStatus::Processed);
    conversation.item_ids = vec![old_item.id().to_string(), kept_item.id().to_string()];
    store
        .upsert_conversation(&conversation)
        .await
        .expect("insert conversation with item refs");

    refine_core::knowledge::DocumentRepository::save_with_replaced_items(
        &store,
        &doc,
        &[new_item.clone()],
    )
    .await
    .expect("replace document items");

    let loaded = store
        .find_conversation_by_id(&conversation.id)
        .await
        .expect("find conversation")
        .expect("conversation should exist");
    assert_eq!(loaded.item_ids, vec![kept_item.id().to_string()]);
}

#[tokio::test]
async fn find_recent_and_count_items_respect_type_and_pagination() {
    let store = SqliteStore::in_memory().expect("failed to create sqlite store");

    store
        .save(&Item::new_knowledge("K1", "knowledge one"))
        .await
        .expect("save failed");
    store
        .save(&Item::new_skill("S1", "skill one"))
        .await
        .expect("save failed");
    store
        .save(&Item::new_knowledge("K2", "knowledge two"))
        .await
        .expect("save failed");

    let total_all = store.count_items(None).await.expect("count all failed");
    let total_knowledge = store
        .count_items(Some(ItemType::Knowledge))
        .await
        .expect("count knowledge failed");
    assert_eq!(total_all, 3);
    assert_eq!(total_knowledge, 2);

    let recent_knowledge = store
        .find_recent(Some(ItemType::Knowledge), 0, 1)
        .await
        .expect("find_recent knowledge failed");
    assert_eq!(recent_knowledge.len(), 1);
    assert!(matches!(
        recent_knowledge[0].item_type(),
        ItemType::Knowledge
    ));

    let paged_all = store
        .find_recent(None, 1, 10)
        .await
        .expect("find_recent all failed");
    assert_eq!(paged_all.len(), 2);
}

#[tokio::test]
async fn conversation_upsert_rejects_invalid_status_regression() {
    let store = SqliteStore::in_memory().expect("failed to create sqlite store");
    let mut conversation =
        build_conversation("conv-invalid-transition", ConversationStatus::Processed);

    store
        .upsert_conversation(&conversation)
        .await
        .expect("insert processed conversation");

    conversation.status = ConversationStatus::Queued;
    let err = store
        .upsert_conversation(&conversation)
        .await
        .expect_err("processed conversations must not regress to queued");
    assert!(
        err.to_string()
            .contains("invalid conversation status transition"),
        "unexpected error: {}",
        err
    );

    let loaded = store
        .find_conversation_by_id(&conversation.id)
        .await
        .expect("find conversation")
        .expect("conversation should exist");
    assert_eq!(loaded.status, ConversationStatus::Processed);
}

#[tokio::test]
async fn deleting_item_prunes_conversation_item_ids() {
    let store = SqliteStore::in_memory().expect("failed to create sqlite store");
    let deleted_item = Item::new_knowledge("delete me", "summary");
    let kept_item = Item::new_knowledge("keep me", "summary");

    store.save(&deleted_item).await.expect("save deleted item");
    store.save(&kept_item).await.expect("save kept item");

    let mut conversation = build_conversation("conv-prune-item", ConversationStatus::Processed);
    conversation.item_ids = vec![deleted_item.id().to_string(), kept_item.id().to_string()];
    store
        .upsert_conversation(&conversation)
        .await
        .expect("insert conversation with item refs");

    let deleted = store
        .delete(deleted_item.id())
        .await
        .expect("delete referenced item");
    assert!(deleted);

    let loaded = store
        .find_conversation_by_id(&conversation.id)
        .await
        .expect("find conversation")
        .expect("conversation should exist");
    assert_eq!(loaded.item_ids, vec![kept_item.id().to_string()]);
}

#[tokio::test]
async fn pending_job_can_record_startup_failure_without_regressing_terminal_state() {
    let store = SqliteStore::in_memory().expect("failed to create sqlite store");
    let now = now_iso();
    let conversation = build_conversation("conv-startup-failure", ConversationStatus::Queued);
    store
        .upsert_conversation(&conversation)
        .await
        .expect("insert parent conversation");
    let mut job = ExtractionJobRecord {
        id: "job-startup-failure".to_string(),
        conversation_id: conversation.id.clone(),
        mode: ExtractionMode::Auto,
        status: JobStatus::Pending,
        created_at: now.clone(),
        updated_at: now,
        error: None,
        attempt_count: 0,
        lease_owner: None,
        lease_expires_at: None,
    };

    store.upsert_job(&job).await.expect("insert pending job");
    job.status = JobStatus::Failed;
    job.error = Some("startup failed".to_string());
    store
        .upsert_job(&job)
        .await
        .expect("pending job should record startup failure");

    job.status = JobStatus::Running;
    let err = store
        .upsert_job(&job)
        .await
        .expect_err("failed jobs must not regress to running");
    assert!(
        err.to_string().contains("invalid job status transition"),
        "unexpected error: {}",
        err
    );

    let loaded = store
        .find_job_by_id(&job.id)
        .await
        .expect("find job")
        .expect("job should exist");
    assert_eq!(loaded.status, JobStatus::Failed);
    assert_eq!(loaded.error.as_deref(), Some("startup failed"));
}

#[tokio::test]
async fn same_document_id_url_migration_rolls_back_with_its_items() {
    let store = SqliteStore::in_memory().expect("failed to create sqlite store");
    let mut original = Document::new("codex-session", "transcript");
    original.set_url("remem-raw://v1/local/repo/session");
    refine_core::knowledge::DocumentRepository::save(&store, &original)
        .await
        .expect("seed original document");
    let mut item = Item::new_observation("preserved", "preserved");
    item.set_document_id(original.id().clone());
    ItemRepository::save(&store, &item)
        .await
        .expect("seed original item");
    let migrated = Document::restore(RestoreDocumentParams {
        id: original.id().clone(),
        title: original.title().map(ToOwned::to_owned),
        raw_content: String::new(),
        source: original.source().to_string(),
        url: "remem-raw://v2/host/local/repo/session".to_string(),
        source_version: Some("sha256:snapshot".to_string()),
        captured_at: original.captured_at(),
        created_at: original.created_at(),
        updated_at: original.updated_at(),
    });

    refine_core::knowledge::DocumentRepository::save_with_replaced_items_and_delete_documents(
        &store,
        &migrated,
        &[],
        std::slice::from_ref(original.id()),
        &[DocumentId::from("missing-obsolete-document")],
    )
    .await
    .expect_err("missing obsolete document must roll back URL migration");

    let restored = refine_core::knowledge::DocumentRepository::find_by_id(&store, original.id())
        .await
        .unwrap()
        .expect("original document restored by rollback");
    assert_eq!(restored.url(), original.url());
    assert!(
        refine_core::knowledge::DocumentRepository::find_by_url(&store, migrated.url())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        ItemRepository::find_by_document_id(&store, original.id())
            .await
            .unwrap()
            .into_iter()
            .map(|saved| saved.id().to_string())
            .collect::<Vec<_>>(),
        vec![item.id().to_string()]
    );
}

#[tokio::test]
async fn url_conflict_convergence_preserves_items_from_both_document_ids() {
    let store = SqliteStore::in_memory().expect("failed to create sqlite store");
    let mut canonical = Document::new("codex-session", "");
    canonical.set_url("remem://raw-session/v2/codex/local/repo/session");
    refine_core::knowledge::DocumentRepository::save(&store, &canonical)
        .await
        .expect("seed canonical document");
    let mut canonical_item = Item::new_observation("canonical", "canonical summary");
    canonical_item.set_content("canonical content");
    canonical_item.set_document_id(canonical.id().clone());
    ItemRepository::save(&store, &canonical_item)
        .await
        .expect("seed canonical item");

    let mut source = Document::new("remem-raw-session", "source transcript");
    source.set_url("remem-raw://v1/local/repo/session");
    refine_core::knowledge::DocumentRepository::save(&store, &source)
        .await
        .expect("seed source document");
    let mut source_item = Item::new_observation("source", "source summary");
    source_item.set_content("source content");
    source_item.set_document_id(source.id().clone());
    ItemRepository::save(&store, &source_item)
        .await
        .expect("seed source item");

    let mut obsolete = Document::new("codex-session", "obsolete transcript");
    obsolete.set_url("/tmp/rollout-session.jsonl");
    refine_core::knowledge::DocumentRepository::save(&store, &obsolete)
        .await
        .expect("seed obsolete document");
    let mut obsolete_item = Item::new_observation("obsolete", "obsolete summary");
    obsolete_item.set_content("obsolete content");
    obsolete_item.set_document_id(obsolete.id().clone());
    ItemRepository::save(&store, &obsolete_item)
        .await
        .expect("seed obsolete item");

    let referenced = Document::restore(RestoreDocumentParams {
        id: source.id().clone(),
        title: Some("referenced".to_string()),
        raw_content: String::new(),
        source: "codex-session".to_string(),
        url: canonical.url().to_string(),
        source_version: Some("sha256:snapshot".to_string()),
        captured_at: source.captured_at(),
        created_at: source.created_at(),
        updated_at: source.updated_at(),
    });
    let source_ids = vec![source.id().clone(), obsolete.id().clone()];
    refine_core::knowledge::DocumentRepository::save_with_replaced_items_and_delete_documents(
        &store,
        &referenced,
        &[],
        &source_ids,
        &source_ids,
    )
    .await
    .expect("converge both URL identities and obsolete document");

    let saved = refine_core::knowledge::DocumentRepository::find_by_url(&store, canonical.url())
        .await
        .unwrap()
        .expect("canonical document remains");
    assert_eq!(saved.id(), canonical.id());
    assert!(
        refine_core::knowledge::DocumentRepository::find_by_id(&store, source.id())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        refine_core::knowledge::DocumentRepository::find_by_id(&store, obsolete.id())
            .await
            .unwrap()
            .is_none()
    );
    let payloads = ItemRepository::find_by_document_id(&store, canonical.id())
        .await
        .unwrap()
        .into_iter()
        .map(|item| (item.id().to_string(), item.content().to_string()))
        .collect::<HashSet<_>>();
    assert_eq!(
        payloads,
        HashSet::from([
            (
                canonical_item.id().to_string(),
                "canonical content".to_string()
            ),
            (source_item.id().to_string(), "source content".to_string()),
            (
                obsolete_item.id().to_string(),
                "obsolete content".to_string()
            ),
        ])
    );
}

#[tokio::test]
async fn replacement_drops_old_canonical_item_but_keeps_explicit_legacy_source_item() {
    let store = SqliteStore::in_memory().expect("failed to create sqlite store");
    let mut canonical = Document::new("codex-session", "");
    canonical.set_url("remem://raw-session/v2/codex/local/repo/session");
    refine_core::knowledge::DocumentRepository::save(&store, &canonical)
        .await
        .expect("seed canonical document");
    let mut old = Item::new_observation("old", "old summary");
    old.set_document_id(canonical.id().clone());
    ItemRepository::save(&store, &old)
        .await
        .expect("seed old canonical item");

    let mut legacy = Document::new("codex-session", "legacy transcript");
    legacy.set_url("/tmp/rollout-session.jsonl");
    refine_core::knowledge::DocumentRepository::save(&store, &legacy)
        .await
        .expect("seed legacy document");
    let mut carried = Item::new_observation("carried", "carried summary");
    carried.set_document_id(legacy.id().clone());
    ItemRepository::save(&store, &carried)
        .await
        .expect("seed carried legacy item");

    let replacement = Document::restore(RestoreDocumentParams {
        id: canonical.id().clone(),
        title: Some("replacement".to_string()),
        raw_content: String::new(),
        source: canonical.source().to_string(),
        url: canonical.url().to_string(),
        source_version: Some("sha256:new".to_string()),
        captured_at: canonical.captured_at(),
        created_at: canonical.created_at(),
        updated_at: canonical.updated_at(),
    });
    let mut generated = Item::new_observation("generated", "generated summary");
    generated.set_document_id(canonical.id().clone());
    refine_core::knowledge::DocumentRepository::save_with_replaced_items_and_delete_documents(
        &store,
        &replacement,
        std::slice::from_ref(&generated),
        std::slice::from_ref(legacy.id()),
        std::slice::from_ref(legacy.id()),
    )
    .await
    .expect("replace canonical facets and carry explicit legacy facets");

    let item_ids = ItemRepository::find_by_document_id(&store, canonical.id())
        .await
        .unwrap()
        .into_iter()
        .map(|item| item.id().to_string())
        .collect::<HashSet<_>>();
    assert_eq!(
        item_ids,
        HashSet::from([generated.id().to_string(), carried.id().to_string()])
    );
    assert!(!ItemRepository::exists(&store, old.id()).await.unwrap());
    assert!(
        refine_core::knowledge::DocumentRepository::find_by_id(&store, legacy.id())
            .await
            .unwrap()
            .is_none()
    );
}

fn build_conversation(id: &str, status: ConversationStatus) -> ConversationRecord {
    let now = now_iso();
    ConversationRecord {
        id: id.to_string(),
        user_id: "test-user".to_string(),
        source: "test".to_string(),
        url: format!("https://example.com/{}", id),
        title: Some("test conversation".to_string()),
        raw_content: "Human: hello\nAssistant: world".to_string(),
        metadata: json!({}),
        captured_at: now.clone(),
        created_at: now,
        status,
        idempotency_key: format!("idempotency-{}", id),
        item_ids: Vec::new(),
        last_error: None,
    }
}
