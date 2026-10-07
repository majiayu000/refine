use super::{doc_ops, ops};
use crate::error::{InfraError, InfraResult};
use crate::knowledge::{
    observation_key, Document, DocumentId, Item, ItemType, SessionProjectionContext,
    SessionProjectionMetadata, SessionProjectionRevision, SessionProjectionVersion, Tag,
};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use std::collections::{HashMap, HashSet};

fn db(error: rusqlite::Error) -> InfraError {
    InfraError::Database(error.to_string())
}
fn json(error: serde_json::Error) -> InfraError {
    InfraError::Serialization(error.to_string())
}

fn has_table(conn: &Connection, table: &str) -> InfraResult<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [table],
        |row| row.get(0),
    )
    .map_err(db)
}

pub(super) fn versions(conn: &Connection) -> InfraResult<Vec<SessionProjectionVersion>> {
    // An existing pre-upgrade database is also inspectable in --dry-run mode.
    if !has_table(conn, "session_projections")? {
        return Ok(Vec::new());
    }
    let mut statement = conn.prepare("SELECT document_id, recipe_id, source_version FROM session_projections ORDER BY document_id").map_err(db)?;
    let rows = statement
        .query_map([], |row| {
            Ok(SessionProjectionVersion {
                document_id: DocumentId::from(row.get::<_, String>(0)?.as_str()),
                recipe_id: row.get(1)?,
                source_version: row.get(2)?,
            })
        })
        .map_err(db)?;
    rows.map(|row| row.map_err(db)).collect()
}

pub(super) fn context(
    conn: &Connection,
    session_ref: &str,
) -> InfraResult<Option<SessionProjectionContext>> {
    // Deferred plus SELECT-only operations also works on a read-only store.
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Deferred).map_err(db)?;
    let Some(document) = doc_ops::find_by_url(&tx, session_ref)? else {
        return Ok(None);
    };
    let items = ops::find_by_document_id(&tx, document.id().as_str())?;
    let metadata: Option<(Option<String>, String)> = tx
        .query_row(
            "SELECT source_version, evidence_json FROM session_projections WHERE document_id=?1",
            [document.id().as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(db)?;
    let mut evidence = match metadata {
        Some((source_version, evidence))
            if source_version.as_deref() == document.source_version() =>
        {
            serde_json::from_str(&evidence).map_err(json)?
        }
        _ => serde_json::Value::Null,
    };
    let mut statement = tx
        .prepare("SELECT item_id FROM observation_overrides WHERE session_ref=?1")
        .map_err(db)?;
    let overridden = statement
        .query_map([session_ref], |row| row.get::<_, String>(0))
        .map_err(db)?
        .collect::<Result<HashSet<_>, _>>()
        .map_err(db)?;
    drop(statement);
    if let Some(observations) = evidence
        .get_mut("observations")
        .and_then(serde_json::Value::as_array_mut)
    {
        observations.retain(|observation| {
            !observation
                .get("item_id")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|id| overridden.contains(id))
        });
    }
    let history = history(&tx, document.id().as_str(), 20)?;
    tx.commit().map_err(db)?;
    Ok(Some(SessionProjectionContext {
        document,
        items,
        evidence,
        history,
    }))
}

pub(super) fn history(
    conn: &Connection,
    document_id: &str,
    limit: usize,
) -> InfraResult<Vec<SessionProjectionRevision>> {
    if !has_table(conn, "session_projection_history")? {
        return Ok(Vec::new());
    }
    let mut statement = conn.prepare("SELECT revision_id, document_id, session_ref, source_version, recipe_id, archived_at, items_json, evidence_json FROM session_projection_history WHERE document_id=?1 ORDER BY archived_at DESC, revision_id DESC LIMIT ?2").map_err(db)?;
    let rows = statement
        .query_map(params![document_id, limit.min(1_000) as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
            ))
        })
        .map_err(db)?;
    let mut history = Vec::new();
    for row in rows {
        let (
            revision_id,
            document_id,
            session_ref,
            source_version,
            recipe_id,
            archived_at,
            items,
            evidence,
        ) = row.map_err(db)?;
        history.push(SessionProjectionRevision {
            revision_id,
            document_id: DocumentId::from(document_id.as_str()),
            session_ref,
            source_version,
            recipe_id,
            archived_at: DateTime::parse_from_rfc3339(&archived_at)
                .map_err(|e| InfraError::Serialization(e.to_string()))?
                .with_timezone(&Utc),
            items: serde_json::from_str(&items).map_err(json)?,
            evidence: serde_json::from_str(&evidence).map_err(json)?,
        });
    }
    Ok(history)
}

pub(super) fn archive(conn: &Connection, document_ids: &[String]) -> InfraResult<()> {
    let mut seen = HashSet::new();
    for id in document_ids {
        if !seen.insert(id) {
            continue;
        }
        let Some(document) = doc_ops::find_by_id(conn, id)? else {
            continue;
        };
        if !document.url().starts_with("remem://raw-session/v2/") {
            continue;
        }
        let items = ops::find_by_document_id(conn, id)?;
        let state: Option<(String, String)> = conn
            .query_row(
                "SELECT recipe_id, evidence_json FROM session_projections WHERE document_id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(db)?;
        if items.is_empty() && state.is_none() {
            continue;
        }
        let (recipe, evidence) = match state {
            Some((recipe, evidence)) => (Some(recipe), evidence),
            None => (None, "{\"status\":\"unknown\"}".to_string()),
        };
        conn.execute("INSERT INTO session_projection_history (revision_id, document_id, session_ref, source_version, recipe_id, archived_at, items_json, evidence_json) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)", params![uuid::Uuid::new_v4().to_string(), id, document.url(), document.source_version(), recipe, Utc::now().to_rfc3339(), serde_json::to_string(&items).map_err(json)?, evidence]).map_err(db)?;
    }
    Ok(())
}

pub(super) fn save_metadata(
    conn: &Connection,
    document: &Document,
    metadata: &SessionProjectionMetadata,
) -> InfraResult<()> {
    if metadata.recipe_id.trim().is_empty() {
        return Err(InfraError::Database(
            "session projection recipe identity is empty".into(),
        ));
    }
    conn.execute("INSERT INTO session_projections (document_id, recipe_id, source_version, evidence_json, completed_at) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(document_id) DO UPDATE SET recipe_id=excluded.recipe_id, source_version=excluded.source_version, evidence_json=excluded.evidence_json, completed_at=excluded.completed_at", params![document.id().as_str(), metadata.recipe_id, document.source_version(), serde_json::to_string(&metadata.evidence).map_err(json)?, Utc::now().to_rfc3339()]).map_err(db)?;
    Ok(())
}

fn override_context(conn: &Connection, item: &Item) -> InfraResult<Option<(String, String)>> {
    if item.item_type() != ItemType::Observation {
        return Ok(None);
    }
    let Some(document_id) = item.document_id() else {
        return Ok(None);
    };
    let Some(document) = doc_ops::find_by_id(conn, document_id.as_str())? else {
        return Ok(None);
    };
    if !document.url().starts_with("remem://raw-session/v2/") {
        return Ok(None);
    }
    let previous: Option<String> = conn.query_row("SELECT logical_key FROM observation_overrides WHERE session_ref=?1 AND item_id=?2 LIMIT 1", params![document.url(), item.id().as_str()], |row| row.get(0)).optional().map_err(db)?;
    Ok(Some((
        document.url().to_string(),
        previous.unwrap_or_else(|| observation_key(document.url(), item)),
    )))
}

fn record_override(conn: &Connection, before: &Item, after: Option<&Item>) -> InfraResult<()> {
    let Some((session_ref, key)) = override_context(conn, before)? else {
        return Ok(());
    };
    let value = after.map(serde_json::to_string).transpose().map_err(json)?;
    conn.execute("INSERT INTO observation_overrides (session_ref, logical_key, item_id, item_json, deleted, changed_at) VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(session_ref,logical_key) DO UPDATE SET item_id=excluded.item_id, item_json=excluded.item_json, deleted=excluded.deleted, changed_at=excluded.changed_at", params![session_ref, key, before.id().as_str(), value, i64::from(after.is_none()), Utc::now().to_rfc3339()]).map_err(db)?;
    Ok(())
}

/// The generic Item save API represents an explicit edit. Machine projection
/// writes use ops::save inside their own transaction and do not create overrides.
pub(super) fn save_explicit_item(conn: &Connection, item: &Item) -> InfraResult<()> {
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate).map_err(db)?;
    if let Some(before) = ops::find_by_id(&tx, item.id().as_str())? {
        if before.title() != item.title()
            || before.summary() != item.summary()
            || before.content() != item.content()
            || before.tags() != item.tags()
            || before.excerpt() != item.excerpt()
            || before
                .source()
                .map(|source| (&source.platform, &source.url))
                != item.source().map(|source| (&source.platform, &source.url))
        {
            record_override(&tx, &before, Some(item))?;
        }
    }
    ops::save(&tx, item)?;
    tx.commit().map_err(db)
}

/// Explicit deletion is a tombstone; a later machine rerun must not resurrect
/// the same observation. Ordinary replacement deletes never enter this method.
pub(super) fn delete_explicit_item(conn: &Connection, id: &str) -> InfraResult<bool> {
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate).map_err(db)?;
    if let Some(before) = ops::find_by_id(&tx, id)? {
        record_override(&tx, &before, None)?;
    }
    let deleted = ops::delete(&tx, id)?;
    tx.commit().map_err(db)?;
    Ok(deleted)
}

pub(super) fn apply_overrides(
    conn: &Connection,
    document: &Document,
    incoming: Vec<Item>,
) -> InfraResult<Vec<Item>> {
    if !document.url().starts_with("remem://raw-session/v2/") {
        return Ok(incoming);
    }
    let mut statement = conn.prepare("SELECT logical_key, item_json, deleted FROM observation_overrides WHERE session_ref=?1 ORDER BY logical_key").map_err(db)?;
    let rows = statement
        .query_map([document.url()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, bool>(2)?,
            ))
        })
        .map_err(db)?;
    let mut overrides: HashMap<String, Option<Item>> = HashMap::new();
    for row in rows {
        let (key, payload, deleted) = row.map_err(db)?;
        let item = if deleted {
            None
        } else {
            Some(
                serde_json::from_str(&payload.ok_or_else(|| {
                    InfraError::Database("curated observation payload is missing".into())
                })?)
                .map_err(json)?,
            )
        };
        overrides.insert(key, item);
    }
    let mut result = Vec::new();
    let mut seen_ids = HashSet::new();
    let mut applied_overrides = HashSet::new();
    for mut item in incoming {
        let key = observation_key(document.url(), &item);
        match overrides.get(&key) {
            Some(None) => continue,
            Some(Some(curated)) => {
                if !applied_overrides.insert(key) {
                    continue;
                }
                item = curated.clone();
                mark_curated(&mut item, document, false)?;
            }
            None => {}
        }
        if seen_ids.insert(item.id().clone()) {
            result.push(item);
        }
    }
    let mut unmatched: Vec<_> = overrides.into_iter().collect();
    unmatched.sort_by(|left, right| left.0.cmp(&right.0));
    for (key, item) in unmatched {
        if applied_overrides.contains(&key) {
            continue;
        }
        if let Some(mut item) = item {
            // A model rewording cannot erase a human edit. Keep it visible and
            // mark it for review instead of guessing a different matching claim.
            mark_curated(&mut item, document, true)?;
            if seen_ids.insert(item.id().clone()) {
                result.push(item);
            }
        }
    }
    Ok(result)
}

fn mark_curated(item: &mut Item, document: &Document, unmatched: bool) -> InfraResult<()> {
    item.set_document_id(document.id().clone());
    let mut tags = item
        .tags()
        .iter()
        .filter(|tag| tag.as_str() != "curation_needs_review" && tag.as_str() != "curated")
        .filter(|tag| !tag.as_str().starts_with("session_mode_"))
        .cloned()
        .collect::<Vec<_>>();
    let mode = document
        .source_version()
        .and_then(|version| version.rsplit(':').next());
    let mode_tag = match mode {
        Some("interactive") => "session_mode_interactive",
        Some("unattended") => "session_mode_unattended",
        Some("subagent") => "session_mode_subagent",
        _ => "session_mode_unknown",
    };
    tags.push(Tag::new(mode_tag).map_err(|e| InfraError::Serialization(e.to_string()))?);
    // Preserve all existing user tags. If the tag budget is full, the override
    // record remains authoritative even when there is no room for a UI hint.
    if tags.len() < 20 {
        tags.push(Tag::new("curated").map_err(|e| InfraError::Serialization(e.to_string()))?);
    }
    if unmatched && tags.len() < 20 {
        tags.push(
            Tag::new("curation_needs_review")
                .map_err(|e| InfraError::Serialization(e.to_string()))?,
        );
    }
    item.set_tags(tags)
        .map_err(|e| InfraError::Serialization(e.to_string()))
}
