use super::*;
use refine_core::infra::SqliteStore;
use refine_core::knowledge::{DocumentId, Item, ItemRepository, Tag};
use refine_core::session::SessionMode;
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

static REMEM_BIN_LOCK: Mutex<()> = Mutex::new(());

struct RememBinGuard {
    previous: Option<OsString>,
    _lock: MutexGuard<'static, ()>,
}

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

impl Drop for RememBinGuard {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(previous) => std::env::set_var("REFINE_REMEM_BIN", previous),
            None => std::env::remove_var("REFINE_REMEM_BIN"),
        }
    }
}

fn install_remem_messages(temp: &tempfile::TempDir) -> RememBinGuard {
    let binary = temp.path().join("remem-messages");
    std::fs::write(
        &binary,
        concat!(
            "#!/bin/sh\n",
            "printf '%s\\n' '",
            r#"{"source_type":"raw_archive","host":"codex-cli","source_root":"local","project":"/repo","session_id":"s1","order":"created_at_epoch_asc_id_asc","limit":2000,"count":2,"has_more":false,"next_cursor":null,"content_hash":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","messages":[{"id":1,"role":"user","content":"question","source":"codex","branch":null,"cwd":null,"created_at_epoch":10},{"id":2,"role":"assistant","content":"answer","source":"codex","branch":null,"cwd":null,"created_at_epoch":20}]}"#,
            "'\n",
        ),
    )
    .expect("write fake remem binary");
    let mut permissions = std::fs::metadata(&binary).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&binary, permissions).expect("make fake remem binary executable");
    RememBinGuard::install(&binary)
}

fn install_forbidden_remem(temp: &tempfile::TempDir) -> (RememBinGuard, PathBuf) {
    let binary = temp.path().join("remem-forbidden");
    std::fs::write(
        &binary,
        "#!/bin/sh\nprintf called > \"$0.called\"\nexit 99\n",
    )
    .expect("write forbidden remem binary");
    let mut permissions = std::fs::metadata(&binary).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&binary, permissions).expect("make forbidden remem executable");
    let marker = PathBuf::from(format!("{}.called", binary.display()));
    (RememBinGuard::install(&binary), marker)
}

async fn document_ids(doc_store: &Arc<dyn DocumentRepository>) -> HashSet<String> {
    doc_store
        .find_recent(0, doc_store.count().await.unwrap())
        .await
        .unwrap()
        .into_iter()
        .map(|document| document.id().to_string())
        .collect()
}

async fn item_ids(item_store: &Arc<dyn ItemRepository>) -> HashSet<String> {
    item_store
        .find_all()
        .await
        .unwrap()
        .into_iter()
        .map(|item| item.id().to_string())
        .collect()
}

async fn item_payloads(
    item_store: &Arc<dyn ItemRepository>,
) -> HashMap<String, (String, String, String, Vec<String>)> {
    item_store
        .find_all()
        .await
        .unwrap()
        .into_iter()
        .map(|item| {
            (
                item.id().to_string(),
                (
                    item.title().to_string(),
                    item.summary().to_string(),
                    item.content().to_string(),
                    item.tags()
                        .iter()
                        .map(|tag| tag.as_str().to_string())
                        .collect(),
                ),
            )
        })
        .collect()
}

fn with_session_mode(
    mut payloads: HashMap<String, (String, String, String, Vec<String>)>,
    mode: &str,
) -> HashMap<String, (String, String, String, Vec<String>)> {
    for (_, _, _, tags) in payloads.values_mut() {
        tags.retain(|tag| !tag.starts_with("session_mode_"));
        tags.push(mode.to_string());
    }
    payloads
}

async fn item_document_ids(
    item_store: &Arc<dyn ItemRepository>,
) -> HashMap<String, Option<String>> {
    item_store
        .find_all()
        .await
        .unwrap()
        .into_iter()
        .map(|item| {
            (
                item.id().to_string(),
                item.document_id().map(ToString::to_string),
            )
        })
        .collect()
}

fn tagged_observation(document: &Document, title: &str, content: &str, tag: &str) -> Item {
    let mut item = Item::new_observation(title, &format!("{title} summary"));
    item.set_content(content);
    item.set_tags(vec![Tag::new(tag).unwrap()]).unwrap();
    item.set_document_id(document.id().clone());
    item
}

fn test_summary(
    host: &str,
    session_ref: &str,
    legacy_identity_is_unique: bool,
) -> RememSessionSummary {
    RememSessionSummary {
        session_ref: session_ref.to_string(),
        host: host.to_string(),
        session_mode: if host == "codex-cli" {
            "interactive".to_string()
        } else {
            "unknown".to_string()
        },
        source_root: "local".to_string(),
        project: "/repo".to_string(),
        session_id: "s1".to_string(),
        first_epoch: 10,
        last_epoch: 20,
        message_count: 2,
        user_message_count: 1,
        assistant_message_count: 1,
        content_hash: format!("sha256:{}", "a".repeat(64)),
        user_message_samples: Vec::new(),
        legacy_identity_is_unique,
    }
}

fn remem_options() -> IngestOptions {
    IngestOptions {
        source: None,
        provider: IngestProvider::Remem,
        limit: None,
        latest: None,
        dry_run: false,
        retry_quarantined: false,
        backfill_session_metadata: false,
    }
}

async fn handle_test_summaries(
    temp: &tempfile::TempDir,
    summaries: Vec<RememSessionSummary>,
    doc_store: Arc<dyn DocumentRepository>,
) -> Result<()> {
    let quarantine = QuarantineStore::load_from(temp.path().join("quarantine.jsonl"))?;
    handle_remem_ingest_sessions_with_loader(
        remem_options(),
        &temp.path().join("refine.db"),
        summaries,
        Some(quarantine),
        load_remem_session,
        doc_store,
        None,
    )
    .await
}

#[tokio::test]
async fn two_hosts_cannot_reuse_the_same_hostless_v1_document() {
    let store = Arc::new(SqliteStore::in_memory().expect("in-memory sqlite store"));
    let doc_store: Arc<dyn DocumentRepository> = store.clone();
    let codex = test_summary(
        "codex-cli",
        "remem://raw-session/v2/636f6465782d636c69/6c6f63616c/2f7265706f/7331",
        false,
    );
    let claude = test_summary(
        "claude-code",
        "remem://raw-session/v2/636c617564652d636f6465/6c6f63616c/2f7265706f/7331",
        false,
    );
    let mut v1 = Document::new("remem-raw-session", "same transcript");
    v1.set_url(&codex.legacy_document_url());
    v1.set_source_version(Some(&codex.projection_version()));
    doc_store
        .save(&v1)
        .await
        .expect("seed hostless v1 document");
    let temp = tempfile::tempdir().expect("temporary lock directory");

    let error = handle_test_summaries(&temp, vec![codex, claude], doc_store.clone())
        .await
        .expect_err("two hosts must not claim one hostless document in summary order");

    assert!(error
        .to_string()
        .contains("ambiguous hostless legacy Remem identity"));
    let preserved = doc_store
        .find_by_url(v1.url())
        .await
        .unwrap()
        .expect("ambiguous v1 document must remain untouched");
    assert_eq!(preserved.id(), v1.id());
    assert_eq!(preserved.raw_content(), "same transcript");
}

#[tokio::test]
async fn v1_reference_update_and_matching_local_cleanup_share_one_transaction() {
    let store = Arc::new(SqliteStore::in_memory().expect("in-memory sqlite store"));
    let doc_store: Arc<dyn DocumentRepository> = store.clone();
    let item_store: Arc<dyn ItemRepository> = store;
    let mut v1 = Document::new("remem-raw-session", "same transcript");
    v1.set_url("remem-raw://v1/6c6f63616c/2f7265706f/7331");
    doc_store.save(&v1).await.expect("seed v1 Remem document");
    let v1_facet = tagged_observation(&v1, "canonical", "canonical content", "canonical-tag");
    item_store.save(&v1_facet).await.expect("seed v1 facet");

    let mut local_one = Document::new("codex-session", "same transcript");
    local_one.set_url("/tmp/rollout-s1.jsonl");
    doc_store
        .save(&local_one)
        .await
        .expect("seed first local legacy document");
    let local_one_first = tagged_observation(
        &local_one,
        "local-one-first",
        "first local content",
        "first-tag",
    );
    let local_one_second = tagged_observation(
        &local_one,
        "local-one-second",
        "second local content",
        "second-tag",
    );
    item_store
        .save(&local_one_first)
        .await
        .expect("seed first local facet");
    item_store
        .save(&local_one_second)
        .await
        .expect("seed second local facet");

    let mut local_two = Document::new("claude-code-session", "same transcript");
    local_two.set_url("/tmp/claude-s1.jsonl");
    doc_store
        .save(&local_two)
        .await
        .expect("seed second local legacy document");
    let local_two_facet =
        tagged_observation(&local_two, "local-two", "third local content", "third-tag");
    item_store
        .save(&local_two_facet)
        .await
        .expect("seed third local facet");

    let before_item_ids = item_ids(&item_store).await;
    let before_item_payloads = item_payloads(&item_store).await;

    let referenced = referenced_session_document(
        &v1,
        SessionSource::Codex,
        "remem://raw-session/v2/636f6465782d636c69/6c6f63616c/2f7265706f/7331",
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    );
    save_referenced_session_and_delete_legacy(
        &doc_store,
        &v1,
        &referenced,
        &[local_one.id().clone(), local_two.id().clone()],
        SessionMode::Interactive,
    )
    .await
    .expect("reference migration and proven legacy cleanup");

    let saved = doc_store
        .find_by_url(referenced.url())
        .await
        .unwrap()
        .expect("referenced document");
    assert_eq!(referenced.id(), v1.id());
    assert_eq!(saved.id(), v1.id());
    assert!(saved.raw_content().is_empty());
    let canonical_facets = item_store.find_by_document_id(saved.id()).await.unwrap();
    assert_eq!(canonical_facets.len(), 4);
    assert!(doc_store
        .find_by_id(local_one.id())
        .await
        .unwrap()
        .is_none());
    assert!(doc_store
        .find_by_id(local_two.id())
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        document_ids(&doc_store).await,
        HashSet::from([v1.id().to_string()])
    );
    assert_eq!(item_ids(&item_store).await, before_item_ids);
    assert_eq!(
        item_payloads(&item_store).await,
        with_session_mode(before_item_payloads, "session_mode_interactive")
    );
    assert!(canonical_facets.iter().all(|item| {
        item.tags()
            .iter()
            .filter(|tag| tag.as_str().starts_with("session_mode_"))
            .map(|tag| tag.as_str())
            .eq(["session_mode_interactive"])
    }));
    assert!(item_document_ids(&item_store)
        .await
        .values()
        .all(|document_id| document_id.as_deref() == Some(v1.id().as_str())));
}

#[tokio::test]
async fn convergence_reads_items_inside_transaction_after_late_injection() {
    let store = Arc::new(SqliteStore::in_memory().expect("in-memory sqlite store"));
    let doc_store: Arc<dyn DocumentRepository> = store.clone();
    let item_store: Arc<dyn ItemRepository> = store;
    let mut legacy = Document::new("codex-session", "same transcript");
    legacy.set_url("/tmp/rollout-s1.jsonl");
    doc_store.save(&legacy).await.expect("seed legacy document");
    let early = tagged_observation(&legacy, "early", "early content", "early-tag");
    item_store.save(&early).await.expect("seed early item");

    let stale_snapshot = doc_store
        .find_items_by_document_id(legacy.id())
        .await
        .expect("take the former caller-side snapshot");
    assert_eq!(stale_snapshot.len(), 1);
    let late = tagged_observation(&legacy, "late", "late content", "late-tag");
    item_store
        .save(&late)
        .await
        .expect("inject item after the former snapshot boundary");

    let referenced = referenced_session_document(
        &legacy,
        SessionSource::Codex,
        "remem://raw-session/v2/636f6465782d636c69/6c6f63616c/2f7265706f/7331",
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    );
    save_referenced_session_and_delete_legacy(
        &doc_store,
        &legacy,
        &referenced,
        &[],
        SessionMode::Interactive,
    )
    .await
    .expect("transaction must load both source items itself");

    let saved_ids = item_store
        .find_by_document_id(referenced.id())
        .await
        .unwrap()
        .into_iter()
        .map(|item| item.id().to_string())
        .collect::<HashSet<_>>();
    assert_eq!(
        saved_ids,
        HashSet::from([early.id().to_string(), late.id().to_string()])
    );
}

#[tokio::test]
async fn unrelated_legacy_document_does_not_pull_unchanged_v2_body() {
    let store = Arc::new(SqliteStore::in_memory().expect("in-memory sqlite store"));
    let doc_store: Arc<dyn DocumentRepository> = store.clone();
    let summary = test_summary(
        "codex-cli",
        "remem://raw-session/v2/636f6465782d636c69/6c6f63616c/2f7265706f/7331",
        true,
    );
    let mut stable = Document::new("codex-session", "");
    stable.set_url(&summary.stable_document_url());
    stable.set_source_version(Some(&summary.projection_version()));
    doc_store.save(&stable).await.expect("seed stable document");
    let mut unrelated = Document::new("claude-code-session", "unrelated transcript");
    unrelated.set_url("/tmp/unrelated-session.jsonl");
    doc_store
        .save(&unrelated)
        .await
        .expect("seed unrelated legacy document");

    let temp = tempfile::tempdir().expect("temporary remem and lock directory");
    let (_remem_bin, marker) = install_forbidden_remem(&temp);
    handle_test_summaries(&temp, vec![summary], doc_store)
        .await
        .expect("unrelated legacy document must keep the unchanged-summary fast path");

    assert!(!marker.exists(), "full Remem body command was invoked");
}

#[tokio::test]
async fn unchanged_stable_v2_converges_coexisting_v1_and_local_copies() {
    let store = Arc::new(SqliteStore::in_memory().expect("in-memory sqlite store"));
    let doc_store: Arc<dyn DocumentRepository> = store.clone();
    let item_store: Arc<dyn ItemRepository> = store;
    let summary = test_summary(
        "codex-cli",
        "remem://raw-session/v2/636f6465782d636c69/6c6f63616c/2f7265706f/7331",
        true,
    );
    let raw_content = "User: question\nAssistant: answer\n";

    let mut stable = Document::new("codex-session", "");
    stable.set_url(&summary.stable_document_url());
    stable.set_source_version(Some(&summary.projection_version()));
    doc_store
        .save(&stable)
        .await
        .expect("seed stable v2 document");
    let mut stable_facet = Item::new_observation("stable", "stable");
    stable_facet.set_document_id(stable.id().clone());
    item_store
        .save(&stable_facet)
        .await
        .expect("seed stable facet");

    let mut v1 = Document::new("remem-raw-session", raw_content);
    v1.set_url(&summary.legacy_document_url());
    doc_store
        .save(&v1)
        .await
        .expect("seed hostless v1 document");
    let mut v1_facet = Item::new_observation("v1", "v1");
    v1_facet.set_document_id(v1.id().clone());
    item_store.save(&v1_facet).await.expect("seed v1 facet");

    let mut local = Document::new("codex-session", raw_content);
    local.set_url("/tmp/rollout-s1.jsonl");
    doc_store.save(&local).await.expect("seed local document");
    let mut local_facet = Item::new_observation("local", "local");
    local_facet.set_document_id(local.id().clone());
    item_store
        .save(&local_facet)
        .await
        .expect("seed local facet");
    let before_item_ids = item_ids(&item_store).await;
    let before_item_payloads = item_payloads(&item_store).await;
    let expected_version = summary.projection_version();

    let temp = tempfile::tempdir().expect("temporary remem and lock directory");
    let _remem_bin = install_remem_messages(&temp);
    handle_test_summaries(&temp, vec![summary], doc_store.clone())
        .await
        .expect("unchanged stable v2 must converge legacy copies without an LLM");

    assert_eq!(
        document_ids(&doc_store).await,
        HashSet::from([stable.id().to_string()])
    );
    assert_eq!(item_ids(&item_store).await, before_item_ids);
    assert_eq!(
        item_payloads(&item_store).await,
        with_session_mode(before_item_payloads, "session_mode_interactive")
    );
    assert!(item_document_ids(&item_store)
        .await
        .values()
        .all(|document_id| document_id.as_deref() == Some(stable.id().as_str())));
    let saved = doc_store
        .find_by_id(stable.id())
        .await
        .unwrap()
        .expect("stable v2 document remains canonical");
    assert_eq!(saved.source_version(), Some(expected_version.as_str()));
    assert!(saved.raw_content().is_empty());
}

#[tokio::test]
async fn failed_legacy_cleanup_rolls_back_document_and_item_reparenting() {
    let store = Arc::new(SqliteStore::in_memory().expect("in-memory sqlite store"));
    let doc_store: Arc<dyn DocumentRepository> = store.clone();
    let item_store: Arc<dyn ItemRepository> = store;

    let mut v1 = Document::new("remem-raw-session", "same transcript");
    v1.set_url("remem-raw://v1/6c6f63616c/2f7265706f/7331");
    doc_store.save(&v1).await.expect("seed v1 document");
    let v1_facet = tagged_observation(&v1, "v1", "v1 content", "v1-tag");
    item_store.save(&v1_facet).await.expect("seed v1 facet");

    let mut local = Document::new("codex-session", "same transcript");
    local.set_url("/tmp/rollout-s1.jsonl");
    doc_store.save(&local).await.expect("seed local document");
    let local_facet = tagged_observation(&local, "local", "local content", "local-tag");
    item_store
        .save(&local_facet)
        .await
        .expect("seed local facet");

    let before_item_payloads = item_payloads(&item_store).await;
    let before_item_documents = item_document_ids(&item_store).await;
    let referenced = referenced_session_document(
        &v1,
        SessionSource::Codex,
        "remem://raw-session/v2/636f6465782d636c69/6c6f63616c/2f7265706f/7331",
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    );
    let missing = DocumentId::from("missing-obsolete-document");

    save_referenced_session_and_delete_legacy(
        &doc_store,
        &v1,
        &referenced,
        &[local.id().clone(), missing],
        SessionMode::Interactive,
    )
    .await
    .expect_err("missing obsolete document must roll back the whole convergence transaction");

    let restored_v1 = doc_store
        .find_by_id(v1.id())
        .await
        .unwrap()
        .expect("v1 document restored by rollback");
    assert_eq!(restored_v1.url(), v1.url());
    assert_eq!(restored_v1.raw_content(), v1.raw_content());
    assert!(doc_store
        .find_by_url(referenced.url())
        .await
        .unwrap()
        .is_none());
    assert!(doc_store.find_by_id(local.id()).await.unwrap().is_some());
    assert_eq!(item_payloads(&item_store).await, before_item_payloads);
    assert_eq!(item_document_ids(&item_store).await, before_item_documents);
}
