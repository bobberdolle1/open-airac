use chrono::{DateTime, Duration, TimeZone, Utc};
use openairac_model::{
    AiracCycle, CanonicalWaypoint, CycleId, CycleStatus, SourceSnapshotId, TemporalValidity,
    WaypointId,
};
use openairac_store::WorldStore;

#[test]
fn test_redteam_temporal_attacks() {
    let mut store = WorldStore::open_in_memory().unwrap();

    let t_2608_eff: DateTime<Utc> = Utc.with_ymd_and_hms(2026, 8, 6, 9, 0, 0).unwrap();
    let t_2609_eff: DateTime<Utc> = Utc.with_ymd_and_hms(2026, 9, 3, 9, 0, 0).unwrap();
    let now_test: DateTime<Utc> = Utc.with_ymd_and_hms(2026, 8, 19, 12, 0, 0).unwrap();

    let snap_2608 = SourceSnapshotId("faa:2608".into());
    let snap_2609 = SourceSnapshotId("faa:2609".into());

    store
        .insert_cycle(&AiracCycle {
            id: CycleId("2608".into()),
            effective_from: Some(t_2608_eff),
            effective_until: Some(t_2609_eff),
            status: CycleStatus::Active,
            source_uri: None,
            created_at: now_test,
            updated_at: now_test,
            notes: None,
        })
        .unwrap();

    store
        .insert_cycle(&AiracCycle {
            id: CycleId("2609".into()),
            effective_from: Some(t_2609_eff),
            effective_until: None,
            status: CycleStatus::Preloaded,
            source_uri: None,
            created_at: now_test,
            updated_at: now_test,
            notes: None,
        })
        .unwrap();

    let snap_meta_2608 = openairac_model::SourceSnapshot {
        id: snap_2608.clone(),
        provider: "FAA".into(),
        dataset: "CIFP".into(),
        provider_revision: Some("2608".into()),
        airac_cycle: Some("2608".into()),
        effective_from: Some(t_2608_eff),
        effective_until: Some(t_2609_eff),
        retrieved_at: now_test,
        source_uri: "http://test".into(),
        content_sha256: "0".repeat(64),
        license_id: None,
        license_notes: None,
        parser_version: "1.0.0".into(),
    };
    store.insert_source_snapshot(&snap_meta_2608).unwrap();

    let snap_meta_2609 = openairac_model::SourceSnapshot {
        id: snap_2609.clone(),
        provider: "FAA".into(),
        dataset: "CIFP".into(),
        provider_revision: Some("2609".into()),
        airac_cycle: Some("2609".into()),
        effective_from: Some(t_2609_eff),
        effective_until: None,
        retrieved_at: now_test,
        source_uri: "http://test".into(),
        content_sha256: "0".repeat(64),
        license_id: None,
        license_notes: None,
        parser_version: "1.0.0".into(),
    };
    store.insert_source_snapshot(&snap_meta_2609).unwrap();

    // Ingest 2608 waypoint (active today)
    let wp_2608 = CanonicalWaypoint {
        object_id: WaypointId("faa:FIX_A".into()),
        ident: "FIX_A".into(),
        name: "FIX A 2608".into(),
        latitude: 40.0,
        longitude: -75.0,
        region_code: "K6".into(),
        is_enroute: true,
        terminal_area_ident: None,
        waypoint_type: Some(10),
        temporal: TemporalValidity {
            valid_from: t_2608_eff,
            valid_until: Some(t_2609_eff),
            source_snapshot_id: snap_2608.clone(),
        },
    };
    store.insert_waypoint(&wp_2608).unwrap();

    // Ingest 2609 waypoint (future revision of FIX_A with moved lat/lon)
    let wp_2609 = CanonicalWaypoint {
        object_id: WaypointId("faa:FIX_A".into()),
        ident: "FIX_A".into(),
        name: "FIX A 2609".into(),
        latitude: 40.5,
        longitude: -75.5,
        region_code: "K6".into(),
        is_enroute: true,
        terminal_area_ident: None,
        waypoint_type: Some(10),
        temporal: TemporalValidity {
            valid_from: t_2609_eff,
            valid_until: None,
            source_snapshot_id: snap_2609.clone(),
        },
    };
    store.insert_waypoint(&wp_2609).unwrap();

    // 1. ATTACK: Query at today's date (2026-08-19) must NEVER leak 2609 future state
    let wps_today = store.query_waypoints_at(now_test).unwrap();
    assert_eq!(wps_today.len(), 1);
    assert_eq!(wps_today[0].name, "FIX A 2608");
    assert!((wps_today[0].latitude - 40.0).abs() < 1e-6);

    // 2. ATTACK: observe_cycles at today's date must NOT activate 2609
    let report = store.observe_cycles(now_test).unwrap();
    assert!(report.activated.is_empty(), "2609 must not activate today");
    let c2609 = store.query_cycle(&CycleId("2609".into())).unwrap().unwrap();
    assert_eq!(c2609.status, CycleStatus::Preloaded);

    // 3. ATTACK: Query at 1 microsecond before 2609 boundary (2026-09-03 08:59:59.999999 UTC)
    let just_before_2609 = t_2609_eff - Duration::microseconds(1);
    let wps_before = store.query_waypoints_at(just_before_2609).unwrap();
    assert_eq!(wps_before.len(), 1);
    assert_eq!(wps_before[0].name, "FIX A 2608");

    // observe_cycles 1 microsecond before must NOT activate 2609
    let report_before = store.observe_cycles(just_before_2609).unwrap();
    assert!(report_before.activated.is_empty());

    // 4. ATTACK: Exact instant of 2609 boundary (2026-09-03 09:00:00 UTC)
    let wps_exact = store.query_waypoints_at(t_2609_eff).unwrap();
    assert_eq!(wps_exact.len(), 1);
    assert_eq!(wps_exact[0].name, "FIX A 2609");
    assert!((wps_exact[0].latitude - 40.5).abs() < 1e-6);

    // observe_cycles at boundary MUST activate 2609 and supersede 2608
    let report_exact = store.observe_cycles(t_2609_eff).unwrap();
    assert_eq!(report_exact.activated, vec![CycleId("2609".into())]);
    assert_eq!(report_exact.superseded, vec![CycleId("2608".into())]);

    // 5. ATTACK: Timezone confusion - local time JST (UTC+9) 2026-09-03 17:00:00 JST is 08:00:00 UTC (BEFORE boundary)
    let jst_before = chrono::FixedOffset::east_opt(9 * 3600)
        .unwrap()
        .with_ymd_and_hms(2026, 9, 3, 17, 0, 0)
        .unwrap()
        .with_timezone(&Utc);
    let wps_jst = store.query_waypoints_at(jst_before).unwrap();
    assert_eq!(wps_jst[0].name, "FIX A 2608"); // 1 hour before UTC 09:00:00 boundary!

    // 6. ATTACK: Attempt to activate unconfirmed cycle without effective_from fails
    let unconfirmed = AiracCycle {
        id: CycleId("2610".into()),
        effective_from: None,
        effective_until: None,
        status: CycleStatus::Discovered,
        source_uri: None,
        created_at: now_test,
        updated_at: now_test,
        notes: None,
    };
    store.insert_cycle(&unconfirmed).unwrap();
    let rep_unconf = store
        .observe_cycles(t_2609_eff + Duration::days(30))
        .unwrap();
    let c2610 = store.query_cycle(&CycleId("2610".into())).unwrap().unwrap();
    assert_eq!(c2610.status, CycleStatus::Discovered);
    assert!(!rep_unconf.activated.contains(&CycleId("2610".into())));

    // 7. ATTACK: Attempt to rewrite effective history (valid_from <= now) must fail closed
    let mut illegal_history_rewrite = wp_2608.clone();
    illegal_history_rewrite.name = "HACKED FIX".into();
    let res = store.insert_waypoint(&illegal_history_rewrite);
    assert!(
        res.is_err(),
        "same-valid_from rewrite of effective history must be rejected"
    );
}
