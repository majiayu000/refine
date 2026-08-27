//! Fail-closed audit and historical repair for detached session observations.
//!
//! The repair rule intentionally uses only immutable evidence and exact,
//! one-to-one identifiers. It never guesses a nearest document and never
//! changes session-mode provenance.

use crate::error::{InfraError, InfraResult};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OpenFlags, TransactionBehavior};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const RULE_VERSION: &str = "shadow-document-id-exact-v1";

mod backup;
mod schema_validation;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DetachedObservationAudit {
    pub detached_observations: u64,
    /// Detached rows have no parent Document event timestamp, so this is the
    /// persisted Observation creation timestamp used as the event fallback.
    pub newest_detached_event_or_created_at: Option<String>,
    pub newest_detached_created_at: Option<String>,
    pub newest_detached_updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct RepairStats {
    pub candidate_groups: usize,
    pub candidate_items: usize,
    pub target_conflicts: usize,
    pub ambiguous_groups: usize,
    pub unproven_groups: usize,
    pub missing_current_items: usize,
    pub already_linked_items: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RepairCandidate {
    pub evidence_document_id: String,
    pub target_document_id: String,
    pub summary_title: String,
    pub item_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RepairPlan {
    pub evidence_sha256: String,
    pub rule_version: &'static str,
    pub stats: RepairStats,
    #[serde(skip_serializing)]
    pub candidates: Vec<RepairCandidate>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ApplyReport {
    pub changed_items: usize,
    pub ledger_rows_added: usize,
    pub backup_path: Option<PathBuf>,
    pub evidence_sha256: String,
    pub rule_version: &'static str,
}

#[derive(Debug, Clone)]
struct EvidenceItem {
    id: String,
    title: String,
    created_at: DateTime<Utc>,
    document_id: String,
}

#[derive(Debug, Clone)]
struct CurrentDocument {
    id: String,
    title: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
struct CurrentItem {
    is_observation: bool,
    document_id: Option<String>,
}

pub fn audit_detached_observations(db_path: &Path) -> InfraResult<DetachedObservationAudit> {
    let conn = open_read_only(db_path)?;
    schema_validation::validate_current_tables(&conn)?;
    let mut statement = conn
        .prepare(
            "SELECT created_at, updated_at FROM items
             WHERE item_type = 'observation' AND document_id IS NULL",
        )
        .map_err(db_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(db_error)?;
    let mut count = 0u64;
    let mut newest_created: Option<DateTime<Utc>> = None;
    let mut newest_updated: Option<DateTime<Utc>> = None;
    for row in rows {
        let (created_at, updated_at) = row.map_err(db_error)?;
        let created_at = parse_timestamp(&created_at)?;
        let updated_at = parse_timestamp(&updated_at)?;
        count += 1;
        newest_created = Some(newest_created.map_or(created_at, |seen| seen.max(created_at)));
        newest_updated = Some(newest_updated.map_or(updated_at, |seen| seen.max(updated_at)));
    }
    let newest_created = newest_created.map(|timestamp| timestamp.to_rfc3339());
    Ok(DetachedObservationAudit {
        detached_observations: count,
        newest_detached_event_or_created_at: newest_created.clone(),
        newest_detached_created_at: newest_created,
        newest_detached_updated_at: newest_updated.map(|timestamp| timestamp.to_rfc3339()),
    })
}

pub fn plan_repair(
    db_path: &Path,
    evidence_path: &Path,
    expected_sha256: &str,
) -> InfraResult<RepairPlan> {
    let evidence_sha256 = validate_evidence(evidence_path, expected_sha256)?;
    let current = open_read_only(db_path)?;
    let evidence = open_immutable(evidence_path)?;
    schema_validation::validate_current_tables(&current)?;
    schema_validation::validate_evidence_tables(&evidence)?;
    current.execute_batch("BEGIN").map_err(db_error)?;
    let plan = build_plan(&current, &evidence, evidence_sha256)?;
    current.execute_batch("COMMIT").map_err(db_error)?;
    validate_evidence(evidence_path, expected_sha256)?;
    Ok(plan)
}

pub fn apply_repair(
    db_path: &Path,
    evidence_path: &Path,
    expected_sha256: &str,
    backup_path: &Path,
) -> InfraResult<ApplyReport> {
    let evidence_sha256 = validate_evidence(evidence_path, expected_sha256)?;
    let evidence = open_immutable(evidence_path)?;
    schema_validation::validate_evidence_tables(&evidence)?;

    let mut current = Connection::open(db_path).map_err(db_error)?;
    super::configure_sqlite_connection(&current)?;
    let tx = current
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_error)?;
    schema_validation::validate_apply_schema(&tx)?;
    let plan = build_plan(&tx, &evidence, evidence_sha256.clone())?;
    validate_evidence(evidence_path, expected_sha256)?;
    if plan.candidates.is_empty() {
        return Ok(ApplyReport {
            changed_items: 0,
            ledger_rows_added: 0,
            backup_path: None,
            evidence_sha256,
            rule_version: RULE_VERSION,
        });
    }

    let before_items = scalar_count(&tx, "SELECT COUNT(*) FROM items")?;
    backup::create_no_clobber(db_path, backup_path)?;
    let revalidated = build_plan(&tx, &evidence, evidence_sha256.clone())?;
    validate_evidence(evidence_path, expected_sha256)?;
    if revalidated != plan {
        return Err(InfraError::Database(
            "repair predicates changed after backup; transaction rolled back".into(),
        ));
    }
    let applied_at = Utc::now().to_rfc3339();
    let mut changed_items = 0usize;
    let mut ledger_rows_added = 0usize;

    for candidate in &plan.candidates {
        for item_id in &candidate.item_ids {
            let changed = tx
                .execute(
                    "UPDATE items SET document_id = ?1
                     WHERE id = ?2 AND item_type = 'observation' AND document_id IS NULL",
                    params![candidate.target_document_id, item_id],
                )
                .map_err(db_error)?;
            if changed != 1 {
                return Err(InfraError::Database(format!(
                    "repair precondition changed for item {item_id}; transaction rolled back"
                )));
            }
            changed_items += changed;
            let inserted = tx
                .execute(
                    "INSERT INTO item_link_repair_ledger
                       (item_id, target_document_id, evidence_sha256, rule_version, applied_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        item_id,
                        candidate.target_document_id,
                        evidence_sha256,
                        RULE_VERSION,
                        applied_at
                    ],
                )
                .map_err(db_error)?;
            ledger_rows_added += inserted;
        }
    }

    let after_items = scalar_count(&tx, "SELECT COUNT(*) FROM items")?;
    if after_items != before_items {
        return Err(InfraError::Database(format!(
            "item count changed during repair: before={before_items}, after={after_items}"
        )));
    }
    verify_sqlite(&tx)?;
    tx.commit().map_err(db_error)?;

    Ok(ApplyReport {
        changed_items,
        ledger_rows_added,
        backup_path: Some(backup_path.to_path_buf()),
        evidence_sha256,
        rule_version: RULE_VERSION,
    })
}

fn build_plan(
    current: &Connection,
    evidence: &Connection,
    evidence_sha256: String,
) -> InfraResult<RepairPlan> {
    let evidence_document_ids = load_ids(evidence, "SELECT id FROM documents")?;
    let evidence_items = load_evidence_items(evidence, &evidence_document_ids)?;
    let current_documents = load_current_documents(current)?;
    let current_items = load_current_items(current)?;
    Ok(build_plan_from_rows(
        evidence_items,
        current_documents,
        current_items,
        evidence_sha256,
    ))
}

fn build_plan_from_rows(
    evidence_items: Vec<EvidenceItem>,
    current_documents: Vec<CurrentDocument>,
    current_items: HashMap<String, CurrentItem>,
    evidence_sha256: String,
) -> RepairPlan {
    let mut groups: BTreeMap<String, Vec<EvidenceItem>> = BTreeMap::new();
    for item in evidence_items {
        groups
            .entry(item.document_id.clone())
            .or_default()
            .push(item);
    }

    let mut title_counts: HashMap<String, usize> = HashMap::new();
    for document in &current_documents {
        *title_counts.entry(document.title.clone()).or_default() += 1;
    }
    let unique_documents: HashMap<String, CurrentDocument> = current_documents
        .into_iter()
        .filter(|document| title_counts.get(&document.title) == Some(&1))
        .map(|document| (document.title.clone(), document))
        .collect();

    let mut stats = RepairStats::default();
    let mut group_claims = Vec::new();
    for (evidence_document_id, mut items) in groups {
        items.sort_by(|left, right| left.id.cmp(&right.id));
        let exact_matches: Vec<(&EvidenceItem, &CurrentDocument)> = items
            .iter()
            .filter_map(|item| {
                unique_documents.get(&item.title).and_then(|document| {
                    (document.created_at - item.created_at)
                        .num_nanoseconds()
                        .is_some_and(|delta| delta.unsigned_abs() <= 1_000_000_000)
                        .then_some((item, document))
                })
            })
            .collect();
        let claimed_targets: HashSet<String> = exact_matches
            .iter()
            .map(|(_, target)| target.id.clone())
            .collect();

        if exact_matches.len() != 1 {
            if exact_matches.len() > 1
                || items.iter().any(|item| {
                    title_counts
                        .get(&item.title)
                        .is_some_and(|count| *count > 1)
                })
            {
                stats.ambiguous_groups += 1;
            } else {
                stats.unproven_groups += 1;
            }
            group_claims.push((claimed_targets, None));
            continue;
        }
        let (summary, target) = exact_matches[0];
        group_claims.push((
            claimed_targets,
            Some(RepairCandidate {
                evidence_document_id,
                target_document_id: target.id.clone(),
                summary_title: summary.title.clone(),
                item_ids: items.into_iter().map(|item| item.id).collect(),
            }),
        ));
    }

    let mut target_claims: HashMap<String, usize> = HashMap::new();
    for (claimed_targets, _) in &group_claims {
        for target_document_id in claimed_targets {
            *target_claims.entry(target_document_id.clone()).or_default() += 1;
        }
    }
    let mut candidates = Vec::new();
    for (claimed_targets, candidate) in group_claims {
        if claimed_targets
            .iter()
            .any(|target| target_claims[target] != 1)
        {
            stats.target_conflicts += 1;
            continue;
        }
        let Some(candidate) = candidate else {
            continue;
        };
        let mut group_valid = true;
        for item_id in &candidate.item_ids {
            match current_items.get(item_id) {
                None => {
                    stats.missing_current_items += 1;
                    group_valid = false;
                }
                Some(current_item) if !current_item.is_observation => {
                    stats.missing_current_items += 1;
                    group_valid = false;
                }
                Some(current_item) if current_item.document_id.is_some() => {
                    stats.already_linked_items += 1;
                    group_valid = false;
                }
                Some(_) => {}
            }
        }
        if group_valid && !candidate.item_ids.is_empty() {
            candidates.push(candidate);
        } else {
            stats.unproven_groups += 1;
        }
    }
    stats.candidate_groups = candidates.len();
    stats.candidate_items = candidates
        .iter()
        .map(|candidate| candidate.item_ids.len())
        .sum();

    RepairPlan {
        evidence_sha256,
        rule_version: RULE_VERSION,
        stats,
        candidates,
    }
}

fn load_evidence_items(
    conn: &Connection,
    evidence_document_ids: &HashSet<String>,
) -> InfraResult<Vec<EvidenceItem>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, title, created_at, document_id FROM items
             WHERE item_type = 'observation' AND document_id IS NOT NULL",
        )
        .map_err(db_error)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(db_error)?;
    let mut items = Vec::new();
    for row in rows {
        let (id, title, created_at, document_id) = row.map_err(db_error)?;
        if evidence_document_ids.contains(&document_id) {
            continue;
        }
        items.push(EvidenceItem {
            id,
            title,
            created_at: parse_timestamp(&created_at)?,
            document_id,
        });
    }
    Ok(items)
}

fn load_current_documents(conn: &Connection) -> InfraResult<Vec<CurrentDocument>> {
    let mut stmt = conn
        .prepare("SELECT id, title, created_at FROM documents WHERE title IS NOT NULL")
        .map_err(db_error)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(db_error)?;
    let mut documents = Vec::new();
    for row in rows {
        let (id, title, created_at) = row.map_err(db_error)?;
        documents.push(CurrentDocument {
            id,
            title,
            created_at: parse_timestamp(&created_at)?,
        });
    }
    Ok(documents)
}

fn load_current_items(conn: &Connection) -> InfraResult<HashMap<String, CurrentItem>> {
    let mut stmt = conn
        .prepare("SELECT id, item_type, document_id FROM items")
        .map_err(db_error)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(db_error)?;
    let mut items = HashMap::new();
    for row in rows {
        let (id, item_type, document_id) = row.map_err(db_error)?;
        items.insert(
            id,
            CurrentItem {
                is_observation: item_type == "observation",
                document_id,
            },
        );
    }
    Ok(items)
}

fn validate_evidence(path: &Path, expected_sha256: &str) -> InfraResult<String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        InfraError::Database(format!(
            "cannot inspect evidence {}: {error}",
            path.display()
        ))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(InfraError::Database(format!(
            "evidence must be a regular non-symlink file: {}",
            path.display()
        )));
    }
    let expected = expected_sha256.trim().to_ascii_lowercase();
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(InfraError::Database(
            "expected evidence SHA-256 must contain exactly 64 hexadecimal characters".into(),
        ));
    }
    let actual = hash_file(path)?;
    if actual != expected {
        return Err(InfraError::Database(format!(
            "evidence SHA-256 mismatch: expected {expected}, got {actual}"
        )));
    }
    Ok(actual)
}

fn verify_sqlite(conn: &Connection) -> InfraResult<()> {
    let quick: String = conn
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .map_err(db_error)?;
    if quick != "ok" {
        return Err(InfraError::Database(format!(
            "SQLite quick_check failed: {quick}"
        )));
    }
    let fk = scalar_count(conn, "SELECT COUNT(*) FROM pragma_foreign_key_check")?;
    if fk != 0 {
        return Err(InfraError::Database(format!(
            "SQLite foreign_key_check found {fk} violation(s)"
        )));
    }
    Ok(())
}

fn open_read_only(path: &Path) -> InfraResult<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(db_error)?;
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(db_error)?;
    Ok(conn)
}

fn open_immutable(path: &Path) -> InfraResult<Connection> {
    let uri = format!("file:{}?immutable=1", encode_uri_path(path));
    Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(db_error)
}

fn encode_uri_path(path: &Path) -> String {
    let mut encoded = String::new();
    for byte in path.to_string_lossy().as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(*byte as char)
            }
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    encoded
}

fn load_ids(conn: &Connection, sql: &str) -> InfraResult<HashSet<String>> {
    let mut stmt = conn.prepare(sql).map_err(db_error)?;
    let ids = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error)?;
    Ok(ids)
}

fn scalar_count(conn: &Connection, sql: &str) -> InfraResult<u64> {
    conn.query_row(sql, [], |row| row.get(0)).map_err(db_error)
}

fn parse_timestamp(raw: &str) -> InfraResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|error| {
            InfraError::Database(format!("invalid RFC3339 timestamp {raw:?}: {error}"))
        })
}

fn hash_file(path: &Path) -> InfraResult<String> {
    let mut file = File::open(path).map_err(|error| {
        InfraError::Database(format!("cannot open evidence {}: {error}", path.display()))
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|error| {
            InfraError::Database(format!("cannot hash evidence {}: {error}", path.display()))
        })?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn db_error(error: rusqlite::Error) -> InfraError {
    InfraError::Database(error.to_string())
}

#[cfg(test)]
mod tests;
