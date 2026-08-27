use crate::cli::Commands;
use crate::ingest_sessions::lock_session_mutations_for_repair;
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use refine_core::infra::item_link_repair::{
    apply_repair, audit_detached_observations, plan_repair,
};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
struct DryRunOutput<'a> {
    mode: &'static str,
    database: String,
    evidence: String,
    evidence_sha256: &'a str,
    rule_version: &'a str,
    stats: &'a refine_core::infra::item_link_repair::RepairStats,
}

pub(crate) fn handle(command: &Commands, db_path: &Path) -> Result<()> {
    match command {
        Commands::AuditItemLinks {
            baseline_detached_count,
            cutoff,
        } => handle_audit(db_path, *baseline_detached_count, cutoff.as_deref()),
        Commands::RepairItemLinks {
            evidence,
            evidence_sha256,
            apply,
            backup,
        } => handle_repair(
            db_path,
            Path::new(evidence),
            evidence_sha256,
            *apply,
            backup.as_deref().map(PathBuf::from),
        ),
        _ => bail!("internal error: expected an item-link maintenance command"),
    }
}

fn handle_audit(
    db_path: &Path,
    baseline_detached_count: Option<u64>,
    cutoff: Option<&str>,
) -> Result<()> {
    if baseline_detached_count.is_some() != cutoff.is_some() {
        bail!("--baseline-detached-count and --cutoff must be supplied together");
    }
    let cutoff = cutoff
        .map(|raw| {
            DateTime::parse_from_rfc3339(raw)
                .map(|value| value.with_timezone(&Utc))
                .with_context(|| format!("--cutoff must be RFC3339, got {raw:?}"))
        })
        .transpose()?;
    let audit = audit_detached_observations(db_path)
        .with_context(|| format!("audit failed for {}", db_path.display()))?;
    println!("{}", serde_json::to_string_pretty(&audit)?);

    if let (Some(baseline), Some(cutoff)) = (baseline_detached_count, cutoff) {
        let newest_after_cutoff = audit
            .newest_detached_created_at
            .as_deref()
            .map(DateTime::parse_from_rfc3339)
            .transpose()
            .context("database contains an invalid detached created_at timestamp")?
            .is_some_and(|newest| newest.with_timezone(&Utc) > cutoff);
        if audit.detached_observations > baseline || newest_after_cutoff {
            bail!(
                "detached observation baseline breached: count={} baseline={} newest_created_at={:?} cutoff={}",
                audit.detached_observations,
                baseline,
                audit.newest_detached_created_at,
                cutoff.to_rfc3339()
            );
        }
    }
    Ok(())
}

fn handle_repair(
    db_path: &Path,
    evidence_path: &Path,
    evidence_sha256: &str,
    apply: bool,
    backup_path: Option<PathBuf>,
) -> Result<()> {
    if !apply {
        if backup_path.is_some() {
            bail!("--backup is only valid together with --apply");
        }
        let plan = plan_repair(db_path, evidence_path, evidence_sha256)
            .context("item-link repair dry-run failed")?;
        let output = DryRunOutput {
            mode: "dry-run",
            database: db_path.display().to_string(),
            evidence: evidence_path.display().to_string(),
            evidence_sha256: &plan.evidence_sha256,
            rule_version: plan.rule_version,
            stats: &plan.stats,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    let backup_path = backup_path.context("--backup is required with --apply")?;
    let _mutation_lock = lock_session_mutations_for_repair(db_path)
        .context("cannot acquire the existing session mutation lock")?;
    let report = apply_repair(db_path, evidence_path, evidence_sha256, &backup_path)
        .context("item-link repair apply failed")?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn repair_defaults_to_dry_run_and_apply_requires_backup_at_runtime() {
        let cli = crate::cli::Cli::try_parse_from([
            "refine",
            "repair-item-links",
            "--evidence",
            "evidence.db",
            "--evidence-sha256",
            &"a".repeat(64),
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Commands::RepairItemLinks { apply: false, .. }
        ));
    }

    #[test]
    fn audit_guard_arguments_must_be_paired() {
        let error = handle_audit(Path::new("missing.db"), Some(1), None).unwrap_err();
        assert!(error.to_string().contains("must be supplied together"));
    }
}
