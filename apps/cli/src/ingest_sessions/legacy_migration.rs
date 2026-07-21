use crate::remem_sessions::RememSession;
use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use refine_core::knowledge::{Document, DocumentId};
use std::collections::HashSet;
use std::path::Path;

const LOCAL_SOURCE_ROOT: &str = "local";
const LOCAL_SOURCE_ROOT_HEX: &str = "6c6f63616c";
const LEGACY_SOURCES: [&str; 2] = ["claude-code-session", "codex-session"];

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
        .filter(|document| LEGACY_SOURCES.contains(&document.source()))
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
                && contents_overlap(document.raw_content(), raw_content)
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
                && contents_overlap(document.raw_content(), raw_content)
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
        if suffix.as_bytes().get(8) == Some(&b'-')
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

fn contents_overlap(left: &str, right: &str) -> bool {
    left == right || left.starts_with(right) || right.starts_with(left)
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
            captured_at,
            created_at: original.created_at(),
            updated_at: original.updated_at(),
        })
    }

    fn remem(source_root: &str, session_id: &str) -> RememSession {
        RememSession {
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
            &[epoch.clone()],
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
            &[remem_doc.clone()],
            path,
            Utc.timestamp_opt(10, 0).unwrap(),
            raw,
        )
        .unwrap()
        .unwrap();
        assert_eq!(matched.id(), remem_doc.id());
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
