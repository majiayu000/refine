use super::legacy_migration::legacy_document_might_match_summary;
use super::provenance::replace_session_mode_tags;
use crate::remem_sessions::RememSessionSummary;
use anyhow::{bail, Result};
use refine_core::knowledge::{Document, DocumentId, DocumentRepository, RestoreDocumentParams};
use refine_core::session::{remem_snapshot_hash, SessionMode, SessionSource};
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

pub(super) fn same_projection_or_snapshot(document: &Document, projection_version: &str) -> bool {
    document.source_version().is_some_and(|stored| {
        stored == projection_version
            || match (
                remem_snapshot_hash(stored).ok(),
                remem_snapshot_hash(projection_version).ok(),
            ) {
                (Some(stored_hash), Some(current_hash)) => stored_hash == current_hash,
                _ => false,
            }
    })
}

pub(super) async fn save_referenced_session_and_delete_legacy(
    doc_store: &Arc<dyn DocumentRepository>,
    existing: &Document,
    referenced: &Document,
    legacy_document_ids: &[DocumentId],
    mode: SessionMode,
) -> Result<()> {
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
    let mut replacement_items = Vec::new();
    for document_id in &source_document_ids {
        replacement_items.extend(doc_store.find_items_by_document_id(document_id).await?);
    }
    replace_session_mode_tags(&mut replacement_items, mode)?;
    for item in &mut replacement_items {
        item.set_document_id(referenced.id().clone());
    }
    doc_store
        .save_with_replaced_items_and_delete_documents(
            referenced,
            &replacement_items,
            &source_document_ids,
            &obsolete,
        )
        .await?;
    Ok(())
}

pub(super) async fn exclude_scheduled_session_documents(
    doc_store: &Arc<dyn DocumentRepository>,
    existing: Option<&Document>,
    source: SessionSource,
    url: &str,
    source_version: &str,
    legacy_document_ids: &[DocumentId],
) -> refine_core::error::InfraResult<()> {
    let Some(existing) = existing else {
        return doc_store
            .delete_documents_with_items(legacy_document_ids)
            .await;
    };
    let referenced = referenced_session_document(existing, source, url, source_version);
    let obsolete = legacy_document_ids
        .iter()
        .filter(|id| *id != referenced.id())
        .cloned()
        .collect::<Vec<_>>();
    doc_store
        .save_with_replaced_items_and_delete_documents(&referenced, &[], &[], &obsolete)
        .await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn skip_unchanged_session(
    doc_store: &Arc<dyn DocumentRepository>,
    existing: Option<&Document>,
    existing_uses_legacy_identity: bool,
    might_have_legacy_documents: bool,
    dry_run: bool,
    source: SessionSource,
    url: &str,
    source_version: &str,
) -> refine_core::error::InfraResult<bool> {
    let Some(existing) = existing.filter(|document| {
        !existing_uses_legacy_identity
            && !might_have_legacy_documents
            && document.source_version() == Some(source_version)
    }) else {
        return Ok(false);
    };
    if !dry_run
        && (!existing.raw_content().is_empty()
            || existing.url() != url
            || existing.source() != source.as_str())
    {
        doc_store
            .save(&referenced_session_document(
                existing,
                source,
                url,
                source_version,
            ))
            .await?;
    }
    Ok(true)
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
