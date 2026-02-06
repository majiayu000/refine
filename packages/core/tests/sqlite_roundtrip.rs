use chrono::{Duration, TimeZone, Utc};
use refine_core::infra::SqliteStore;
use refine_core::knowledge::{Item, ItemId, ItemRepository, ItemType, Source, Tag};

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
