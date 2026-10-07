use refine_core::infra::SqliteStore;
use refine_core::knowledge::{
    assign_session_observation_ids, Document, DocumentRepository, Item, ItemRepository,
    SessionProjectionMetadata, Tag,
};

fn document() -> Document {
    let mut document = Document::new("codex-session", "");
    document.set_url("remem://raw-session/v2/synthetic/local/project/session");
    document.set_source_version(Some(&format!("sha256:{}:interactive", "a".repeat(64))));
    document
}

fn items(document: &Document) -> Vec<Item> {
    let mut item = Item::new_observation("Choose SQLite", "Choose SQLite for local persistence");
    item.set_document_id(document.id().clone());
    item.set_tags(vec![
        Tag::new("decision").unwrap(),
        Tag::new("session_mode_interactive").unwrap(),
    ])
    .unwrap();
    let mut items = vec![item];
    assign_session_observation_ids(&mut items, document.url()).unwrap();
    items
}

fn metadata(recipe: &str) -> SessionProjectionMetadata {
    SessionProjectionMetadata {
        recipe_id: recipe.to_string(),
        evidence: serde_json::json!({"status":"unknown"}),
    }
}

#[tokio::test]
async fn projection_context_excludes_direct_edits_and_deletions_without_tag_hints() {
    for full_tag_budget in [false, true] {
        let store = SqliteStore::in_memory().unwrap();
        let document = document();
        let mut original = items(&document);
        if full_tag_budget {
            let mut tags = original[0].tags().to_vec();
            tags.extend((0..18).map(|index| Tag::new(&format!("user-{index}")).unwrap()));
            original[0].set_tags(tags).unwrap();
            assert_eq!(original[0].tags().len(), 20);
        }
        let evidence = serde_json::json!({
            "observations": [{"item_id": original[0].id(), "field": "decisions", "index": 0}],
            "facets": [{"field": "decisions", "index": 0, "status": "references_validated", "message_ids": [42]}]
        });
        let metadata = SessionProjectionMetadata {
            recipe_id: "recipe-a".into(),
            evidence,
        };
        store
            .save_session_projection(&document, &original, &[], &[], &metadata)
            .await
            .unwrap();
        let before = store
            .find_session_projection_context(document.url())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(before.evidence["observations"].as_array().unwrap().len(), 1);
        let mut edited = original[0].clone();
        edited.set_title("Human selected a different approach");
        ItemRepository::save(&store, &edited).await.unwrap();
        let context = store
            .find_session_projection_context(document.url())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(context.document.id(), document.id());
        assert_eq!(context.items[0].title(), edited.title());
        assert!(!context.items[0]
            .tags()
            .iter()
            .any(|tag| tag.as_str() == "curated"));
        assert_eq!(context.items[0].tags().len(), original[0].tags().len());
        assert!(context.evidence["observations"]
            .as_array()
            .unwrap()
            .is_empty());
        assert_eq!(
            context.evidence["facets"][0]["message_ids"],
            serde_json::json!([42])
        );
        ItemRepository::delete(&store, edited.id()).await.unwrap();
        let deleted = store
            .find_session_projection_context(document.url())
            .await
            .unwrap()
            .unwrap();
        assert!(deleted.items.is_empty());
        assert!(deleted.evidence["observations"]
            .as_array()
            .unwrap()
            .is_empty());
    }
}

#[tokio::test]
async fn projection_context_suppresses_evidence_from_a_different_document_version() {
    let store = SqliteStore::in_memory().unwrap();
    let mut document = document();
    store
        .save_session_projection(
            &document,
            &items(&document),
            &[],
            &[],
            &metadata("recipe-a"),
        )
        .await
        .unwrap();
    assert!(!store
        .find_session_projection_context(document.url())
        .await
        .unwrap()
        .unwrap()
        .evidence
        .is_null());
    document.set_source_version(Some(&format!("sha256:{}:interactive", "b".repeat(64))));
    DocumentRepository::save(&store, &document).await.unwrap();
    let context = store
        .find_session_projection_context(document.url())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(context.document.source_version(), document.source_version());
    assert!(context.evidence.is_null());
    assert_eq!(context.items.len(), 1);
    assert!(store
        .find_session_projection_context("remem://raw-session/v2/missing")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn explicit_edits_and_deletions_survive_reprocessing() {
    let store = SqliteStore::in_memory().unwrap();
    let document = document();
    let original = items(&document);
    store
        .save_session_projection(&document, &original, &[], &[], &metadata("recipe-a"))
        .await
        .unwrap();
    let mut edited = original[0].clone();
    edited.set_title("Human correction: SQLite WAL");
    ItemRepository::save(&store, &edited).await.unwrap();

    store
        .save_session_projection(
            &document,
            &items(&document),
            &[],
            &[],
            &metadata("recipe-b"),
        )
        .await
        .unwrap();
    let current = store
        .find_items_by_document_id(document.id())
        .await
        .unwrap();
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].id(), edited.id());
    assert_eq!(current[0].title(), edited.title());
    let history = store
        .find_session_projection_history(document.id(), 20)
        .await
        .unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].recipe_id.as_deref(), Some("recipe-a"));
    assert_eq!(history[0].items[0].title(), edited.title());

    ItemRepository::delete(&store, edited.id()).await.unwrap();
    let mut duplicate_machine_claims = items(&document);
    duplicate_machine_claims.extend(items(&document));
    store
        .save_session_projection(
            &document,
            &duplicate_machine_claims,
            &[],
            &[],
            &metadata("recipe-c"),
        )
        .await
        .unwrap();
    assert!(store
        .find_items_by_document_id(document.id())
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        store.find_session_projection_versions().await.unwrap()[0].recipe_id,
        "recipe-c"
    );
    let archived = store
        .find_session_projection_history(document.id(), 20)
        .await
        .unwrap();
    assert!(archived
        .iter()
        .any(|revision| revision.recipe_id.as_deref() == Some("recipe-b")
            && revision.items.is_empty()));
}

#[tokio::test]
async fn unmatchable_human_edit_is_preserved_and_marked_for_review() {
    let store = SqliteStore::in_memory().unwrap();
    let document = document();
    let mut original = items(&document);
    store
        .save_session_projection(&document, &original, &[], &[], &metadata("recipe-a"))
        .await
        .unwrap();
    original[0].set_summary("User verified constraints");
    ItemRepository::save(&store, &original[0]).await.unwrap();
    store
        .save_session_projection(&document, &[], &[], &[], &metadata("recipe-b"))
        .await
        .unwrap();
    let current = store
        .find_items_by_document_id(document.id())
        .await
        .unwrap();
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].summary(), "User verified constraints");
    assert!(current[0]
        .tags()
        .iter()
        .any(|tag| tag.as_str() == "curation_needs_review"));
}

#[tokio::test]
async fn unmatched_curation_stays_visible_without_double_counting_the_new_machine_claim() {
    use refine_core::knowledge::Source;
    use refine_core::session::{
        aggregate_observations, cluster_observations, cluster_session_observation_windows,
        cluster_session_observations, format_data_quality_stats,
        portrait_session_observation_windows, portrait_session_observations,
    };
    use std::collections::HashMap;

    let store = SqliteStore::in_memory().unwrap();
    let document = document();
    let mut original = items(&document);
    original[0].set_source(Source::new("session-project").with_url("/previous/service"));
    store
        .save_session_projection(&document, &original, &[], &[], &metadata("recipe-a"))
        .await
        .unwrap();
    original[0].set_summary("Human correction: use WAL with SQLite");
    ItemRepository::save(&store, &original[0]).await.unwrap();

    let mut replacement = items(&document);
    replacement[0].set_title("SQLite chosen for local storage");
    replacement[0].set_source(Source::new("session-project").with_url("/current/service"));
    assign_session_observation_ids(&mut replacement, document.url()).unwrap();
    store
        .save_session_projection(&document, &replacement, &[], &[], &metadata("recipe-b"))
        .await
        .unwrap();
    let visible = store
        .find_items_by_document_id(document.id())
        .await
        .unwrap();
    assert_eq!(
        visible.len(),
        2,
        "human correction remains available to the UI"
    );
    assert!(visible
        .iter()
        .any(|item| item.summary() == "Human correction: use WAL with SQLite"));
    let pending = visible
        .iter()
        .find(|item| {
            item.tags()
                .iter()
                .any(|tag| tag.as_str() == "curation_needs_review")
        })
        .unwrap()
        .clone();

    let sources = HashMap::from([(document.id().to_string(), document.source().to_string())]);
    let generic = cluster_observations(&visible);
    let full = cluster_session_observations(&visible, &sources);
    let portrait = portrait_session_observations(&visible, &sources);
    for cluster in [&generic, &full.cluster] {
        assert_eq!(cluster.global_stats.total_decisions, 1);
        assert_eq!(cluster.global_stats.total_sessions, 1);
        assert_eq!(
            cluster.global_stats.project_ranking,
            vec![("service".to_string(), 1)]
        );
        assert_eq!(cluster.data_quality.curation_excluded_observations, 1);
        assert_eq!(cluster.data_quality.eligible_observations, 1);
        assert!(cluster.data_quality.is_degraded());
        assert!(format_data_quality_stats(&cluster.data_quality).contains("待核对修订排除: 1"));
    }
    assert_eq!(
        full.cohort_items.len(),
        1,
        "raw evidence consumers also exclude pending edits"
    );
    assert_eq!(portrait.eligible_items.len(), 1);
    assert_eq!(portrait.global_stats.total_decisions, 1);
    assert_eq!(portrait.data_quality, full.cluster.data_quality);
    assert_eq!(aggregate_observations(&visible).l2.decision_count, 1);

    let previous = vec![pending];
    let windows = [&replacement[..], &previous[..]];
    let full_windows = cluster_session_observation_windows(&windows, &sources);
    let portrait_windows = portrait_session_observation_windows(&windows, &sources);
    assert_eq!(
        full_windows[0].cluster.global_stats.project_ranking,
        vec![("service".to_string(), 1)],
        "pending prior path must not create a project alias collision"
    );
    assert!(full_windows[1].cohort_items.is_empty());
    assert_eq!(
        full_windows[1]
            .cluster
            .data_quality
            .curation_excluded_observations,
        1
    );
    for (full, portrait) in full_windows.iter().zip(&portrait_windows) {
        assert_eq!(full.cluster.data_quality, portrait.data_quality);
        assert_eq!(
            full.cluster.global_stats.project_ranking,
            portrait.global_stats.project_ranking
        );
    }
}

#[tokio::test]
async fn human_corrections_keep_trusted_mode_when_the_source_is_reclassified() {
    let store = SqliteStore::in_memory().unwrap();
    let mut document = document();
    let original = items(&document);
    store
        .save_session_projection(&document, &original, &[], &[], &metadata("recipe-a"))
        .await
        .unwrap();
    let mut edited = original[0].clone();
    edited.set_summary("Human correction");
    ItemRepository::save(&store, &edited).await.unwrap();
    document.set_source_version(Some(&format!("sha256:{}:unattended", "a".repeat(64))));
    store
        .save_session_projection(
            &document,
            &items(&document),
            &[],
            &[],
            &metadata("recipe-b"),
        )
        .await
        .unwrap();
    let current = store
        .find_items_by_document_id(document.id())
        .await
        .unwrap();
    assert_eq!(current[0].summary(), "Human correction");
    assert!(current[0]
        .tags()
        .iter()
        .any(|tag| tag.as_str() == "session_mode_unattended"));
    assert!(!current[0]
        .tags()
        .iter()
        .any(|tag| tag.as_str() == "session_mode_interactive"));
}

#[tokio::test]
async fn recipe_failure_rolls_back_items_document_and_revision_history() {
    let store = SqliteStore::in_memory().unwrap();
    let mut document = document();
    let original = items(&document);
    let old_version = document.source_version().unwrap().to_string();
    store
        .save_session_projection(&document, &original, &[], &[], &metadata("recipe-a"))
        .await
        .unwrap();
    document.set_source_version(Some(&format!("sha256:{}:interactive", "b".repeat(64))));
    assert!(store
        .save_session_projection(&document, &[], &[], &[], &metadata(""))
        .await
        .is_err());
    assert_eq!(
        store
            .find_items_by_document_id(document.id())
            .await
            .unwrap()[0]
            .title(),
        original[0].title()
    );
    assert_eq!(
        DocumentRepository::find_by_id(&store, document.id())
            .await
            .unwrap()
            .unwrap()
            .source_version(),
        Some(old_version.as_str())
    );
    assert_eq!(
        store.find_session_projection_versions().await.unwrap()[0].recipe_id,
        "recipe-a"
    );
    assert!(store
        .find_session_projection_history(document.id(), 20)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn pre_recipe_items_keep_their_old_ids_in_history() {
    let store = SqliteStore::in_memory().unwrap();
    let document = document();
    DocumentRepository::save(&store, &document).await.unwrap();
    let mut legacy = Item::new_observation("Previously corrected manually", "Legacy evidence");
    legacy.set_document_id(document.id().clone());
    ItemRepository::save(&store, &legacy).await.unwrap();
    store
        .save_session_projection(
            &document,
            &items(&document),
            &[],
            &[],
            &metadata("recipe-b"),
        )
        .await
        .unwrap();
    let history = store
        .find_session_projection_history(document.id(), 20)
        .await
        .unwrap();
    assert_eq!(history.len(), 1);
    assert!(history[0].recipe_id.is_none());
    assert_eq!(history[0].items[0].id(), legacy.id());
    assert_eq!(history[0].items[0].title(), legacy.title());
}
