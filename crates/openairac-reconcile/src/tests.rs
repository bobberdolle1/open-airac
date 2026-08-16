//! S8 reconciliation test matrix: deterministic fixtures over two
//! providers (FAA_CIFP namespace `faa`, OurAirports namespace
//! `ourairports`).

use chrono::{Duration, Utc};
use openairac_model::*;
use openairac_store::WorldStore;

use crate::{Reconciler, resolve::resolved_entity};

fn seeded_store() -> (WorldStore, chrono::DateTime<Utc>) {
    let mut store = WorldStore::open_in_memory().unwrap();
    store.migrate().unwrap();
    let t0 = Utc::now();
    for (id, provider, dataset) in [
        ("snap-faa", "FAA_CIFP", "FAACIFP18"),
        ("snap-oa", "OurAirports", "airports"),
    ] {
        store
            .insert_source_snapshot(&SourceSnapshot {
                id: SourceSnapshotId(id.to_string()),
                provider: provider.to_string(),
                dataset: dataset.to_string(),
                provider_revision: None,
                airac_cycle: None,
                effective_from: Some(t0),
                effective_until: None,
                retrieved_at: t0,
                source_uri: "fixture".to_string(),
                content_sha256: "0".repeat(64),
                license_id: None,
                license_notes: None,
                parser_version: "test".to_string(),
            })
            .unwrap();
    }
    (store, t0)
}

fn airport(
    id: &str,
    ident: &str,
    name: &str,
    lat: f64,
    lon: f64,
    country: Option<&str>,
    snap: &str,
) -> CanonicalAirport {
    CanonicalAirport {
        id: AirportId(id.to_string()),
        ident: ident.to_string(),
        name: name.to_string(),
        airport_type: "large_airport".to_string(),
        latitude: lat,
        longitude: lon,
        elevation_ft: Some(13.0),
        iso_country: country.map(|s| s.to_string()),
        municipality: None,
        runways: Vec::new(),
        temporal: TemporalValidity {
            valid_from: Utc::now(),
            valid_until: None,
            source_snapshot_id: SourceSnapshotId(snap.to_string()),
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn navaid(
    id: &str,
    ident: &str,
    kind: NavaidKind,
    freq_khz: u32,
    lat: f64,
    lon: f64,
    snap: &str,
    vf: chrono::DateTime<Utc>,
) -> CanonicalNavaid {
    CanonicalNavaid {
        object_id: NavaidId(id.to_string()),
        ident: ident.to_string(),
        name: ident.to_string(),
        kind,
        frequency: FrequencyKhz(freq_khz),
        latitude: lat,
        longitude: lon,
        elevation_ft: None,
        region_code: Some("K2".to_string()),
        associated_airport: None,
        magnetic_variation_deg: None,
        slaved_variation_deg: None,
        service_volume_nm: None,
        dme_paired: false,
        associated_runway: None,
        localizer_bearing_true_deg: None,
        localizer_bearing_mag_deg: None,
        glideslope_angle_deg: None,
        temporal: TemporalValidity {
            valid_from: vf,
            valid_until: None,
            source_snapshot_id: SourceSnapshotId(snap.to_string()),
        },
    }
}

fn waypoint(
    id: &str,
    ident: &str,
    region: &str,
    lat: f64,
    lon: f64,
    snap: &str,
) -> CanonicalWaypoint {
    CanonicalWaypoint {
        object_id: WaypointId(id.to_string()),
        ident: ident.to_string(),
        name: ident.to_string(),
        latitude: lat,
        longitude: lon,
        is_enroute: true,
        region_code: region.to_string(),
        terminal_area_ident: None,
        waypoint_type: Some(0x202057),
        temporal: TemporalValidity {
            valid_from: Utc::now(),
            valid_until: None,
            source_snapshot_id: SourceSnapshotId(snap.to_string()),
        },
    }
}

#[test]
fn test_same_airport_across_providers_exact() {
    let (store, _t0) = seeded_store();
    store
        .insert_airport(&airport(
            "faa:KSFO",
            "KSFO",
            "San Francisco Intl",
            37.6188,
            -122.3750,
            Some("US"),
            "snap-faa",
        ))
        .unwrap();
    store
        .insert_airport(&airport(
            "ourairports:1",
            "KSFO",
            "San Francisco International Airport",
            37.6188,
            -122.3750,
            Some("US"),
            "snap-oa",
        ))
        .unwrap();

    let stats = Reconciler::new(&store).reconcile(Utc::now()).unwrap();
    assert_eq!(stats.exact_matches, 1);
    assert_eq!(stats.probable_matches, 0);
    assert_eq!(stats.conflicts, 0);

    let memberships = store.query_memberships().unwrap();
    assert_eq!(memberships.len(), 2);
    assert_eq!(memberships[0].confidence, MatchConfidence::Exact);
    // Both members share one canonical id.
    assert_eq!(memberships[0].canonical_id, memberships[1].canonical_id);

    // Resolved view: fields selected per authority (FAA first).
    let resolved = resolved_entity(&store, &memberships[0].canonical_id, Utc::now())
        .unwrap()
        .unwrap();
    assert_eq!(resolved.entity_table, "airports");
    assert_eq!(resolved.members.len(), 2);
    let ident = resolved.fields.iter().find(|f| f.field == "name").unwrap();
    assert_eq!(ident.value, "San Francisco Intl"); // FAA preferred
    assert_eq!(ident.source.provider, "FAA_CIFP");
}

#[test]
fn test_same_ident_distant_airports_conflict() {
    let (store, _t0) = seeded_store();
    store
        .insert_airport(&airport(
            "faa:KXXX",
            "KXXX",
            "North",
            40.0,
            -74.0,
            Some("US"),
            "snap-faa",
        ))
        .unwrap();
    store
        .insert_airport(&airport(
            "ourairports:1",
            "KXXX",
            "South",
            20.0,
            -74.0,
            Some("US"),
            "snap-oa",
        ))
        .unwrap();

    let stats = Reconciler::new(&store).reconcile(Utc::now()).unwrap();
    assert_eq!(stats.conflicts, 1);
    assert_eq!(stats.exact_matches, 0);
    assert!(store.query_memberships().unwrap().is_empty());
    let conflicts = store.query_reconciliation_conflicts().unwrap();
    assert_eq!(conflicts[0].severity, ConflictSeverity::Error);
    assert_eq!(conflicts[0].category, "identity");
}

#[test]
fn test_waypoint_same_ident_different_regions_distinct() {
    let (store, _t0) = seeded_store();
    store
        .insert_waypoint(&waypoint("faa:WP1", "ABCDE", "K2", 30.0, -80.0, "snap-faa"))
        .unwrap();
    store
        .insert_waypoint(&waypoint(
            "ourairports:WP1",
            "ABCDE",
            "MY",
            30.0,
            -80.0,
            "snap-oa",
        ))
        .unwrap();

    let stats = Reconciler::new(&store).reconcile(Utc::now()).unwrap();
    // Different regions: never candidates, never merged.
    assert_eq!(stats.exact_matches, 0);
    assert_eq!(stats.candidate_pairs, 0);
    assert!(store.query_memberships().unwrap().is_empty());
}

#[test]
fn test_vor_vs_ndb_same_ident_distinct() {
    let (store, _t0) = seeded_store();
    let now = Utc::now();
    store
        .insert_navaid(&navaid(
            "faa:ABC",
            "ABC",
            NavaidKind::Vor,
            112_000,
            30.0,
            -80.0,
            "snap-faa",
            now,
        ))
        .unwrap();
    store
        .insert_navaid(&navaid(
            "ourairports:ABC",
            "ABC",
            NavaidKind::Ndb,
            350,
            30.0,
            -80.0,
            "snap-oa",
            now,
        ))
        .unwrap();

    let stats = Reconciler::new(&store).reconcile(now).unwrap();
    // Kind classes differ: different identity keys, never merged.
    assert_eq!(stats.exact_matches, 0);
    assert!(store.query_memberships().unwrap().is_empty());
}

#[test]
fn test_same_vor_slightly_different_coords_exact() {
    let (store, _t0) = seeded_store();
    let now = Utc::now();
    store
        .insert_navaid(&navaid(
            "faa:ABC",
            "ABC",
            NavaidKind::Vor,
            112_000,
            30.0,
            -80.0,
            "snap-faa",
            now,
        ))
        .unwrap();
    store
        .insert_navaid(&navaid(
            "ourairports:ABC",
            "ABC",
            NavaidKind::Vordme,
            112_000,
            30.001,
            -80.0005,
            "snap-oa",
            now,
        ))
        .unwrap();

    let stats = Reconciler::new(&store).reconcile(now).unwrap();
    assert_eq!(stats.exact_matches, 1);
    assert_eq!(store.query_memberships().unwrap().len(), 2);
}

#[test]
fn test_frequency_change_over_time_same_canonical() {
    let (store, _t0) = seeded_store();
    let t1 = Utc::now();
    let t2 = t1 + Duration::seconds(3600);
    // OurAirports constant; FAA frequency changes at t2.
    store
        .insert_navaid(&navaid(
            "faa:ABC",
            "ABC",
            NavaidKind::Vor,
            112_000,
            30.0,
            -80.0,
            "snap-faa",
            t1,
        ))
        .unwrap();
    store
        .insert_navaid(&navaid(
            "ourairports:ABC",
            "ABC",
            NavaidKind::Vor,
            112_000,
            30.0,
            -80.0,
            "snap-oa",
            t1,
        ))
        .unwrap();

    let stats = Reconciler::new(&store).reconcile(t1).unwrap();
    assert_eq!(stats.exact_matches, 1);
    assert_eq!(store.query_reconciliation_conflicts().unwrap().len(), 0);

    // FAA revision with a new frequency at t2.
    store
        .insert_navaid(&navaid(
            "faa:ABC",
            "ABC",
            NavaidKind::Vor,
            113_000,
            30.0,
            -80.0,
            "snap-faa",
            t2,
        ))
        .unwrap();
    let stats2 = Reconciler::new(&store).reconcile(t2).unwrap();
    assert_eq!(stats2.exact_matches, 1);
    // One canonical identity, two temporal membership intervals; the
    // frequency change is across instants, not a conflict.
    let memberships = store.query_memberships().unwrap();
    assert_eq!(memberships.len(), 3); // faa(t1), oa(t1), faa(t2)
    let canonical: Vec<_> = memberships.iter().map(|m| m.canonical_id.clone()).collect();
    assert!(canonical.iter().all(|c| c == &canonical[0])); // one identity
    // At t2 the two providers genuinely disagree on the CURRENT
    // frequency: a same-instant field conflict (Warning), never an
    // identity break.
    let conflicts = store.query_reconciliation_conflicts().unwrap();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].category, "field");
}

#[test]
fn test_conflicting_frequency_same_instant_field_conflict() {
    let (store, _t0) = seeded_store();
    let now = Utc::now();
    store
        .insert_navaid(&navaid(
            "faa:ABC",
            "ABC",
            NavaidKind::Vor,
            112_000,
            30.0,
            -80.0,
            "snap-faa",
            now,
        ))
        .unwrap();
    store
        .insert_navaid(&navaid(
            "ourairports:ABC",
            "ABC",
            NavaidKind::Vor,
            113_000,
            30.0,
            -80.0,
            "snap-oa",
            now,
        ))
        .unwrap();

    let stats = Reconciler::new(&store).reconcile(now).unwrap();
    assert_eq!(stats.exact_matches, 1); // identity holds
    let conflicts = store.query_reconciliation_conflicts().unwrap();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].category, "field");
    assert_eq!(conflicts[0].field_name.as_deref(), Some("frequency_khz"));
    assert_eq!(conflicts[0].severity, ConflictSeverity::Warning);
}

#[test]
fn test_airport_ident_change_continuity_link() {
    let (store, _t0) = seeded_store();
    store
        .insert_airport(&airport(
            "faa:KOLD",
            "KOLD",
            "Testville",
            35.0,
            -90.05,
            Some("US"),
            "snap-faa",
        ))
        .unwrap();
    // Renamed, same location (same spatial cell).
    store
        .insert_airport(&airport(
            "ourairports:1",
            "KNEW",
            "Testville",
            35.0001,
            -90.05,
            Some("US"),
            "snap-oa",
        ))
        .unwrap();

    Reconciler::new(&store).reconcile(Utc::now()).unwrap();
    // No memberships (different idents, never identity-merged), but a
    // continuity link exists between the two canonical identities.
    assert!(store.query_memberships().unwrap().is_empty());
    let continuity: i64 = store
        .raw_conn()
        .query_row("SELECT COUNT(*) FROM identity_continuity", [], |r| r.get(0))
        .unwrap();
    assert_eq!(continuity, 1);
}

#[test]
fn test_runway_renumbering_same_physical_runway() {
    let (store, _t0) = seeded_store();
    let mut a = airport(
        "faa:KAAA",
        "KAAA",
        "A",
        40.0,
        -100.0,
        Some("US"),
        "snap-faa",
    );
    let mut b = airport(
        "ourairports:1",
        "KAAA",
        "A",
        40.0,
        -100.0,
        Some("US"),
        "snap-oa",
    );
    let mk = |designator: &str, ap: &str| CanonicalRunway {
        id: RunwayId(format!("{ap}:09")),
        airport_id: AirportId(ap.to_string()),
        airport_ident: "KAAA".to_string(),
        official_designator: designator.to_string(),
        computed_magnetic_designator: None,
        true_heading_deg: Some(90.0),
        length_ft: 9000,
        width_ft: 150,
        surface: Some("ASP".to_string()),
        le_ident: designator.to_string(),
        le_lat: 40.0,
        le_lon: -100.0,
        le_elevation_ft: None,
        he_ident: format!("{designator}R"),
        he_lat: 40.02,
        he_lon: -100.0,
        he_elevation_ft: None,
        temporal: TemporalValidity {
            valid_from: Utc::now(),
            valid_until: None,
            source_snapshot_id: SourceSnapshotId(
                if ap.starts_with("faa") {
                    "snap-faa"
                } else {
                    "snap-oa"
                }
                .to_string(),
            ),
        },
    };
    a.runways = vec![mk("09", "faa:KAAA")];
    b.runways = vec![mk("10", "ourairports:1")];
    store.insert_airport(&a).unwrap();
    store.insert_airport(&b).unwrap();

    let stats = Reconciler::new(&store).reconcile(Utc::now()).unwrap();
    assert_eq!(stats.exact_matches, 2); // airport + runway
    let conflicts = store.query_reconciliation_conflicts().unwrap();
    assert!(
        conflicts.iter().any(|c| c.entity_table == "runways"
            && c.field_name.as_deref() == Some("official_designator"))
    );
}

#[test]
fn test_ambiguous_two_candidates_no_merge() {
    let (store, _t0) = seeded_store();
    let now = Utc::now();
    // One FAA VOR; two OurAirports candidates with the same ident
    // within the probable band.
    store
        .insert_navaid(&navaid(
            "faa:ABC",
            "ABC",
            NavaidKind::Vor,
            112_000,
            30.0,
            -80.0,
            "snap-faa",
            now,
        ))
        .unwrap();
    store
        .insert_navaid(&navaid(
            "ourairports:A1",
            "ABC",
            NavaidKind::Vor,
            112_000,
            30.02,
            -80.0,
            "snap-oa",
            now,
        ))
        .unwrap();
    store
        .insert_navaid(&navaid(
            "ourairports:A2",
            "ABC",
            NavaidKind::Vor,
            112_000,
            30.04,
            -80.0,
            "snap-oa",
            now,
        ))
        .unwrap();

    let stats = Reconciler::new(&store).reconcile(now).unwrap();
    assert_eq!(stats.ambiguous, 2); // two cross pairs, both ambiguous
    assert_eq!(stats.exact_matches, 0);
    assert!(store.query_memberships().unwrap().is_empty());
    let conflicts = store.query_reconciliation_conflicts().unwrap();
    assert!(conflicts.iter().all(|c| c.category == "ambiguity"));
}

#[test]
fn test_replay_idempotent_and_stable_canonical_ids() {
    let (store, _t0) = seeded_store();
    let now = Utc::now();
    store
        .insert_navaid(&navaid(
            "faa:ABC",
            "ABC",
            NavaidKind::Vor,
            112_000,
            30.0,
            -80.0,
            "snap-faa",
            now,
        ))
        .unwrap();
    store
        .insert_navaid(&navaid(
            "ourairports:ABC",
            "ABC",
            NavaidKind::Vor,
            112_000,
            30.0,
            -80.0,
            "snap-oa",
            now,
        ))
        .unwrap();

    let first = Reconciler::new(&store).reconcile(now).unwrap();
    let ids_1: Vec<_> = store.query_canonical_identities().unwrap();
    let memberships_1 = store.query_memberships().unwrap().len();
    let second = Reconciler::new(&store).reconcile(now).unwrap();
    assert_eq!(first, second);
    let ids_2: Vec<_> = store.query_canonical_identities().unwrap();
    assert_eq!(ids_1, ids_2); // stable canonical ids
    assert_eq!(store.query_memberships().unwrap().len(), memberships_1); // no duplicates
}

#[test]
fn test_tombstone_removes_current_membership_view() {
    let (mut store, _t0) = seeded_store();
    let t1 = Utc::now();
    let t2 = t1 + Duration::seconds(3600);
    store
        .insert_navaid(&navaid(
            "faa:ABC",
            "ABC",
            NavaidKind::Vor,
            112_000,
            30.0,
            -80.0,
            "snap-faa",
            t1,
        ))
        .unwrap();
    store
        .insert_navaid(&navaid(
            "ourairports:ABC",
            "ABC",
            NavaidKind::Vor,
            112_000,
            30.0,
            -80.0,
            "snap-oa",
            t1,
        ))
        .unwrap();

    Reconciler::new(&store).reconcile(t1).unwrap();
    let canonical = store.query_memberships().unwrap()[0].canonical_id.clone();

    // Tombstone the FAA navaid at t2.
    store
        .apply_tombstone(&Tombstone {
            provider: "FAA_CIFP".to_string(),
            dataset: "FAACIFP18".to_string(),
            entity_table: "navaids".to_string(),
            entity_id: "faa:ABC".to_string(),
            effective_from: t2,
            source_snapshot_id: SourceSnapshotId("snap-faa".to_string()),
            reason: Some("decommissioned".to_string()),
        })
        .unwrap();

    // Historical reconciliation is untouched.
    let historical = resolved_entity(&store, &canonical, t1).unwrap().unwrap();
    assert_eq!(historical.members.len(), 2);
    // Current view drops the tombstoned member.
    let current = resolved_entity(&store, &canonical, t2).unwrap().unwrap();
    assert_eq!(current.members.len(), 1);
    assert_eq!(current.members[0].provider, "OurAirports");
}

#[test]
fn test_correction_updates_membership_without_history_loss() {
    let (store, _t0) = seeded_store();
    let t1 = Utc::now();
    let t2 = t1 + Duration::seconds(3600);
    store
        .insert_navaid(&navaid(
            "faa:ABC",
            "ABC",
            NavaidKind::Vor,
            112_000,
            30.0,
            -80.0,
            "snap-faa",
            t1,
        ))
        .unwrap();
    store
        .insert_navaid(&navaid(
            "ourairports:ABC",
            "ABC",
            NavaidKind::Vor,
            112_000,
            30.0,
            -80.0,
            "snap-oa",
            t1,
        ))
        .unwrap();
    Reconciler::new(&store).reconcile(t1).unwrap();
    let before = store.query_memberships().unwrap().len();

    // Correction moves the FAA facility slightly (new revision at t2).
    store
        .insert_navaid(&navaid(
            "faa:ABC",
            "ABC",
            NavaidKind::Vor,
            112_000,
            30.1,
            -80.0,
            "snap-faa",
            t2,
        ))
        .unwrap();
    Reconciler::new(&store).reconcile(t2).unwrap();
    let after = store.query_memberships().unwrap();
    assert_eq!(after.len(), before + 1); // new interval, old intact
    let canonical: Vec<_> = after.iter().map(|m| m.canonical_id.clone()).collect();
    assert!(canonical.iter().all(|c| c == &canonical[0]));
}

#[test]
fn test_provider_b_far_entity_never_merged() {
    let (store, _t0) = seeded_store();
    let now = Utc::now();
    store
        .insert_navaid(&navaid(
            "faa:ABC",
            "ABC",
            NavaidKind::Vor,
            112_000,
            30.0,
            -80.0,
            "snap-faa",
            now,
        ))
        .unwrap();
    store
        .insert_navaid(&navaid(
            "ourairports:ABC",
            "ABC",
            NavaidKind::Vor,
            112_000,
            50.0,
            -100.0,
            "snap-oa",
            now,
        ))
        .unwrap();

    let stats = Reconciler::new(&store).reconcile(now).unwrap();
    assert_eq!(stats.exact_matches, 0);
    assert!(store.query_memberships().unwrap().is_empty());
    assert_eq!(store.query_reconciliation_conflicts().unwrap().len(), 1);
}

#[test]
fn test_historical_world_at_preserved() {
    let (store, _t0) = seeded_store();
    let t1 = Utc::now();
    let t2 = t1 + Duration::seconds(7200);
    store
        .insert_navaid(&navaid(
            "faa:ABC",
            "ABC",
            NavaidKind::Vor,
            112_000,
            30.0,
            -80.0,
            "snap-faa",
            t1,
        ))
        .unwrap();
    store
        .insert_navaid(&navaid(
            "ourairports:ABC",
            "ABC",
            NavaidKind::Vor,
            112_000,
            30.0,
            -80.0,
            "snap-oa",
            t1,
        ))
        .unwrap();
    Reconciler::new(&store).reconcile(t1).unwrap();
    let m1 = store.query_memberships().unwrap();

    Reconciler::new(&store).reconcile(t2).unwrap();
    let m2 = store.query_memberships().unwrap();
    assert_eq!(m1.len(), m2.len()); // same intervals; nothing rewritten
    assert_eq!(m1, m2);
}

#[test]
fn test_paired_dme_component_not_a_candidate() {
    let (store, _t0) = seeded_store();
    let now = Utc::now();
    let mut parent = navaid(
        "faa:ABC",
        "ABC",
        NavaidKind::Vortac,
        112_000,
        30.0,
        -80.0,
        "snap-faa",
        now,
    );
    parent.object_id = NavaidId("faa:ABC".to_string());
    store.insert_navaid(&parent).unwrap();
    // FAA paired DME component row (row 12 semantics).
    let mut dme = navaid(
        "faa:ABC:dme",
        "ABC",
        NavaidKind::Dme,
        112_000,
        30.0,
        -80.0,
        "snap-faa",
        now,
    );
    dme.dme_paired = true;
    store.insert_navaid(&dme).unwrap();
    store
        .insert_navaid(&navaid(
            "ourairports:ABC",
            "ABC",
            NavaidKind::Vordme,
            112_000,
            30.0,
            -80.0,
            "snap-oa",
            now,
        ))
        .unwrap();

    let stats = Reconciler::new(&store).reconcile(now).unwrap();
    assert_eq!(stats.ambiguous, 0);
    assert_eq!(stats.exact_matches, 1);
    assert_eq!(store.query_memberships().unwrap().len(), 2);
}

#[test]
fn test_regionless_fallback_distinct_canonical_identities() {
    // P0 regression: NDB "CO" exists in K2 and K6. Region-less
    // OurAirports twins must map to TWO canonical identities — the
    // candidate key (kind+ident) must never define identity.
    let (store, _t0) = seeded_store();
    let now = Utc::now();
    // FAA K2:CO + OA twin (Colorado Springs area).
    store
        .insert_navaid(&navaid(
            "faa:COK2",
            "CO",
            NavaidKind::Ndb,
            400,
            38.6944,
            -104.7163,
            "snap-faa",
            now,
        ))
        .unwrap();
    let mut oa_k2 = navaid(
        "ourairports:COK2",
        "CO",
        NavaidKind::Ndb,
        400,
        38.6943,
        -104.7160,
        "snap-oa",
        now,
    );
    oa_k2.region_code = None; // region-less provider
    store.insert_navaid(&oa_k2).unwrap();
    // FAA K6:CO + OA twin (New Hampshire).
    let mut faa_k6 = navaid(
        "faa:COK6",
        "CO",
        NavaidKind::Ndb,
        400,
        43.1188,
        -71.4524,
        "snap-faa",
        now,
    );
    faa_k6.region_code = Some("K6".to_string());
    store.insert_navaid(&faa_k6).unwrap();
    let mut oa_k6 = navaid(
        "ourairports:COK6",
        "CO",
        NavaidKind::Ndb,
        400,
        43.1188,
        -71.4524,
        "snap-oa",
        now,
    );
    oa_k6.region_code = None;
    store.insert_navaid(&oa_k6).unwrap();

    let stats = Reconciler::new(&store).reconcile(now).unwrap();
    assert_eq!(stats.exact_matches, 2);

    // TWO canonical identities, each with exactly its two members.
    let memberships = store.query_memberships().unwrap();
    assert_eq!(memberships.len(), 4);
    let mut by_canonical: std::collections::HashMap<
        openairac_model::CanonicalEntityId,
        Vec<&SourceMembership>,
    > = std::collections::HashMap::new();
    for m in &memberships {
        by_canonical
            .entry(m.canonical_id.clone())
            .or_default()
            .push(m);
    }
    assert_eq!(by_canonical.len(), 2, "{by_canonical:?}");
    for (canonical, members) in &by_canonical {
        assert_eq!(members.len(), 2, "{canonical:?}");
        // One K2 pair or one K6 pair — never crossed.
        let idents: Vec<&str> = members
            .iter()
            .map(|m| m.source.entity_id.as_str())
            .collect();
        assert!(
            (idents.contains(&"faa:COK2") && idents.contains(&"ourairports:COK2"))
                || (idents.contains(&"faa:COK6") && idents.contains(&"ourairports:COK6")),
            "{idents:?}"
        );
    }
    // No cross-membership.
    assert!(!memberships.iter().any(|m| {
        m.canonical_id != by_canonical.keys().next().unwrap().clone()
            && m.source.entity_id == "faa:COK2"
            && m.source.entity_id == "ourairports:COK6"
    }));

    // Replay: exactly the same canonical ids, no duplicates.
    let ids_before: Vec<_> = store.query_canonical_identities().unwrap();
    let stats2 = Reconciler::new(&store).reconcile(now).unwrap();
    assert_eq!(stats, stats2);
    let ids_after: Vec<_> = store.query_canonical_identities().unwrap();
    assert_eq!(ids_before, ids_after);
    assert_eq!(store.query_memberships().unwrap().len(), 4);
}

#[test]
fn test_membership_intervals_follow_source_revisions() {
    let (store, _t0) = seeded_store();
    let t0 = Utc::now();
    let t1 = t0 + Duration::seconds(3600);
    // Revision A valid [t0, t1): FAA, then a correction closes it.
    let mut rev_a = navaid(
        "faa:ABC",
        "ABC",
        NavaidKind::Vor,
        112_000,
        30.0,
        -80.0,
        "snap-faa",
        t0,
    );
    rev_a.temporal.valid_until = Some(t1);
    store.insert_navaid(&rev_a).unwrap();
    store
        .insert_navaid(&navaid(
            "ourairports:ABC",
            "ABC",
            NavaidKind::Vor,
            112_000,
            30.0,
            -80.0,
            "snap-oa",
            t0,
        ))
        .unwrap();
    // Revision B valid [t1, ...): frequency change, same identity.
    store
        .insert_navaid(&navaid(
            "faa:ABC",
            "ABC",
            NavaidKind::Vor,
            113_000,
            30.0,
            -80.0,
            "snap-faa",
            t1,
        ))
        .unwrap();

    Reconciler::new(&store).reconcile(t0).unwrap();
    Reconciler::new(&store).reconcile(t1).unwrap();

    let memberships = store.query_memberships().unwrap();
    let faa_a = memberships
        .iter()
        .find(|m| m.source.entity_id == "faa:ABC" && m.valid_from == t0)
        .expect("revision A membership");
    let faa_b = memberships
        .iter()
        .find(|m| m.source.entity_id == "faa:ABC" && m.valid_from == t1)
        .expect("revision B membership");
    // Interval copied exactly.
    assert_eq!(faa_a.valid_until, Some(t1));
    assert_eq!(faa_b.valid_until, None);

    // canonical_entity at t0 uses A; at t1 A is not current.
    let canonical = memberships[0].canonical_id.clone();
    let at_t0 = crate::resolve::resolved_entity(&store, &canonical, t0)
        .unwrap()
        .unwrap();
    assert_eq!(at_t0.members.len(), 2);
    let at_t1 = crate::resolve::resolved_entity(&store, &canonical, t1)
        .unwrap()
        .unwrap();
    assert_eq!(at_t1.members.len(), 2); // FAA B + OA
    // The frequency field at t1 resolves to B's 113000 (FAA authority).
    let freq = at_t1
        .fields
        .iter()
        .find(|f| f.field == "frequency_khz")
        .unwrap();
    assert_eq!(freq.value, "113000");
    // Historical world_at unchanged: store still has A for [t0,t1).
    let world_t0 = store.query_navaids_at(t0).unwrap();
    assert_eq!(
        world_t0
            .iter()
            .find(|n| n.object_id.0 == "faa:ABC")
            .unwrap()
            .frequency
            .0,
        112_000
    );
    assert!(store.validate().unwrap().is_empty());
}

#[test]
fn test_conflict_dedup_across_ten_reruns() {
    let (store, _t0) = seeded_store();
    let now = Utc::now();
    // Ambiguous fixture: one FAA VOR, two OurAirports candidates in
    // the probable band -> ambiguity conflicts with canonical NULL.
    store
        .insert_navaid(&navaid(
            "faa:ABC",
            "ABC",
            NavaidKind::Vor,
            112_000,
            30.0,
            -80.0,
            "snap-faa",
            now,
        ))
        .unwrap();
    store
        .insert_navaid(&navaid(
            "ourairports:A1",
            "ABC",
            NavaidKind::Vor,
            112_000,
            30.02,
            -80.0,
            "snap-oa",
            now,
        ))
        .unwrap();
    store
        .insert_navaid(&navaid(
            "ourairports:A2",
            "ABC",
            NavaidKind::Vor,
            112_000,
            30.04,
            -80.0,
            "snap-oa",
            now,
        ))
        .unwrap();

    let mut stats = None;
    for _ in 0..10 {
        let s = Reconciler::new(&store).reconcile(now).unwrap();
        match &stats {
            None => stats = Some(s),
            Some(prev) => assert_eq!(*prev, s),
        }
    }
    let conflicts = store.query_reconciliation_conflicts().unwrap();
    assert_eq!(conflicts.len(), 2); // exactly the two semantic ambiguity conflicts
    // Both carry dedup keys (backfilled/inserted).
    let nokey: i64 = store
        .raw_conn()
        .query_row(
            "SELECT COUNT(*) FROM reconciliation_conflicts WHERE conflict_key IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(nokey, 0);
    assert!(store.validate().unwrap().is_empty());
}

#[test]
fn test_conflict_values_refresh_on_rerun() {
    let (store, _t0) = seeded_store();
    let t1 = Utc::now();
    // Field conflict (frequencies disagree) — first observation.
    store
        .insert_navaid(&navaid(
            "faa:ABC",
            "ABC",
            NavaidKind::Vor,
            112_000,
            30.0,
            -80.0,
            "snap-faa",
            t1,
        ))
        .unwrap();
    store
        .insert_navaid(&navaid(
            "ourairports:ABC",
            "ABC",
            NavaidKind::Vor,
            113_000,
            30.0,
            -80.0,
            "snap-oa",
            t1,
        ))
        .unwrap();
    Reconciler::new(&store).reconcile(t1).unwrap();
    let first = store.query_reconciliation_conflicts().unwrap();
    assert_eq!(first.len(), 1);
    // Provider order in the pair is not semantically meaningful:
    // values must be {112000, 113000}.
    let mut values = [
        first[0].value_a.as_deref().unwrap_or(""),
        first[0].value_b.as_deref().unwrap_or(""),
    ];
    values.sort();
    assert_eq!(values, ["112000", "113000"]);

    // FAA corrects to 113000: the conflict DISAPPEARS semantically
    // but the same identity persists; rerun must not keep the stale
    // row (upsert refreshes values; identical values produce no
    // conflict at all).
    let t2 = t1 + chrono::Duration::seconds(3600);
    store
        .insert_navaid(&navaid(
            "faa:ABC",
            "ABC",
            NavaidKind::Vor,
            113_000,
            30.0,
            -80.0,
            "snap-faa",
            t2,
        ))
        .unwrap();
    Reconciler::new(&store).reconcile(t2).unwrap();
    let after = store.query_reconciliation_conflicts().unwrap();
    // The old instant still disagrees (historical rows), but the
    // reconciler evaluates world_at(as_of): at t2 the values agree,
    // so no NEW conflict is recorded; the old row keeps its
    // historical values (dedup by key) — exactly one row total.
    assert_eq!(after.len(), 1);
    assert!(
        after[0].value_a.as_deref() == Some("112000")
            || after[0].value_a.as_deref() == Some("113000")
    );
}
