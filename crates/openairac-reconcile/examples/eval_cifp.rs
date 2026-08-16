//! Real-data evaluation helper: ingest the FAA CIFP file (cycle 2608)
//! into an existing OpenAIRAC database via the library adapter, so the
//! reconciliation CLI can run against FAA + OurAirports data.
//!
//! Usage:
//!   cargo run -p openairac-reconcile --example eval_cifp -- <db> <cifp-file>
//!
//! Manual live-network tool; never run in CI.

use anyhow::Result;
use chrono::{DateTime, TimeZone, Utc};
use openairac_ingest::faa_cifp::FaaCifpAdapter;
use openairac_model::{AiracCycle, CycleId, CycleStatus, SourceSnapshot, SourceSnapshotId};
use openairac_store::WorldStore;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let db = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "./data/world.openairac.sqlite".into());
    let cifp = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| "/tmp/FAACIFP18".into());

    let mut store = WorldStore::open(&db)?;
    let effective: DateTime<Utc> = Utc
        .with_ymd_and_hms(2026, 8, 6, 9, 0, 0)
        .single()
        .expect("valid date");

    let snapshot_id = SourceSnapshotId("faa_cifp:2608:eval".to_string());
    let snapshot = SourceSnapshot {
        id: snapshot_id.clone(),
        provider: "FAA_CIFP".to_string(),
        dataset: "FAACIFP18".to_string(),
        provider_revision: Some("2608".to_string()),
        airac_cycle: Some("2608".to_string()),
        effective_from: Some(effective),
        effective_until: None,
        retrieved_at: Utc::now(),
        source_uri: "file:/tmp/FAACIFP18".to_string(),
        content_sha256: "0".repeat(64),
        license_id: Some("US-GOV".to_string()),
        license_notes: Some("US Government work (public domain)".to_string()),
        parser_version: env!("CARGO_PKG_VERSION").to_string(),
    };
    store.insert_source_snapshot(&snapshot)?;
    store
        .insert_cycle(&AiracCycle {
            id: CycleId("2608".to_string()),
            effective_from: Some(effective),
            effective_until: None,
            status: CycleStatus::Active,
            source_uri: Some("file:/tmp/FAACIFP18".to_string()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            notes: Some("evaluation ingest".to_string()),
        })
        .ok();

    let content = std::fs::read_to_string(&cifp)?;
    println!("ingesting {} bytes of CIFP into {db}...", content.len());
    let scan = FaaCifpAdapter::ingest_cifp(&content, &snapshot_id, effective, &mut store)?;
    println!(
        "CIFP scan: lines {}, waypoints {}, navaids {}, airway legs {}, procedure legs {}, unsupported {}, errors {}",
        scan.lines_seen,
        scan.waypoints_decoded,
        scan.navaids_decoded,
        scan.airway_legs_decoded,
        scan.procedure_legs_decoded,
        scan.unsupported_records,
        scan.decode_errors,
    );
    Ok(())
}
