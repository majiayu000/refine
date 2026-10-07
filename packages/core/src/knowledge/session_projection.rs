//! Versioned derived session data. This never contains a second transcript.

use super::{Document, DocumentId, Item, ItemId, RestoreParams};
use crate::error::DomainError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A stable key for an exact generated observation. Reworded claims are not
/// guessed to be equivalent; their former IDs remain available in history.
pub(crate) fn observation_key(session_ref: &str, item: &Item) -> String {
    let kind = if item.content().starts_with("认知水平:") {
        "summary"
    } else if item.tags().iter().any(|tag| tag.as_str() == "decision") {
        "decision"
    } else if item.tags().iter().any(|tag| tag.as_str() == "bugfix") {
        "bugfix"
    } else {
        "observation"
    };
    let mut digest = Sha256::new();
    for value in ["refine-observation-v1", session_ref, kind] {
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    }
    if kind != "summary" {
        let text = item
            .title()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        digest.update((text.len() as u64).to_be_bytes());
        digest.update(text.as_bytes());
    }
    format!("{:x}", digest.finalize())
}

pub fn assign_session_observation_ids(
    items: &mut [Item],
    session_ref: &str,
) -> Result<(), DomainError> {
    for item in items {
        let id = ItemId::from(format!("obs-{}", observation_key(session_ref, item)).as_str());
        *item = Item::restore(RestoreParams {
            id,
            item_type: item.item_type(),
            title: item.title().to_string(),
            summary: item.summary().to_string(),
            content: item.content().to_string(),
            tags: item.tags().to_vec(),
            source: item.source().cloned(),
            document_id: item.document_id().cloned(),
            excerpt: item.excerpt().map(str::to_string),
            created_at: item.created_at(),
            updated_at: item.updated_at(),
        })?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionProjectionMetadata {
    pub recipe_id: String,
    /// Validated message references, or an explicit unknown status. Kept
    /// independent of Remem's source_version contract.
    pub evidence: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionProjectionVersion {
    pub document_id: DocumentId,
    pub recipe_id: String,
    pub source_version: Option<String>,
}

/// Current source and derived content from one read snapshot. Machine evidence
/// excludes observations that have an authoritative human override.
#[derive(Debug)]
pub struct SessionProjectionContext {
    pub document: Document,
    pub items: Vec<Item>,
    pub evidence: serde_json::Value,
    pub history: Vec<SessionProjectionRevision>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionProjectionRevision {
    pub revision_id: String,
    pub document_id: DocumentId,
    pub session_ref: String,
    pub source_version: Option<String>,
    pub recipe_id: Option<String>,
    pub archived_at: DateTime<Utc>,
    pub items: Vec<Item>,
    pub evidence: serde_json::Value,
}
