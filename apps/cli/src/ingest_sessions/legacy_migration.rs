use crate::remem_sessions::RememSession;
#[cfg(test)]
use crate::remem_sessions::RememSessionSummary;
use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use refine_core::knowledge::{Document, DocumentId};
#[cfg(test)]
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;

const LOCAL_SOURCE_ROOT: &str = "local";
const LOCAL_SOURCE_ROOT_HEX: &str = "6c6f63616c";
const LEGACY_SOURCES: [&str; 2] = ["claude-code-session", "codex-session"];

pub(super) fn legacy_document_might_match_summary(
    document: &Document,
    summary: &crate::remem_sessions::RememSessionSummary,
) -> bool {
    summary.source_root == LOCAL_SOURCE_ROOT
        && LEGACY_SOURCES.contains(&document.source())
        && !document.url().starts_with("remem://raw-session/v2/")
        && (url_matches_session_id(document.url(), &summary.session_id)
            || document.captured_at().timestamp() == summary.first_epoch)
}

#[cfg(test)]
pub(super) fn legacy_document_index(documents: &[Document]) -> HashMap<String, Vec<&Document>> {
    let mut index = HashMap::new();
    for document in documents
        .iter()
        .filter(|document| LEGACY_SOURCES.contains(&document.source()))
    {
        for session_id in session_id_candidates(Path::new(document.url())) {
            index
                .entry(session_id)
                .or_insert_with(Vec::new)
                .push(document);
        }
    }
    index
}

#[cfg(test)]
pub(super) fn matching_legacy_document_for_summary<'doc>(
    index: &HashMap<String, Vec<&'doc Document>>,
    summary: &RememSessionSummary,
) -> Result<Option<&'doc Document>> {
    if summary.source_root != LOCAL_SOURCE_ROOT {
        return Ok(None);
    }
    match index.get(&summary.session_id).map(Vec::as_slice) {
        None | Some([]) => Ok(None),
        Some([document]) => Ok(Some(*document)),
        Some(matches) => bail!(
            "ambiguous legacy filename identity for remem tuple ({:?}, {:?}, {:?}): {} candidates",
            summary.source_root,
            summary.project,
            summary.session_id,
            matches.len()
        ),
    }
}

pub(super) fn matching_legacy_document_ids(
    documents: &[Document],
    remem_session: &RememSession,
    raw_content: &str,
) -> Result<Vec<DocumentId>> {
    if remem_session.source_root != LOCAL_SOURCE_ROOT {
        return Ok(Vec::new());
    }

    let legacy: Vec<&Document> = documents
        .iter()
        .filter(|document| {
            LEGACY_SOURCES.contains(&document.source())
                && !document.url().starts_with("remem://raw-session/v2/")
        })
        .collect();

    let filename_and_content: Vec<&Document> = legacy
        .iter()
        .copied()
        .filter(|document| {
            url_matches_session_id(document.url(), &remem_session.session_id)
                && content_matches(document.raw_content(), raw_content)
        })
        .collect();
    if !filename_and_content.is_empty() {
        return unique_match(filename_and_content, remem_session);
    }

    let epoch_and_content: Vec<&Document> = legacy
        .iter()
        .copied()
        .filter(|document| {
            document.captured_at().timestamp() == remem_session.first_epoch
                && content_matches(document.raw_content(), raw_content)
        })
        .collect();
    if !epoch_and_content.is_empty() {
        return unique_match(epoch_and_content, remem_session);
    }

    Ok(Vec::new())
}

pub(super) fn legacy_document_covering_nonunique_summary(
    documents: &[Document],
    remem_session: &RememSession,
    raw_content: &str,
) -> Option<DocumentId> {
    (remem_session.source_root == LOCAL_SOURCE_ROOT)
        .then(|| {
            documents.iter().find(|document| {
                LEGACY_SOURCES.contains(&document.source())
                    && url_matches_session_id(document.url(), &remem_session.session_id)
                    && document.raw_content() == raw_content
            })
        })
        .flatten()
        .map(|document| document.id().clone())
}

fn unique_match(matches: Vec<&Document>, remem_session: &RememSession) -> Result<Vec<DocumentId>> {
    match matches.as_slice() {
        [] => Ok(Vec::new()),
        [document] => Ok(vec![document.id().clone()]),
        _ => bail!(
            "ambiguous legacy identity for remem tuple ({:?}, {:?}, {:?}): {} candidates",
            remem_session.source_root,
            remem_session.project,
            remem_session.session_id,
            matches.len()
        ),
    }
}

fn url_matches_session_id(url: &str, session_id: &str) -> bool {
    let Some(stem) = Path::new(url).file_stem().and_then(|stem| stem.to_str()) else {
        return false;
    };
    stem == session_id || stem.ends_with(&format!("-{session_id}"))
}

fn content_matches(legacy: &str, current: &str) -> bool {
    legacy == current || current.starts_with(legacy)
}

pub(super) fn matching_remem_document(
    documents: &[Document],
    legacy_path: &Path,
    captured_at: DateTime<Utc>,
    raw_content: &str,
) -> Result<Option<Document>> {
    let remem_documents: Vec<&Document> = documents
        .iter()
        .filter(|document| {
            document.source() == "remem-raw-session" && is_local_remem_url(document.url())
        })
        .collect();
    let session_ids = session_id_candidates(legacy_path);

    let url_and_content: Vec<&Document> = remem_documents
        .iter()
        .copied()
        .filter(|document| {
            session_ids
                .iter()
                .any(|session_id| document.url().ends_with(&hex_component(session_id)))
                && canonical_contains_local(document.raw_content(), raw_content)
        })
        .collect();
    if !url_and_content.is_empty() {
        return unique_remem_match(url_and_content, legacy_path);
    }

    let epoch_and_content: Vec<&Document> = remem_documents
        .iter()
        .copied()
        .filter(|document| {
            document.captured_at().timestamp() == captured_at.timestamp()
                && canonical_contains_local(document.raw_content(), raw_content)
        })
        .collect();
    unique_remem_match(epoch_and_content, legacy_path)
}

pub(super) fn claim_remem_document_once(
    claimed: &mut HashSet<DocumentId>,
    document: &Document,
    legacy_path: &Path,
) -> Result<()> {
    if claimed.insert(document.id().clone()) {
        return Ok(());
    }
    bail!(
        "remem document {} matched more than one legacy path; second claimant {:?}",
        document.id(),
        legacy_path
    )
}

fn unique_remem_match(matches: Vec<&Document>, legacy_path: &Path) -> Result<Option<Document>> {
    match matches.as_slice() {
        [] => Ok(None),
        [document] => Ok(Some((*document).clone())),
        _ => bail!(
            "ambiguous remem identity for legacy session {:?}: {} candidates",
            legacy_path,
            matches.len()
        ),
    }
}

fn session_id_candidates(path: &Path) -> Vec<String> {
    let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return Vec::new();
    };
    let mut candidates = vec![stem.to_string()];
    if stem.len() >= 36 {
        let suffix = &stem[stem.len() - 36..];
        if suffix != stem
            && suffix.as_bytes().get(8) == Some(&b'-')
            && suffix.as_bytes().get(13) == Some(&b'-')
            && suffix.as_bytes().get(18) == Some(&b'-')
            && suffix.as_bytes().get(23) == Some(&b'-')
        {
            candidates.push(suffix.to_string());
        }
    }
    candidates
}

fn hex_component(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn canonical_contains_local(canonical: &str, local: &str) -> bool {
    canonical == local || canonical.starts_with(local)
}

fn is_local_remem_url(url: &str) -> bool {
    url.strip_prefix("remem-raw://v1/")
        .and_then(|rest| rest.split('/').next())
        == Some(LOCAL_SOURCE_ROOT_HEX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use refine_core::knowledge::RestoreDocumentParams;
    use refine_core::session::{Session, SessionMeta, SessionSource};
    use std::path::PathBuf;

    fn document(source: &str, url: &str, content: &str, epoch: i64) -> Document {
        let original = Document::new(source, content);
        let captured_at = Utc.timestamp_opt(epoch, 0).unwrap();
        Document::restore(RestoreDocumentParams {
            id: original.id().clone(),
            title: None,
            raw_content: content.to_string(),
            source: source.to_string(),
            url: url.to_string(),
            source_version: None,
            captured_at,
            created_at: original.created_at(),
            updated_at: original.updated_at(),
        })
    }

    fn remem(source_root: &str, session_id: &str) -> RememSession {
        RememSession {
            session_ref: format!("remem://raw-session/v2/test/{source_root}/repo/{session_id}"),
            source_root: source_root.to_string(),
            project: "/repo".to_string(),
            session_id: session_id.to_string(),
            first_epoch: 10,
            session: Session {
                source: SessionSource::RememRaw,
                file_path: PathBuf::new(),
                messages: Vec::new(),
                meta: SessionMeta::default(),
            },
        }
    }

    fn summary(source_root: &str, session_id: &str) -> RememSessionSummary {
        RememSessionSummary {
            session_ref: format!("remem://raw-session/v2/test/{source_root}/repo/{session_id}"),
            host: "codex-cli".to_string(),
            session_mode: "interactive".to_string(),
            source_root: source_root.to_string(),
            project: "/repo".to_string(),
            session_id: session_id.to_string(),
            first_epoch: 10,
            last_epoch: 20,
            message_count: 2,
            user_message_count: 1,
            assistant_message_count: 1,
            content_hash: format!("sha256:{}", "a".repeat(64)),
            user_message_samples: Vec::new(),
            legacy_identity_is_unique: true,
        }
    }

    #[test]
    fn summary_lookup_uses_unique_legacy_filename_without_content_loading() {
        let session_id = "12345678-1234-1234-1234-123456789abc";
        let legacy = document(
            "codex-session",
            &format!("/tmp/rollout-prefix-{session_id}.jsonl"),
            "old",
            10,
        );
        let documents = vec![legacy.clone()];
        let index = legacy_document_index(&documents);

        let matched =
            matching_legacy_document_for_summary(&index, &summary(LOCAL_SOURCE_ROOT, session_id))
                .unwrap()
                .unwrap();
        assert_eq!(matched.id(), legacy.id());
        assert!(
            matching_legacy_document_for_summary(&index, &summary("remote", session_id))
                .unwrap()
                .is_none()
        );

        let exact = document(
            "claude-code-session",
            &format!("/tmp/{session_id}.jsonl"),
            "old",
            10,
        );
        let exact_documents = vec![exact];
        let exact_index = legacy_document_index(&exact_documents);
        assert_eq!(exact_index.get(session_id).unwrap().len(), 1);
    }

    #[test]
    fn nonunique_summary_uses_legacy_content_only_as_read_only_coverage() {
        let legacy = document("codex-session", "/tmp/session-1.jsonl", "same", 10);
        let documents = vec![legacy];

        assert!(legacy_document_covering_nonunique_summary(
            &documents,
            &remem("local", "session-1"),
            "same",
        )
        .is_some());
        assert!(legacy_document_covering_nonunique_summary(
            &documents,
            &remem("local", "session-1"),
            "same plus append",
        )
        .is_none());
        assert!(legacy_document_covering_nonunique_summary(
            &documents,
            &remem("remote", "session-1"),
            "same",
        )
        .is_none());
    }

    #[test]
    fn matches_local_filename_or_epoch_content_but_not_remote() {
        let filename = document("claude-code-session", "/tmp/session-1.jsonl", "old", 5);
        let epoch = document("codex-session", "/tmp/other.jsonl", "prefix", 10);
        let documents = vec![filename.clone(), epoch.clone()];

        let matched =
            matching_legacy_document_ids(&documents, &remem("local", "session-1"), "old appended")
                .unwrap();
        assert_eq!(matched, vec![filename.id().clone()]);

        let matched = matching_legacy_document_ids(
            std::slice::from_ref(&epoch),
            &remem("local", "canonical-id"),
            "prefix plus append",
        )
        .unwrap();
        assert_eq!(matched, vec![epoch.id().clone()]);

        assert!(
            matching_legacy_document_ids(&[filename], &remem("remote", "session-1"), "old",)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn rejects_ambiguous_content_fallback() {
        let documents = vec![
            document("claude-code-session", "/tmp/a.jsonl", "same", 10),
            document("codex-session", "/tmp/b.jsonl", "same", 10),
        ];
        assert!(
            matching_legacy_document_ids(&documents, &remem("local", "canonical-id"), "same",)
                .unwrap_err()
                .to_string()
                .contains("ambiguous legacy identity")
        );
    }

    #[test]
    fn rollback_finds_remem_identity_by_uuid_suffix() {
        let raw = "User: same\n";
        let session_id = "12345678-1234-1234-1234-123456789abc";
        let remem_doc = document(
            "remem-raw-session",
            &format!(
                "remem-raw://v1/{}/{}/{}",
                LOCAL_SOURCE_ROOT_HEX,
                hex_component("/repo"),
                hex_component(session_id)
            ),
            raw,
            10,
        );
        let path = Path::new(
            "/tmp/rollout-2026-07-20T00-00-00-12345678-1234-1234-1234-123456789abc.jsonl",
        );
        let matched = matching_remem_document(
            std::slice::from_ref(&remem_doc),
            path,
            Utc.timestamp_opt(10, 0).unwrap(),
            raw,
        )
        .unwrap()
        .unwrap();
        assert_eq!(matched.id(), remem_doc.id());
    }

    #[test]
    fn longer_local_snapshot_cannot_claim_or_overwrite_canonical_remem() {
        let session_id = "12345678-1234-1234-1234-123456789abc";
        let remem_doc = document(
            "remem-raw-session",
            &format!(
                "remem-raw://v1/{}/{}/{}",
                LOCAL_SOURCE_ROOT_HEX,
                hex_component("/repo"),
                hex_component(session_id)
            ),
            "User: canonical prefix\n",
            10,
        );
        let path = Path::new(
            "/tmp/rollout-2026-07-20T00-00-00-12345678-1234-1234-1234-123456789abc.jsonl",
        );
        let matched = matching_remem_document(
            &[remem_doc],
            path,
            Utc.timestamp_opt(10, 0).unwrap(),
            "User: canonical prefix\nAssistant: local-only suffix\n",
        )
        .unwrap();
        assert!(matched.is_none());
    }

    #[test]
    fn rollback_never_reuses_remote_remem_identity() {
        let raw = "User: same\n";
        let session_id = "12345678-1234-1234-1234-123456789abc";
        let remote = document(
            "remem-raw-session",
            &format!(
                "remem-raw://v1/{}/repo/{}",
                hex_component("remote"),
                hex_component(session_id)
            ),
            raw,
            10,
        );
        let path = Path::new(
            "/tmp/rollout-2026-07-20T00-00-00-12345678-1234-1234-1234-123456789abc.jsonl",
        );
        assert!(
            matching_remem_document(&[remote], path, Utc.timestamp_opt(10, 0).unwrap(), raw,)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn filename_without_content_or_epoch_corroboration_is_not_destructive() {
        let legacy = document(
            "claude-code-session",
            "/other/project/session-1.jsonl",
            "unrelated",
            5,
        );
        assert!(
            matching_legacy_document_ids(&[legacy], &remem("local", "session-1"), "different",)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn exact_content_without_identity_corroboration_is_not_destructive() {
        let legacy = document(
            "claude-code-session",
            "/other/project/unrelated.jsonl",
            "same",
            5,
        );
        assert!(
            matching_legacy_document_ids(&[legacy], &remem("local", "session-1"), "same",)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn rollback_rejects_a_second_path_claiming_the_same_remem_document() {
        let remem_doc = document(
            "remem-raw-session",
            "remem-raw://v1/6c6f63616c/repo/session",
            "same",
            10,
        );
        let mut claimed = HashSet::new();
        claim_remem_document_once(&mut claimed, &remem_doc, Path::new("/tmp/one.jsonl")).unwrap();
        assert!(
            claim_remem_document_once(&mut claimed, &remem_doc, Path::new("/tmp/two.jsonl"),)
                .unwrap_err()
                .to_string()
                .contains("more than one legacy path")
        );
    }
}
