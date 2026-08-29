use super::legacy_migration::legacy_document_might_match_summary;
use crate::remem_sessions::RememSessionSummary;
use anyhow::{bail, Result};
use refine_core::knowledge::{Document, DocumentId, DocumentRepository, RestoreDocumentParams};
use refine_core::session::SessionSource;
use std::sync::Arc;

pub(super) fn referenced_session_document(
    document: &Document,
    source: SessionSource,
    url: &str,
    source_version: &str,
) -> Document {
    Document::restore(RestoreDocumentParams {
        id: document.id().clone(),
        title: document.title().map(ToOwned::to_owned),
        raw_content: String::new(),
        source: source.as_str().to_string(),
        url: url.to_string(),
        source_version: Some(source_version.to_string()),
        captured_at: document.captured_at(),
        created_at: document.created_at(),
        updated_at: document.updated_at(),
    })
}

pub(super) async fn save_referenced_session_and_delete_legacy(
    doc_store: &Arc<dyn DocumentRepository>,
    existing: &Document,
    referenced: &Document,
    legacy_document_ids: &[DocumentId],
) -> refine_core::error::InfraResult<()> {
    let mut obsolete: Vec<DocumentId> = legacy_document_ids
        .iter()
        .filter(|id| *id != referenced.id())
        .cloned()
        .collect();
    if existing.id() != referenced.id() && !obsolete.contains(existing.id()) {
        obsolete.push(existing.id().clone());
    }

    let mut source_document_ids = vec![existing.id().clone()];
    for document_id in &obsolete {
        if !source_document_ids.contains(document_id) {
            source_document_ids.push(document_id.clone());
        }
    }
    doc_store
        .save_with_replaced_items_and_delete_documents(
            referenced,
            &[],
            &source_document_ids,
            &obsolete,
        )
        .await
}

pub(super) fn might_have_legacy_documents(
    summary: &RememSessionSummary,
    legacy_v1: Option<&Document>,
    documents: &[Document],
) -> bool {
    legacy_v1.is_some()
        || documents
            .iter()
            .any(|document| legacy_document_might_match_summary(document, summary))
}

pub(super) fn include_hostless_v1_document(
    legacy_document_ids: &mut Vec<DocumentId>,
    stable: Option<&Document>,
    legacy_v1: Option<&Document>,
    legacy_identity_is_unique: bool,
    source_root: &str,
    session_id: &str,
) -> Result<()> {
    let (Some(_), Some(legacy_v1)) = (stable, legacy_v1) else {
        return Ok(());
    };
    if !legacy_identity_is_unique {
        bail!(
            "ambiguous hostless legacy Remem identity for selector ({source_root:?}, {session_id:?})"
        );
    }
    if !legacy_document_ids.contains(legacy_v1.id()) {
        legacy_document_ids.push(legacy_v1.id().clone());
    }
    Ok(())
}
