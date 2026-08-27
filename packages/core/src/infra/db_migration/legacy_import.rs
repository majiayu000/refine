use rusqlite::Connection;
use std::path::Path;

#[cfg(test)]
mod observation_import_tests;

pub(super) fn run(
    conn: &Connection,
    candidate: &Path,
    signature_before: &str,
    content_hash: &str,
) -> Result<usize, String> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|error| format!("failed to start migration transaction: {error}"))?;

    // Legacy schemas either predate document_id or legitimately contain
    // historical detached observations. Pause only the two forward-write
    // guards inside this dedicated import transaction. A rollback restores
    // them automatically, and no normal application write uses this path.
    crate::infra::observation_integrity::suspend_for_legacy_import(&tx)
        .map_err(|error| format!("failed to suspend observation invariant: {error}"))?;
    let rows = super::copy_all_tables(&tx, "refine_migration_src", candidate)?;
    crate::infra::observation_integrity::ensure_triggers(&tx)
        .map_err(|error| format!("failed to restore observation invariant: {error}"))?;
    crate::infra::observation_integrity::verify_triggers(&tx)
        .map_err(|error| format!("failed to verify observation invariant: {error}"))?;

    let signature_after = super::source_signature(candidate)?;
    if signature_after != signature_before {
        return Err(format!(
            "legacy DB {} changed while its migration snapshot was imported; retry migration",
            candidate.display()
        ));
    }
    super::save_migration_state(&tx, candidate, &signature_after, content_hash)?;
    tx.commit()
        .map_err(|error| format!("failed to commit migration transaction: {error}"))?;
    Ok(rows)
}
