//! End-to-end pipeline integration test:
//!
//! ```text
//! fixture datasets (OurAirports CSV + FAA CIFP records)
//!    → ingest (transactional, fail-closed)
//!    → temporal SQLite store
//!    → temporal queries (world_at)
//!    → structural validation
//!    → X-Plane 12 export (staged, atomic)
//! ```

use chrono::{DateTime, TimeDelta, Utc};
use openairac_export_xplane::XPlane12Exporter;
use openairac_ingest::faa_cifp::FaaCifpAdapter;
use openairac_ingest::ourairports::OurAirportsImporter;
use openairac_ingest::provider::{FetchedDataset, sha256_hex};
use openairac_model::SourceSnapshotId;
use openairac_store::WorldStore;

const AIRPORTS_CSV: &str = "\
id,ident,type,name,latitude_deg,longitude_deg,elevation_ft,iso_country,municipality
1,KSFO,large_airport,San Francisco International Airport,37.6188,-122.3750,13,US,San Francisco
2,KJFK,large_airport,John F Kennedy International Airport,40.6398,-73.7789,13,US,New York
";

const RUNWAYS_CSV: &str = "\
id,airport_ref,airport_ident,length_ft,width_ft,surface,le_ident,le_latitude_deg,le_longitude_deg,le_elevation_ft,le_heading_degT,he_ident,he_latitude_deg,he_longitude_deg,he_elevation_ft
101,1,KSFO,11870,200,ASP,28R,37.6188,-122.3750,13,284.0,10L,37.6140,-122.3900,11
102,2,KJFK,14511,200,ASP,13L,40.6398,-73.7789,13,134.0,31R,40.6200,-73.7500,11
";

const NAVAIDS_CSV: &str = "\
id,filename,ident,name,type,frequency_khz,latitude_deg,longitude_deg,elevation_ft,associated_airport,magnetic_variation_deg
201,SFO.navaid,SFO,San Francisco VOR-DME,VOR-DME,115800,37.6195,-122.3739,13,KSFO,-13.0
202,ABI.navaid,ABI,Abilene VORTAC,VORTAC,113700,32.4813,-99.8635,1809,,10.0
";

// Real records from FAA CIFP cycle 2608 (public domain US Government work).
const CIFP_CONTENT: &str = "\
SUSAEAENRT   AABBZ K50    W     N37522460W086183136                       W0047     NAR           AABBZ                    272032407
SUSAD        ABI   K4011370VTHW N32285279W099514843    N32285279W099514843E0100018092     NARABILENE                       249601810
SUSADB       AA    K3003650HOLW N47003259W096485466                       E0040           NARKENIE                         268711805
SUSAERENRT   J1    K 0J1    001J1  A                                           NAR           J1                         200091305   
";

fn fixture_dataset(name: &str, content: &str, retrieved_at: DateTime<Utc>) -> FetchedDataset {
    FetchedDataset {
        provider_name: "OurAirports".to_string(),
        dataset_name: name.to_string(),
        source_uri: "fixture".to_string(),
        content_sha256: sha256_hex(content.as_bytes()),
        retrieved_at,
        provider_revision: Some("fixture".to_string()),
        raw_content: content.to_string(),
    }
}

fn temp_db_path(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "openairac_pipeline_{tag}_{}.sqlite",
        std::process::id()
    ))
}

#[test]
fn test_fixture_to_export_pipeline() {
    let db_path = temp_db_path("full");
    let _ = std::fs::remove_file(&db_path);

    let now = Utc::now();
    let mut store = WorldStore::open(&db_path).unwrap();

    // ---- 1. Ingest OurAirports fixtures transactionally --------------------
    for (name, content) in [
        ("airports", AIRPORTS_CSV),
        ("runways", RUNWAYS_CSV),
        ("navaids", NAVAIDS_CSV),
    ] {
        let dataset = fixture_dataset(name, content, now);
        let report = OurAirportsImporter::ingest_dataset(&dataset, &mut store).unwrap();
        assert!(
            report.errors.is_empty(),
            "ingest errors: {:?}",
            report.errors
        );
        assert_eq!(report.records_accepted(), 2);
    }

    // ---- 2. Ingest FAA CIFP fixtures ---------------------------------------
    let snapshot_id = SourceSnapshotId("snap-faa-pipeline".to_string());
    let snapshot = openairac_model::SourceSnapshot {
        id: snapshot_id.clone(),
        provider: "FAA_CIFP".to_string(),
        dataset: "FAACIFP18".to_string(),
        provider_revision: Some("2608".to_string()),
        airac_cycle: Some("2608".to_string()),
        effective_from: Some(now),
        effective_until: None,
        retrieved_at: now,
        source_uri: "fixture".to_string(),
        content_sha256: sha256_hex(CIFP_CONTENT.as_bytes()),
        license_id: Some("US-GOV".to_string()),
        license_notes: Some("US Government work (public domain)".to_string()),
        parser_version: env!("CARGO_PKG_VERSION").to_string(),
    };
    store
        .transact(|conn| {
            openairac_store::insert_source_snapshot_conn(conn, &snapshot)?;
            Ok(())
        })
        .unwrap();
    let scan = FaaCifpAdapter::ingest_cifp(
        CIFP_CONTENT,
        &snapshot_id,
        now + TimeDelta::seconds(1),
        &mut store,
    )
    .unwrap();
    assert_eq!(scan.waypoints_decoded, 1);
    assert_eq!(scan.navaids_decoded, 2);
    assert_eq!(scan.unsupported_records, 1); // the ER airway record

    // ---- 3. Temporal queries ------------------------------------------------
    let airports = store.query_airports_at(now).unwrap();
    assert_eq!(airports.len(), 2);
    let ksfo = airports.iter().find(|a| a.ident == "KSFO").unwrap();
    assert_eq!(ksfo.runways.len(), 1);

    // CIFP records become valid one second after the OurAirports batch.
    let waypoints = store
        .query_waypoints_at(now + TimeDelta::seconds(1))
        .unwrap();
    assert_eq!(waypoints.len(), 1);
    assert_eq!(waypoints[0].ident, "AABBZ");

    let navaids = store.query_navaids_at(now + TimeDelta::seconds(1)).unwrap();
    assert_eq!(navaids.len(), 4); // 2 OurAirports + ABI VORTAC + KENIE NDB

    // ---- 4. Structural validation ------------------------------------------
    let issues = store.validate().unwrap();
    assert!(issues.is_empty(), "store issues: {issues:?}");

    // ---- 5. X-Plane export ---------------------------------------------------
    let out_dir =
        std::env::temp_dir().join(format!("openairac_pipeline_xp_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out_dir);
    let export_date = now + TimeDelta::seconds(2);

    // OurAirports navaids lack ICAO regions -> skipped; CIFP navaids and the
    // waypoint carry regions -> exported. The export must succeed.
    let report = XPlane12Exporter::export_from_db(&store, export_date, &out_dir, false).unwrap();
    assert_eq!(report.fixes_written, 1);
    assert_eq!(report.navaids_written, 2); // ABI VORTAC + KENIE NDB
    assert_eq!(report.navaids_skipped, 2); // SFO/ABI OurAirports duplicates

    let fix_content = std::fs::read_to_string(out_dir.join("earth_fix.dat")).unwrap();
    assert!(fix_content.contains("AABBZ ENRT K5"));
    assert!(fix_content.ends_with("99\n"));

    let nav_content = std::fs::read_to_string(out_dir.join("earth_nav.dat")).unwrap();
    assert!(nav_content.contains("ABI  ENRT  K4 NARABILENE VORTAC"));
    assert!(nav_content.contains("AA   ENRT  K3"));
    assert!(nav_content.contains("1200 Version - data cycle"));
    assert!(nav_content.ends_with("99\n"));

    let _ = std::fs::remove_dir_all(&out_dir);
    let _ = std::fs::remove_file(&db_path);
    // WAL sidecar files.
    let _ = std::fs::remove_file(format!("{}-wal", db_path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", db_path.display()));
}

#[test]
fn test_future_revision_preload_and_rollback() {
    let db_path = temp_db_path("preload");
    let _ = std::fs::remove_file(&db_path);
    let now = Utc::now();
    let mut store = WorldStore::open(&db_path).unwrap();

    // Preload a revision valid in one hour.
    let future = now + TimeDelta::seconds(3600);
    let dataset = fixture_dataset("airports", AIRPORTS_CSV, future);
    let report = OurAirportsImporter::ingest_dataset(&dataset, &mut store).unwrap();
    assert_eq!(report.records_accepted(), 2);

    // Not visible now, visible after it becomes effective.
    assert_eq!(store.query_airports_at(now).unwrap().len(), 0);
    assert_eq!(store.query_airports_at(future).unwrap().len(), 2);
    assert_eq!(
        store
            .query_airports_at(future + TimeDelta::seconds(10))
            .unwrap()
            .len(),
        2
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(format!("{}-wal", db_path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", db_path.display()));
}
