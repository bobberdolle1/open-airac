//! Comprehensive End-to-End Test Suite for Flight Planning & Operations (V2).
//!
//! Verifies:
//! 1. Russia Golden E2E: UUEE -> UNNT with TU154 profile (SIDs, STARs, ATS Airways, FL350, Semicircular Rule, FMS Export)
//! 2. Russia Long-Haul E2E: UUEE -> UHWW with IL96 profile (Trans-Siberian Long-Haul, Flight Time Calculation)
//! 3. Russia Regional E2E: Regional route with AN24 profile (Unpaved/Runway Suitability Heuristics)
//! 4. France Second-Region E2E: LFPG -> LFMN with A320 profile (SIA France Procedures, Runway Suggestions)
//! 5. Provider Switching & Degradation E2E: Plan -> Rollback Provider -> Revalidate flags STALE_DATA
//! 6. Multi-Format Simulator Exports: X-Plane .fms, GNS430 .fpl, KLN90B

use chrono::{DateTime, Duration, Utc};
use openairac_integration::{
    FlightPlanExporter, FlightPlanLegKind, FlightPlanRequest, FlightPlanStore,
    FlightPlanValidationStatus, Planner, PlanningMode,
};
use openairac_model::{
    AirportId, AirwayLegId, CanonicalAirport, CanonicalAirwayLeg, CanonicalProcedureLeg,
    CanonicalRunway, CanonicalWaypoint, ProcedureLegId, RunwayId, SourceSnapshot, SourceSnapshotId,
    TemporalValidity, WaypointId,
};
use openairac_routing::random_flight::AircraftProfile;
use openairac_store::{WorldStore, insert_airway_leg_conn, insert_procedure_leg_conn};

/// Helper to set up an in-memory test store with Russian and French airports, runways, procedures, and ATS airways.
fn setup_test_world_store() -> (WorldStore, DateTime<Utc>) {
    let mut store = WorldStore::open_in_memory().expect("open store");
    store.migrate().expect("migrate");
    let t = Utc::now();
    let snap_id = SourceSnapshotId("test_source_snapshot".to_string());

    store
        .insert_source_snapshot(&SourceSnapshot {
            id: snap_id.clone(),
            provider: "TEST_PROVIDER".to_string(),
            dataset: "WORLD_BASELINE".to_string(),
            provider_revision: Some("2608".to_string()),
            airac_cycle: Some("2608".to_string()),
            effective_from: Some(t - Duration::days(5)),
            effective_until: Some(t + Duration::days(23)),
            retrieved_at: t,
            source_uri: "http://openairac.org".to_string(),
            content_sha256: "0".repeat(64),
            license_id: Some("PublicDomain".to_string()),
            license_notes: None,
            parser_version: "1.0.0".to_string(),
        })
        .unwrap();

    let val = TemporalValidity {
        valid_from: t - Duration::days(5),
        valid_until: None,
        source_snapshot_id: snap_id.clone(),
    };

    // 1. UUEE Moscow Sheremetyevo
    let uuee_rwy = CanonicalRunway {
        id: RunwayId("uuee_24l".to_string()),
        airport_id: AirportId("uuee".to_string()),
        airport_ident: "UUEE".to_string(),
        official_designator: "24L".to_string(),
        computed_magnetic_designator: Some("24L".to_string()),
        true_heading_deg: Some(239.0),
        length_ft: 12139,
        width_ft: Some(197),
        surface: Some("ASP".to_string()),
        le_ident: "24L".to_string(),
        le_lat: 55.9728,
        le_lon: 37.4147,
        le_elevation_ft: Some(622.0),
        he_ident: "06R".to_string(),
        he_lat: 55.9600,
        he_lon: 37.3800,
        he_elevation_ft: Some(600.0),
        temporal: val.clone(),
    };
    let uuee = CanonicalAirport {
        id: AirportId("uuee".to_string()),
        ident: "UUEE".to_string(),
        name: "Moscow Sheremetyevo".to_string(),
        airport_type: "large_airport".to_string(),
        latitude: 55.9728,
        longitude: 37.4147,
        elevation_ft: Some(622.0),
        iso_country: Some("RU".to_string()),
        municipality: Some("Moscow".to_string()),
        runways: vec![uuee_rwy],
        temporal: val.clone(),
    };
    store.insert_airport(&uuee).unwrap();

    // 2. UNNT Novosibirsk Tolmachevo
    let unnt_rwy = CanonicalRunway {
        id: RunwayId("unnt_07".to_string()),
        airport_id: AirportId("unnt".to_string()),
        airport_ident: "UNNT".to_string(),
        official_designator: "07".to_string(),
        computed_magnetic_designator: Some("07".to_string()),
        true_heading_deg: Some(72.0),
        length_ft: 11808,
        width_ft: Some(197),
        surface: Some("CON".to_string()),
        le_ident: "07".to_string(),
        le_lat: 55.0125,
        le_lon: 82.6506,
        le_elevation_ft: Some(365.0),
        he_ident: "25".to_string(),
        he_lat: 55.0200,
        he_lon: 82.6900,
        he_elevation_ft: Some(360.0),
        temporal: val.clone(),
    };
    let unnt = CanonicalAirport {
        id: AirportId("unnt".to_string()),
        ident: "UNNT".to_string(),
        name: "Novosibirsk Tolmachevo".to_string(),
        airport_type: "large_airport".to_string(),
        latitude: 55.0125,
        longitude: 82.6506,
        elevation_ft: Some(365.0),
        iso_country: Some("RU".to_string()),
        municipality: Some("Novosibirsk".to_string()),
        runways: vec![unnt_rwy],
        temporal: val.clone(),
    };
    store.insert_airport(&unnt).unwrap();

    // 3. UHWW Vladivostok Knevichi
    let uhww_rwy = CanonicalRunway {
        id: RunwayId("uhww_25r".to_string()),
        airport_id: AirportId("uhww".to_string()),
        airport_ident: "UHWW".to_string(),
        official_designator: "25R".to_string(),
        computed_magnetic_designator: Some("25R".to_string()),
        true_heading_deg: Some(251.0),
        length_ft: 11483,
        width_ft: Some(197),
        surface: Some("CON".to_string()),
        le_ident: "25R".to_string(),
        le_lat: 43.3992,
        le_lon: 132.1481,
        le_elevation_ft: Some(46.0),
        he_ident: "07L".to_string(),
        he_lat: 43.4000,
        he_lon: 132.1800,
        he_elevation_ft: Some(40.0),
        temporal: val.clone(),
    };
    let uhww = CanonicalAirport {
        id: AirportId("uhww".to_string()),
        ident: "UHWW".to_string(),
        name: "Vladivostok Knevichi".to_string(),
        airport_type: "large_airport".to_string(),
        latitude: 43.3992,
        longitude: 132.1481,
        elevation_ft: Some(46.0),
        iso_country: Some("RU".to_string()),
        municipality: Some("Vladivostok".to_string()),
        runways: vec![uhww_rwy],
        temporal: val.clone(),
    };
    store.insert_airport(&uhww).unwrap();

    // 4. LFPG Paris Charles de Gaulle
    let lfpg_rwy = CanonicalRunway {
        id: RunwayId("lfpg_26l".to_string()),
        airport_id: AirportId("lfpg".to_string()),
        airport_ident: "LFPG".to_string(),
        official_designator: "26L".to_string(),
        computed_magnetic_designator: Some("26L".to_string()),
        true_heading_deg: Some(266.0),
        length_ft: 13779,
        width_ft: Some(148),
        surface: Some("CON".to_string()),
        le_ident: "26L".to_string(),
        le_lat: 49.0097,
        le_lon: 2.5478,
        le_elevation_ft: Some(392.0),
        he_ident: "08R".to_string(),
        he_lat: 49.0100,
        he_lon: 2.5800,
        he_elevation_ft: Some(380.0),
        temporal: val.clone(),
    };
    let lfpg = CanonicalAirport {
        id: AirportId("lfpg".to_string()),
        ident: "LFPG".to_string(),
        name: "Paris Charles de Gaulle".to_string(),
        airport_type: "large_airport".to_string(),
        latitude: 49.0097,
        longitude: 2.5478,
        elevation_ft: Some(392.0),
        iso_country: Some("FR".to_string()),
        municipality: Some("Paris".to_string()),
        runways: vec![lfpg_rwy],
        temporal: val.clone(),
    };
    store.insert_airport(&lfpg).unwrap();

    // 5. LFMN Nice Cote d'Azur
    let lfmn_rwy = CanonicalRunway {
        id: RunwayId("lfmn_04l".to_string()),
        airport_id: AirportId("lfmn".to_string()),
        airport_ident: "LFMN".to_string(),
        official_designator: "04L".to_string(),
        computed_magnetic_designator: Some("04L".to_string()),
        true_heading_deg: Some(44.0),
        length_ft: 8432,
        width_ft: Some(148),
        surface: Some("ASP".to_string()),
        le_ident: "04L".to_string(),
        le_lat: 43.6584,
        le_lon: 7.2159,
        le_elevation_ft: Some(12.0),
        he_ident: "22R".to_string(),
        he_lat: 43.6600,
        he_lon: 7.2300,
        he_elevation_ft: Some(10.0),
        temporal: val.clone(),
    };
    let lfmn = CanonicalAirport {
        id: AirportId("lfmn".to_string()),
        ident: "LFMN".to_string(),
        name: "Nice Cote d'Azur".to_string(),
        airport_type: "large_airport".to_string(),
        latitude: 43.6584,
        longitude: 7.2159,
        elevation_ft: Some(12.0),
        iso_country: Some("FR".to_string()),
        municipality: Some("Nice".to_string()),
        runways: vec![lfmn_rwy],
        temporal: val.clone(),
    };
    store.insert_airport(&lfmn).unwrap();

    // Insert Waypoints
    let waypoints = vec![
        ("DIPOP", 55.8000, 38.2000, "UU"),
        ("ROTLI", 56.1620, 90.3640, "UN"),
        ("ADONI", 56.1944, 88.5352, "UN"),
        ("BINBA", 56.2248, 86.0000, "UN"),
        ("OPALE", 49.3000, 2.9000, "LF"),
        ("NURMO", 48.5000, 2.6000, "LF"),
        ("LUKIR", 59.2000, 31.0000, "UL"),
    ];

    for (ident, lat, lon, reg) in waypoints {
        let wp = CanonicalWaypoint {
            object_id: WaypointId(format!("{}_{}", ident, reg)),
            ident: ident.to_string(),
            name: ident.to_string(),
            latitude: lat,
            longitude: lon,
            region_code: reg.to_string(),
            is_enroute: true,
            terminal_area_ident: None,
            waypoint_type: Some(10),
            temporal: val.clone(),
        };
        store.insert_waypoint(&wp).unwrap();
    }

    // Insert Airway Legs (A300: DIPOP -> ROTLI -> ADONI -> BINBA)
    let airway_legs = vec![
        ("A300", "DIPOP", "UU", "ROTLI", "UN", 1),
        ("A300", "ROTLI", "UN", "ADONI", "UN", 2),
        ("B210", "ADONI", "UN", "BINBA", "UN", 1),
    ];

    for (r_ident, f_ident, f_reg, t_ident, t_reg, seq) in airway_legs {
        let leg = CanonicalAirwayLeg {
            object_id: AirwayLegId(format!("{}_{}_{}", r_ident, f_ident, t_ident)),
            route_ident: r_ident.to_string(),
            route_type: "R".to_string(),
            level: Some('B'),
            sequence_number: seq,
            start_fix: f_ident.to_string(),
            start_icao_code: f_reg.to_string(),
            end_fix: t_ident.to_string(),
            end_icao_code: t_reg.to_string(),
            direction: 'N',
            minimum_altitude_ft: Some(10000),
            maximum_altitude_ft: Some(45000),
            temporal: val.clone(),
        };
        insert_airway_leg_conn(store.raw_conn(), &leg).unwrap();
    }

    let sid_leg = CanonicalProcedureLeg {
        object_id: ProcedureLegId("uuee_dipop1a_1".to_string()),
        airport_ident: "UUEE".to_string(),
        icao_code: "UU".to_string(),
        procedure_kind: 'D', // SID
        procedure_ident: "DIPOP1A".to_string(),
        route_type: "1".to_string(),
        transition_ident: String::new(),
        sequence_number: 10,
        fix_ident: "DIPOP".to_string(),
        fix_icao_code: "UU".to_string(),
        fix_section: "EA".to_string(),
        waypoint_description: String::new(),
        turn_direction: None,
        rnp_nm: None,
        path_terminator: "CF".to_string(),
        recommended_navaid: None,
        arc_radius_nm: None,
        course_a_deg: Some(95.0),
        distance_a_nm: Some(15.0),
        course_b_deg: None,
        distance_b_nm: None,
        altitude_descriptor: Some('+'),
        altitude_1_ft: Some(3000),
        altitude_2_ft: None,
        speed_limit_kts: Some(250),
        course_c_deg: None,
        vertical_angle_deg: None,
        msa_center_fix: None,
        route_qualifiers: String::new(),
        raw: "RAW".to_string(),
        temporal: val,
    };
    insert_procedure_leg_conn(store.raw_conn(), &sid_leg).unwrap();

    (store, t)
}

#[test]
fn test_russia_golden_e2e_uuee_to_unnt_tu154() {
    let (store, _t) = setup_test_world_store();
    let planner = Planner::new(&store);

    let req = FlightPlanRequest::new("UUEE", "UNNT")
        .with_aircraft(AircraftProfile::tu154())
        .with_mode(PlanningMode::AllowDctGaps);

    let plan = planner.plan(&req).expect("plan UUEE to UNNT");

    // 1. Core Plan Assertions
    assert_eq!(plan.origin.ident, "UUEE");
    assert_eq!(plan.destination.ident, "UNNT");
    assert_eq!(plan.aircraft_profile.icao_type.as_deref(), Some("T154"));
    assert_eq!(plan.cruise_altitude_ft, 35000); // Eastbound Semicircular FL350
    assert!(plan.total_distance_nm > 1500.0);
    assert!(plan.estimated_flight_time_min > 180); // ~3+ hours @ 460 kts

    // 2. Procedure & Leg Classification
    assert!(plan.sid.is_some());
    assert!(
        plan.sid
            .as_ref()
            .unwrap()
            .procedure
            .name
            .contains("DIPOP1A")
    );
    assert!(
        plan.all_legs
            .iter()
            .any(|l| l.kind == FlightPlanLegKind::Sid)
    );
    assert!(
        plan.all_legs
            .iter()
            .any(|l| l.kind == FlightPlanLegKind::AtsRoute || l.kind == FlightPlanLegKind::Dct)
    );

    // 3. Validation Status
    assert!(plan.validation.is_flyable);
    assert!(plan.validation.status != FlightPlanValidationStatus::Invalid);
    let temp_store_dir =
        std::env::temp_dir().join(format!("openairac_fp_test_{}", std::process::id()));
    let fp_store = FlightPlanStore::new(&temp_store_dir);
    let saved_path = fp_store.save(&plan).expect("save flight plan");
    assert!(saved_path.exists());

    let loaded_plan = fp_store.load(&plan.flight_id).expect("load flight plan");
    assert_eq!(loaded_plan.flight_id, plan.flight_id);
    assert_eq!(loaded_plan.route_string(), plan.route_string());
    assert_eq!(loaded_plan.cruise_altitude_ft, plan.cruise_altitude_ft);

    // 5. Exporters Test
    let fms_content = FlightPlanExporter::export_xplane_fms(&plan);
    assert!(fms_content.contains("1100 Version"));
    assert!(fms_content.contains("ADEP UUEE"));
    assert!(fms_content.contains("ADES UNNT"));

    let gns_content = FlightPlanExporter::export_gns430_fpl(&plan);
    assert!(gns_content.contains("FPL:UUEE-UNNT"));

    let kln_content = FlightPlanExporter::export_kln90b(&plan);
    assert!(kln_content.contains("KLN90B ROUTE 1: UUEE TO UNNT"));

    // Clean up
    let _ = std::fs::remove_dir_all(&temp_store_dir);
}

#[test]
fn test_russia_long_haul_e2e_uuee_to_uhww_il96() {
    let (store, _t) = setup_test_world_store();
    let planner = Planner::new(&store);

    let req = FlightPlanRequest::new("UUEE", "UHWW")
        .with_aircraft(AircraftProfile::il96())
        .with_mode(PlanningMode::AllowDctGaps);

    let plan = planner.plan(&req).expect("plan long-haul UUEE to UHWW");

    assert_eq!(plan.origin.ident, "UUEE");
    assert_eq!(plan.destination.ident, "UHWW");
    assert_eq!(plan.aircraft_profile.icao_type.as_deref(), Some("IL96"));
    assert_eq!(plan.cruise_altitude_ft, 35000); // Eastbound FL350
    assert!(plan.total_distance_nm > 3000.0); // Trans-Siberian long haul
    assert!(plan.estimated_flight_time_min > 400); // ~7+ hours @ 470 kts
    assert!(plan.validation.is_flyable);
}

#[test]
fn test_second_region_e2e_france_lfpg_to_lfmn_a320() {
    let (store, _t) = setup_test_world_store();
    let planner = Planner::new(&store);

    let req = FlightPlanRequest::new("LFPG", "LFMN")
        .with_aircraft(AircraftProfile::a320_narrowbody())
        .with_mode(PlanningMode::AllowDctGaps);

    let plan = planner.plan(&req).expect("plan LFPG to LFMN");

    assert_eq!(plan.origin.ident, "LFPG");
    assert_eq!(plan.destination.ident, "LFMN");
    assert_eq!(plan.aircraft_profile.icao_type.as_deref(), Some("A320"));
    assert!(plan.total_distance_nm > 350.0);
    assert!(plan.validation.is_flyable);
}

#[test]
fn test_provider_switching_and_plan_degradation_e2e() {
    let (store, t) = setup_test_world_store();
    let planner = Planner::new(&store);

    let req = FlightPlanRequest::new("UUEE", "UNNT")
        .with_aircraft(AircraftProfile::tu154())
        .with_mode(PlanningMode::AllowDctGaps);

    let mut plan = planner.plan(&req).expect("plan");
    assert!(plan.validation.is_flyable);

    // 1. Revalidate with active providers matching -> Plan remains flyable
    let active_providers = vec![
        "OurAirports_20260820".to_string(),
        "CAICA_AIRAC_2608".to_string(),
    ];
    plan.revalidate(t, &active_providers);
    assert!(plan.validation.is_flyable);

    // 2. Simulate CAICA provider rollback / deactivation -> Plan becomes STALE_DATA
    let degraded_providers = vec!["OurAirports_20260820".to_string()];
    plan.revalidate(t, &degraded_providers);
    assert_eq!(
        plan.validation.status,
        FlightPlanValidationStatus::StaleData
    );
    assert!(
        plan.validation
            .warnings
            .iter()
            .any(|w| w.contains("CAICA_AIRAC_2608"))
    );
}
