use chrono::{Duration, TimeZone, Utc};
use refine_core::infra::SqliteStore;
use refine_core::knowledge::{
    Document, DocumentId, Item, ItemId, ItemRepository, ItemType, RestoreDocumentParams,
    RestoreParams, Source, Tag,
};
use std::collections::HashSet;

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
    assert_eq!(by_url.captured_at(), second_captured_at);
    assert_eq!(by_url.updated_at(), second_updated_at);
    assert!(by_url.updated_at() >= first_updated_at);
    assert_eq!(by_id.title(), Some("Newer title"));
    assert_eq!(by_id.raw_content(), "newer content");
    assert_eq!(by_id.captured_at(), second_captured_at);
    assert_eq!(by_id.updated_at(), second_updated_at);
}

#[tokio::test]
async fn document_save_preserves_existing_title_when_duplicate_url_has_no_title() {
    let store = SqliteStore::in_memory().expect("failed to create sqlite store");
    let url = "https://claude.ai/chat/duplicate-no-title";

    let mut first = Document::new("claude", "older content");
    first.set_title("Canonical title");
    first.set_url(url);
    refine_core::knowledge::DocumentRepository::save(&store, &first)
        .await
        .expect("failed to save first document");

    let mut second = Document::new("claude", "newer content");
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
    assert_eq!(by_url.captured_at(), second_captured_at);
    assert_eq!(by_url.updated_at(), second_updated_at);
    assert_eq!(by_id.title(), Some("Canonical title"));
    assert_eq!(by_id.raw_content(), "newer content");
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
