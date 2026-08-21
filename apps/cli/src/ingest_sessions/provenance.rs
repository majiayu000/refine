use anyhow::{Context, Result};
use refine_core::knowledge::{Document, DocumentRepository, Tag};
use refine_core::session::SessionMode;
use std::sync::Arc;

fn is_session_mode_tag(tag: &str) -> bool {
    tag.starts_with("session_mode_")
}

/// Replace only Refine-owned provenance tags, preserving every other tag and
/// the document payload. No LLM extraction is involved.
pub(super) async fn backfill_session_metadata(
    doc_store: &Arc<dyn DocumentRepository>,
    document: &Document,
    mode: SessionMode,
    persist: bool,
) -> Result<bool> {
    let mode_tag = Tag::new(mode.as_tag()).context("build session provenance tag")?;
    let mut items = doc_store
        .find_items_by_document_id(document.id())
        .await
        .context("load observations for session provenance backfill")?;
    let mut changed = false;

    for item in &mut items {
        let existing_modes: Vec<&str> = item
            .tags()
            .iter()
            .map(|tag| tag.as_str())
            .filter(|tag| is_session_mode_tag(tag))
            .collect();
        if existing_modes == [mode.as_tag()]
            || (mode == SessionMode::Unknown
                && existing_modes
                    .iter()
                    .copied()
                    .any(|tag| tag != SessionMode::Unknown.as_tag()))
        {
            continue;
        }
        let mut tags: Vec<Tag> = item
            .tags()
            .iter()
            .filter(|tag| !is_session_mode_tag(tag.as_str()))
            .cloned()
            .collect();
        tags.push(mode_tag.clone());
        if tags == item.tags() {
            continue;
        }
        item.set_tags(tags)
            .context("replace session provenance tag on observation")?;
        changed = true;
    }

    if changed && persist {
        doc_store
            .save_with_replaced_items(document, &items)
            .await
            .context("persist session provenance metadata backfill")?;
    }
    Ok(changed)
}
