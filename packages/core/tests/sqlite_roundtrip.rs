use chrono::{Duration, TimeZone, Utc};
use refine_core::infra::SqliteStore;
use refine_core::knowledge::{Item, ItemId, ItemRepository, ItemType, Source, Tag};
use std::collections::HashSet;

#[tokio::test]
async fn roundtrip_preserves_timestamps_and_content() {
    let store = SqliteStore::in_memory().expect("failed to create sqlite store");

    let created_at = Utc
        .with_ymd_and_hms(2026, 2, 6, 10, 0, 0)
        .single()
        .expect("invalid created_at");
    let updated_at = created_at + Duration::minutes(5);

    let item = Item::restore(
        ItemId::from_str("item-1"),
        ItemType::Knowledge,
        "Rust Time Semantics".to_string(),
        "Verify persisted timestamps are stable".to_string(),
        "Detailed content should survive roundtrip.".to_string(),
        vec![Tag::new("rust").expect("invalid tag")],
        Some(Source::new("claude").with_url("https://claude.ai/chat/abc")),
        created_at,
        updated_at,
    )
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
