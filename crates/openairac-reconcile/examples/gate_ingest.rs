//! Release-gate ingest helper: decode a local CIFP master file into a
//! fresh world database with an explicit effective instant (never
//! wall-clock-inferred). Used by `scripts/release-gate.sh`.
//!
//! Usage:
//!   cargo run -p openairac-reconcile --example gate_ingest -- \
//!     <db> <cifp-file> <effective RFC3339>

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use openairac_ingest::faa_cifp::FaaCifpAdapter;
use openairac_model::{AiracCycle, CycleId, CycleStatus, SourceSnapshot, SourceSnapshotId};
use openairac_store::WorldStore;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        bail!("usage: gate_ingest <db> <cifp-file> <effective RFC3339>");
    }
    let db = &args[1];
    let cifp = &args[2];
    let effective: DateTime<Utc> = DateTime::parse_from_rfc3339(&args[3])
        .context("parsing effective instant")?
        .with_timezone(&Utc);

    let cycle = {
        // AIRAC cycle number from the effective date (YYNN, 28-day
        // epochs from 2020-01-30) - used for provenance only.
        let days = (effective.date_naive() - chrono::NaiveDate::from_ymd_opt(2020, 1, 30).unwrap())
            .num_days();
        let num = 2001 + days / 28;
        format!("{num}")
    };

    let _ = std::fs::remove_file(db);
    let mut store = WorldStore::open(db)?;
    let snapshot_id = SourceSnapshotId(format!("faa_cifp:{}:gate", cycle));
    store.insert_source_snapshot(&SourceSnapshot {
        id: snapshot_id.clone(),
        provider: "FAA_CIFP".to_string(),
        dataset: "FAACIFP18".to_string(),
        provider_revision: Some(cycle.clone()),
        airac_cycle: Some(cycle.clone()),
        effective_from: Some(effective),
        effective_until: None,
        retrieved_at: Utc::now(),
        source_uri: format!("file:{cifp}"),
        content_sha256: "0".repeat(64),
        license_id: Some("US-GOV".to_string()),
        license_notes: Some("US Government work (public domain)".to_string()),
        parser_version: env!("CARGO_PKG_VERSION").to_string(),
    })?;
    store.insert_cycle(&AiracCycle {
        id: CycleId(cycle.clone()),
        effective_from: Some(effective),
        effective_until: None,
        status: CycleStatus::Active,
        source_uri: Some(format!("file:{cifp}")),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        notes: Some("release gate ingest".to_string()),
    })?;

    let content = std::fs::read_to_string(cifp).with_context(|| format!("reading {cifp}"))?;
    let scan = FaaCifpAdapter::ingest_cifp(&content, &snapshot_id, effective, &mut store)?;
    println!(
        "ingested cycle {cycle}: waypoints {}, navaids {}, airway legs {}, procedure legs {} \
         (unsupported {}, errors {})",
        scan.waypoints_decoded,
        scan.navaids_decoded,
        scan.airway_legs_decoded,
        scan.procedure_legs_decoded,
        scan.unsupported_records,
        scan.decode_errors
    );
    Ok(())
}
