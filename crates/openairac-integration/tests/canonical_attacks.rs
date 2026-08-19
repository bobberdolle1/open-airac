use chrono::{DateTime, TimeZone, Utc};
use openairac_model::{
    AiracCycle, CanonicalWaypoint, Coverage, CycleId, CycleStatus, DatasetVersion, RevisionKind,
    SourceSnapshot, SourceSnapshotId, TemporalValidity, WaypointId,
};
use openairac_store::WorldStore;

#[test]
fn test_redteam_canonical_data_integrity_attacks() {
    let store = WorldStore::open_in_memory().unwrap();

    let t0: DateTime<Utc> = Utc.with_ymd_and_hms(2026, 8, 6, 9, 0, 0).unwrap();
    let t1: DateTime<Utc> = Utc.with_ymd_and_hms(2026, 9, 3, 9, 0, 0).unwrap();
    let now: DateTime<Utc> = Utc.with_ymd_and_hms(2026, 8, 19, 12, 0, 0).unwrap();

    let snap_faa = SourceSnapshotId("faa:2608".into());
    let snap_oa = SourceSnapshotId("ourairports:2608".into());

    store
        .insert_source_snapshot(&SourceSnapshot {
            id: snap_faa.clone(),
            provider: "FAA".into(),
            dataset: "CIFP".into(),
            provider_revision: Some("2608".into()),
            airac_cycle: Some("2608".into()),
            effective_from: Some(t0),
            effective_until: None,
            retrieved_at: now,
            source_uri: "http://faa".into(),
            content_sha256: "0".repeat(64),
            license_id: None,
            license_notes: None,
            parser_version: "1.0.0".into(),
        })
        .unwrap();

    store
        .insert_source_snapshot(&SourceSnapshot {
            id: snap_oa.clone(),
            provider: "OurAirports".into(),
            dataset: "navaids".into(),
            provider_revision: Some("2608".into()),
            airac_cycle: Some("2608".into()),
            effective_from: Some(t0),
            effective_until: None,
            retrieved_at: now,
            source_uri: "http://ourairports".into(),
            content_sha256: "0".repeat(64),
            license_id: None,
            license_notes: None,
            parser_version: "1.0.0".into(),
        })
        .unwrap();

    store
        .insert_cycle(&AiracCycle {
            id: CycleId("2608".into()),
            effective_from: Some(t0),
            effective_until: Some(t1),
            status: CycleStatus::Active,
            source_uri: None,
            created_at: now,
            updated_at: now,
            notes: None,
        })
        .unwrap();

    // 1. Ingest FAA waypoint FIX1 in region K1
    let wp1_k1 = CanonicalWaypoint {
        object_id: WaypointId("faa:FIX1:K1".into()),
        ident: "FIX1".into(),
        name: "FIX ONE USA".into(),
        latitude: 40.0,
        longitude: -75.0,
        region_code: "K1".into(),
        is_enroute: true,
        terminal_area_ident: None,
        waypoint_type: Some(10),
        temporal: TemporalValidity {
            valid_from: t0,
            valid_until: None,
            source_snapshot_id: snap_faa.clone(),
        },
    };
    store.insert_waypoint(&wp1_k1).unwrap();

    // 2. ATTACK: Same ident in DIFFERENT region (FIX1 in NZ) must NOT collide or overwrite
    let wp1_nz = CanonicalWaypoint {
        object_id: WaypointId("faa:FIX1:NZ".into()),
        ident: "FIX1".into(),
        name: "FIX ONE NZ".into(),
        latitude: -41.0,
        longitude: 174.0,
        region_code: "NZ".into(),
        is_enroute: true,
        terminal_area_ident: None,
        waypoint_type: Some(10),
        temporal: TemporalValidity {
            valid_from: t0,
            valid_until: None,
            source_snapshot_id: snap_faa.clone(),
        },
    };
    store.insert_waypoint(&wp1_nz).unwrap();

    let all_wps = store.query_waypoints_at(now).unwrap();
    assert_eq!(
        all_wps.len(),
        2,
        "both regional fixes must coexist independently"
    );

    // 3. ATTACK: Publication audit trail append and query
    let pub_faa = DatasetVersion {
        id: 0,
        provider: "FAA".into(),
        dataset: "CIFP".into(),
        airac_cycle: Some("2608".into()),
        content_sha256: "0".repeat(64),
        retrieved_at: now,
        revision_kind: RevisionKind::Baseline,
        coverage: Coverage::FullSnapshot,
        publication_id: Some("PUB_FAA_2608".into()),
        valid_from: Some(t0),
        notes: None,
    };
    store.insert_dataset_version(&pub_faa).unwrap();

    let pub_oa = DatasetVersion {
        id: 0,
        provider: "OurAirports".into(),
        dataset: "navaids".into(),
        airac_cycle: Some("2608".into()),
        content_sha256: "0".repeat(64),
        retrieved_at: now,
        revision_kind: RevisionKind::Baseline,
        coverage: Coverage::FullSnapshot,
        publication_id: Some("PUB_OA_2608".into()),
        valid_from: Some(t0),
        notes: None,
    };
    store.insert_dataset_version(&pub_oa).unwrap();

    let versions = store.query_dataset_versions().unwrap();
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0].publication_id.as_deref(), Some("PUB_FAA_2608"));
    assert_eq!(versions[1].publication_id.as_deref(), Some("PUB_OA_2608"));

    // 4. ATTACK: Immutable effective history rewrite must fail closed
    let mut rewrite_k1 = wp1_k1.clone();
    rewrite_k1.name = "ILLEGAL MUTATION".into();
    let res_rewrite = store.insert_waypoint(&rewrite_k1);
    assert!(
        res_rewrite.is_err(),
        "cannot rewrite active effective history"
    );

    // 5. ATTACK: Future revision at t1 updates entity at t1 while leaving t0 unchanged
    let mut wp1_k1_future = wp1_k1.clone();
    wp1_k1_future.name = "FIX ONE FUTURE 2609".into();
    wp1_k1_future.latitude = 40.5;
    wp1_k1_future.temporal.valid_from = t1;
    store.insert_waypoint(&wp1_k1_future).unwrap();

    // Query at now (2608): original name and lat
    let wp_now = store.query_waypoints_at(now).unwrap();
    let k1_now = wp_now
        .iter()
        .find(|w| w.object_id.0 == "faa:FIX1:K1")
        .unwrap();
    assert_eq!(k1_now.name, "FIX ONE USA");
    assert!((k1_now.latitude - 40.0).abs() < 1e-6);

    // Query at t1 (2609): updated future name and lat
    let wp_t1 = store.query_waypoints_at(t1).unwrap();
    let k1_t1 = wp_t1
        .iter()
        .find(|w| w.object_id.0 == "faa:FIX1:K1")
        .unwrap();
    assert_eq!(k1_t1.name, "FIX ONE FUTURE 2609");
    assert!((k1_t1.latitude - 40.5).abs() < 1e-6);
}
