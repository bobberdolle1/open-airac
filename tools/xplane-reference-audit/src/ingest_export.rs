use anyhow::Result;
use chrono::{DateTime, Utc};
use openairac_export_xplane::XPlane12Exporter;
use openairac_ingest::faa_cifp::FaaCifpAdapter;
use openairac_model::{AiracCycle, CycleId, CycleStatus, SourceSnapshot, SourceSnapshotId};
use openairac_store::WorldStore;
use std::path::Path;

pub fn run_ingest_and_export(
    cifp_path: &Path,
    db_path: &Path,
    out_dir: &Path,
    effective: DateTime<Utc>,
) -> Result<()> {
    println!("Opening WorldStore at {:?}", db_path);
    if db_path.exists() {
        let _ = std::fs::remove_file(db_path);
    }
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut store = WorldStore::open(db_path)?;

    let snapshot_id = SourceSnapshotId("faa_cifp:2608:audit".to_string());
    store.insert_source_snapshot(&SourceSnapshot {
        id: snapshot_id.clone(),
        provider: "FAA_CIFP".to_string(),
        dataset: "FAACIFP18".to_string(),
        provider_revision: Some("2608".to_string()),
        airac_cycle: Some("2608".to_string()),
        effective_from: Some(effective),
        effective_until: None,
        retrieved_at: Utc::now(),
        source_uri: format!("file:{}", cifp_path.display()),
        content_sha256: "0".repeat(64),
        license_id: Some("US-GOV".to_string()),
        license_notes: Some("US Government work (public domain)".to_string()),
        parser_version: env!("CARGO_PKG_VERSION").to_string(),
    })?;

    store.insert_cycle(&AiracCycle {
        id: CycleId("2608".to_string()),
        effective_from: Some(effective),
        effective_until: None,
        status: CycleStatus::Active,
        source_uri: Some(format!("file:{}", cifp_path.display())),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        notes: Some("FAA CIFP 2608 cycle for audit".to_string()),
    })?;

    println!("Reading CIFP file {:?} into memory...", cifp_path);
    let cifp_content = std::fs::read_to_string(cifp_path)?;
    println!("Ingesting CIFP file into store...");
    let report = FaaCifpAdapter::ingest_cifp(&cifp_content, &snapshot_id, effective, &mut store)?;
    println!(
        "Ingested: {} waypoints, {} navaids, {} airway legs, {} procedure legs, {} airports, {} runways (lines: {}, unsupported: {}, errors: {})",
        report.waypoints_decoded,
        report.navaids_decoded,
        report.airway_legs_decoded,
        report.procedure_legs_decoded,
        report.airports_decoded,
        report.runways_decoded,
        report.lines_seen,
        report.unsupported_records,
        report.decode_errors,
    );

    println!("Exporting to X-Plane 12 directory {:?}...", out_dir);
    std::fs::create_dir_all(out_dir)?;
    let export_report = XPlane12Exporter::export_from_db(&store, effective, out_dir, true)?;
    println!(
        "Export complete: {} fixes, {} navaids, {} airway rows written (skipped {} fixes, {} navaids, {} airway legs)",
        export_report.fixes_written,
        export_report.navaids_written,
        export_report.airway_rows_written,
        export_report.fixes_skipped,
        export_report.navaids_skipped,
        export_report.airway_legs_skipped,
    );

    Ok(())
}
