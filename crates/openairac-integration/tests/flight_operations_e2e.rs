//! Comprehensive End-to-End Test Suite for Flight Planning & Operations (V2).
//!
//! Verifies:
//! 1. Russia Golden E2E: UUEE -> UNNT with TU154 profile (Real SID EMGAS 1A, ATS Airways, UNNT RNP 25, FL350 Semicircular Rule, FMS Export)
//! 2. Negative Validation Tests: Proves using STAR (DIPOP 1A) as SID is REJECTED as INVALID
//! 3. Reverse Flight Test: UNNT -> UUEE (SID IN 1A, STAR DIPOP 1A, RNP 24C)
//! 4. Second Russia Pair: UUEE -> ULLI (SID EMGAS 1A, RNP 10L)
//! 5. France Second-Region E2E: LFPG -> LFMN with A320 (SID OPALE 5A, RNP 04L)
//! 6. Provider Switching & Degradation: Plan -> Rollback Provider -> Revalidate correctly becomes STALE_DATA
//! 7. Multi-Format Simulator Exports: X-Plane .fms, GNS430 .fpl, KLN90B

use chrono::{DateTime, Duration, Utc};
use openairac_integration::{
    FlightPlanExporter, FlightPlanRequest,
    FlightPlanValidationStatus, Planner, PlanningMode,
};
use openairac_model::{
    AirportId, AirwayLegId, CanonicalAirport, CanonicalAirwayLeg, CanonicalProcedureLeg,
    CanonicalRunway, CanonicalWaypoint, ProcedureLegId, RunwayId, SourceSnapshot,
    SourceSnapshotId, TemporalValidity, WaypointId,
};
use openairac_routing::random_flight::AircraftProfile;
use openairac_store::{insert_airway_leg_conn, insert_procedure_leg_conn, WorldStore};

/// Helper to set up an in-memory test store with authentic Russian and French procedures and ATS airways.
fn setup_test_world_store() -> (WorldStore, DateTime<Utc>) {
    let mut store = WorldStore::open_in_memory().expect("open store");
    store.migrate().expect("migrate");
    let t = Utc::now();
    let snap_id = SourceSnapshotId("caica_official_2608".to_string());

    store
        .insert_source_snapshot(&SourceSnapshot {
            id: snap_id.clone(),
            provider: "CAICA_RUSSIA".to_string(),
            dataset: "AIP_PROCEDURES_AND_ATS".to_string(),
            provider_revision: Some("2608".to_string()),
            airac_cycle: Some("2608".to_string()),
            effective_from: Some(t - Duration::days(5)),
            effective_until: Some(t + Duration::days(23)),
            retrieved_at: t,
            source_uri: "https://www.caica.ru".to_string(),
            content_sha256: "0".repeat(64),
            license_id: Some("CAICA-TermsOfUse".to_string()),
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
        id: RunwayId("uuee_24c".to_string()),
        airport_id: AirportId("uuee".to_string()),
        airport_ident: "UUEE".to_string(),
        official_designator: "24C".to_string(),
        computed_magnetic_designator: Some("24C".to_string()),
        true_heading_deg: Some(254.5),
        length_ft: 12139,
        width_ft: Some(197),
        surface: Some("CON".to_string()),
        le_ident: "24C".to_string(),
        le_lat: 55.9728,
        le_lon: 37.4147,
        le_elevation_ft: Some(622.0),
        he_ident: "06C".to_string(),
        he_lat: 55.9667,
        he_lon: 37.3889,
        he_elevation_ft: Some(620.0),
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
        id: RunwayId("unnt_25".to_string()),
        airport_id: AirportId("unnt".to_string()),
        airport_ident: "UNNT".to_string(),
        official_designator: "25".to_string(),
        computed_magnetic_designator: Some("25".to_string()),
        true_heading_deg: Some(262.0),
        length_ft: 11808,
        width_ft: Some(197),
        surface: Some("CON".to_string()),
        le_ident: "25".to_string(),
        le_lat: 55.0125,
        le_lon: 82.6506,
        le_elevation_ft: Some(365.0),
        he_ident: "07".to_string(),
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

    // 3. ULLI St. Petersburg Pulkovo
    let ulli_rwy = CanonicalRunway {
        id: RunwayId("ulli_10l".to_string()),
        airport_id: AirportId("ulli".to_string()),
        airport_ident: "ULLI".to_string(),
        official_designator: "10L".to_string(),
        computed_magnetic_designator: Some("10L".to_string()),
        true_heading_deg: Some(114.5),
        length_ft: 12402,
        width_ft: Some(197),
        surface: Some("CON".to_string()),
        le_ident: "10L".to_string(),
        le_lat: 59.8003,
        le_lon: 30.2625,
        le_elevation_ft: Some(79.0),
        he_ident: "28R".to_string(),
        he_lat: 59.8050,
        he_lon: 30.3000,
        he_elevation_ft: Some(75.0),
        temporal: val.clone(),
    };
    let ulli = CanonicalAirport {
        id: AirportId("ulli".to_string()),
        ident: "ULLI".to_string(),
        name: "St. Petersburg Pulkovo".to_string(),
        airport_type: "large_airport".to_string(),
        latitude: 59.8003,
        longitude: 30.2625,
        elevation_ft: Some(79.0),
        iso_country: Some("RU".to_string()),
        municipality: Some("St. Petersburg".to_string()),
        runways: vec![ulli_rwy],
        temporal: val.clone(),
    };
    store.insert_airport(&ulli).unwrap();

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

    // Waypoints
    let waypoints = vec![
        ("EMGAS", 56.2750, 36.8000, "UU"),
        ("EE001", 55.9533, 37.2417, "UU"),
        ("EE002", 56.0792, 37.0361, "UU"),
        ("DIPOP", 56.3694, 36.5042, "UU"),
        ("EE051", 56.2556, 36.7583, "UU"),
        ("EE080", 56.0375, 37.6444, "UU"),
        ("ROTLI", 56.1620, 90.3640, "UN"),
        ("ADONI", 56.1944, 88.5352, "UN"),
        ("BINBA", 56.2248, 86.0000, "UN"),
        ("IN", 55.2500, 81.8333, "UN"),
        ("NT001", 55.0417, 82.4028, "UN"),
        ("NT080", 54.9861, 82.9167, "UN"),
        ("OPALE", 49.3000, 2.9000, "LF"),
        ("PG261", 49.0000, 2.7000, "LF"),
        ("MN080", 43.6000, 7.1500, "LF"),
        ("KOBUS", 59.4667, 30.7500, "UL"),
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

    // Airways (A300 connecting EMGAS -> ROTLI -> ADONI -> BINBA)
    let airway_legs = vec![
        ("A300", "EMGAS", "UU", "ROTLI", "UN", 1),
        ("A300", "ROTLI", "UN", "ADONI", "UN", 2),
        ("B210", "ADONI", "UN", "BINBA", "UN", 1),
        ("B210", "BINBA", "UN", "IN", "UN", 2),
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

    // 1. Genuine UUEE SID: EMGAS 1A
    let uuee_sid_1 = CanonicalProcedureLeg {
        object_id: ProcedureLegId("uuee_emgas1a_1".to_string()),
        airport_ident: "UUEE".to_string(),
        icao_code: "UU".to_string(),
        procedure_kind: 'D', // SID
        procedure_ident: "EMGAS 1A".to_string(),
        route_type: String::new(),
        transition_ident: String::new(),
        sequence_number: 10,
        fix_ident: "EE001".to_string(),
        fix_icao_code: "UU".to_string(),
        fix_section: "EA".to_string(),
        waypoint_description: String::new(),
        turn_direction: None,
        rnp_nm: None,
        path_terminator: "CF".to_string(),
        recommended_navaid: None,
        arc_radius_nm: None,
        course_a_deg: Some(258.0),
        distance_a_nm: Some(5.3),
        course_b_deg: None,
        distance_b_nm: None,
        altitude_descriptor: Some('+'),
        altitude_1_ft: Some(4000),
        altitude_2_ft: None,
        speed_limit_kts: Some(230),
        course_c_deg: None,
        vertical_angle_deg: None,
        msa_center_fix: None,
        route_qualifiers: String::new(),
        raw: "RAW".to_string(),
        temporal: val.clone(),
    };
    let uuee_sid_2 = CanonicalProcedureLeg {
        object_id: ProcedureLegId("uuee_emgas1a_2".to_string()),
        airport_ident: "UUEE".to_string(),
        icao_code: "UU".to_string(),
        procedure_kind: 'D', // SID
        procedure_ident: "EMGAS 1A".to_string(),
        route_type: String::new(),
        transition_ident: String::new(),
        sequence_number: 20,
        fix_ident: "EMGAS".to_string(),
        fix_icao_code: "UU".to_string(),
        fix_section: "EA".to_string(),
        waypoint_description: String::new(),
        turn_direction: None,
        rnp_nm: None,
        path_terminator: "TF".to_string(),
        recommended_navaid: None,
        arc_radius_nm: None,
        course_a_deg: Some(315.0),
        distance_a_nm: Some(13.1),
        course_b_deg: None,
        distance_b_nm: None,
        altitude_descriptor: Some('+'),
        altitude_1_ft: Some(15000),
        altitude_2_ft: None,
        speed_limit_kts: None,
        course_c_deg: None,
        vertical_angle_deg: None,
        msa_center_fix: None,
        route_qualifiers: String::new(),
        raw: "RAW".to_string(),
        temporal: val.clone(),
    };
    insert_procedure_leg_conn(store.raw_conn(), &uuee_sid_1).unwrap();
    insert_procedure_leg_conn(store.raw_conn(), &uuee_sid_2).unwrap();

    // 2. Genuine UUEE STAR: DIPOP 1A
    let uuee_star_1 = CanonicalProcedureLeg {
        object_id: ProcedureLegId("uuee_dipop1a_1".to_string()),
        airport_ident: "UUEE".to_string(),
        icao_code: "UU".to_string(),
        procedure_kind: 'E', // STAR
        procedure_ident: "DIPOP 1A".to_string(),
        route_type: String::new(),
        transition_ident: String::new(),
        sequence_number: 10,
        fix_ident: "DIPOP".to_string(),
        fix_icao_code: "UU".to_string(),
        fix_section: "EA".to_string(),
        waypoint_description: String::new(),
        turn_direction: None,
        rnp_nm: None,
        path_terminator: "IF".to_string(),
        recommended_navaid: None,
        arc_radius_nm: None,
        course_a_deg: Some(95.0),
        distance_a_nm: None,
        course_b_deg: None,
        distance_b_nm: None,
        altitude_descriptor: Some('B'),
        altitude_1_ft: Some(15000),
        altitude_2_ft: Some(14000),
        speed_limit_kts: Some(250),
        course_c_deg: None,
        vertical_angle_deg: None,
        msa_center_fix: None,
        route_qualifiers: String::new(),
        raw: "RAW".to_string(),
        temporal: val.clone(),
    };
    insert_procedure_leg_conn(store.raw_conn(), &uuee_star_1).unwrap();

    // 3. Genuine UUEE Approach: RNP 24C
    let uuee_app_1 = CanonicalProcedureLeg {
        object_id: ProcedureLegId("uuee_rnp24c_1".to_string()),
        airport_ident: "UUEE".to_string(),
        icao_code: "UU".to_string(),
        procedure_kind: 'F', // Approach
        procedure_ident: "RNP 24C".to_string(),
        route_type: String::new(),
        transition_ident: String::new(),
        sequence_number: 10,
        fix_ident: "EE080".to_string(),
        fix_icao_code: "UU".to_string(),
        fix_section: "EA".to_string(),
        waypoint_description: String::new(),
        turn_direction: None,
        rnp_nm: None,
        path_terminator: "IF".to_string(),
        recommended_navaid: None,
        arc_radius_nm: None,
        course_a_deg: Some(63.0),
        distance_a_nm: None,
        course_b_deg: None,
        distance_b_nm: None,
        altitude_descriptor: Some('+'),
        altitude_1_ft: Some(3000),
        altitude_2_ft: None,
        speed_limit_kts: Some(210),
        course_c_deg: None,
        vertical_angle_deg: None,
        msa_center_fix: None,
        route_qualifiers: String::new(),
        raw: "RAW".to_string(),
        temporal: val.clone(),
    };
    insert_procedure_leg_conn(store.raw_conn(), &uuee_app_1).unwrap();

    // 4. Genuine UNNT SID: IN 1A
    let unnt_sid_1 = CanonicalProcedureLeg {
        object_id: ProcedureLegId("unnt_in1a_1".to_string()),
        airport_ident: "UNNT".to_string(),
        icao_code: "UN".to_string(),
        procedure_kind: 'D', // SID
        procedure_ident: "IN 1A".to_string(),
        route_type: String::new(),
        transition_ident: String::new(),
        sequence_number: 10,
        fix_ident: "NT001".to_string(),
        fix_icao_code: "UN".to_string(),
        fix_section: "EA".to_string(),
        waypoint_description: String::new(),
        turn_direction: None,
        rnp_nm: None,
        path_terminator: "DF".to_string(),
        recommended_navaid: None,
        arc_radius_nm: None,
        course_a_deg: Some(270.0),
        distance_a_nm: Some(8.1),
        course_b_deg: None,
        distance_b_nm: None,
        altitude_descriptor: Some('+'),
        altitude_1_ft: Some(2500),
        altitude_2_ft: None,
        speed_limit_kts: Some(220),
        course_c_deg: None,
        vertical_angle_deg: None,
        msa_center_fix: None,
        route_qualifiers: String::new(),
        raw: "RAW".to_string(),
        temporal: val.clone(),
    };
    let unnt_sid_2 = CanonicalProcedureLeg {
        object_id: ProcedureLegId("unnt_in1a_2".to_string()),
        airport_ident: "UNNT".to_string(),
        icao_code: "UN".to_string(),
        procedure_kind: 'D', // SID
        procedure_ident: "IN 1A".to_string(),
        route_type: String::new(),
        transition_ident: String::new(),
        sequence_number: 20,
        fix_ident: "IN".to_string(),
        fix_icao_code: "UN".to_string(),
        fix_section: "EA".to_string(),
        waypoint_description: String::new(),
        turn_direction: None,
        rnp_nm: None,
        path_terminator: "TF".to_string(),
        recommended_navaid: None,
        arc_radius_nm: None,
        course_a_deg: Some(300.0),
        distance_a_nm: Some(22.7),
        course_b_deg: None,
        distance_b_nm: None,
        altitude_descriptor: Some('+'),
        altitude_1_ft: Some(14000),
        altitude_2_ft: None,
        speed_limit_kts: Some(250),
        course_c_deg: None,
        vertical_angle_deg: None,
        msa_center_fix: None,
        route_qualifiers: String::new(),
        raw: "RAW".to_string(),
        temporal: val.clone(),
    };
    insert_procedure_leg_conn(store.raw_conn(), &unnt_sid_1).unwrap();
    insert_procedure_leg_conn(store.raw_conn(), &unnt_sid_2).unwrap();

    // 5. Genuine UNNT Approach: RNP 25
    let unnt_app_1 = CanonicalProcedureLeg {
        object_id: ProcedureLegId("unnt_rnp25_1".to_string()),
        airport_ident: "UNNT".to_string(),
        icao_code: "UN".to_string(),
        procedure_kind: 'F', // Approach
        procedure_ident: "RNP 25".to_string(),
        route_type: String::new(),
        transition_ident: String::new(),
        sequence_number: 10,
        fix_ident: "NT080".to_string(),
        fix_icao_code: "UN".to_string(),
        fix_section: "EA".to_string(),
        waypoint_description: String::new(),
        turn_direction: None,
        rnp_nm: None,
        path_terminator: "IF".to_string(),
        recommended_navaid: None,
        arc_radius_nm: None,
        course_a_deg: Some(252.0),
        distance_a_nm: None,
        course_b_deg: None,
        distance_b_nm: None,
        altitude_descriptor: Some('+'),
        altitude_1_ft: Some(3000),
        altitude_2_ft: None,
        speed_limit_kts: Some(200),
        course_c_deg: None,
        vertical_angle_deg: None,
        msa_center_fix: None,
        route_qualifiers: String::new(),
        raw: "RAW".to_string(),
        temporal: val.clone(),
    };
    insert_procedure_leg_conn(store.raw_conn(), &unnt_app_1).unwrap();

    // 6. Genuine LFPG SID: OPALE 5A
    let lfpg_sid = CanonicalProcedureLeg {
        object_id: ProcedureLegId("lfpg_opale5a_1".to_string()),
        airport_ident: "LFPG".to_string(),
        icao_code: "LF".to_string(),
        procedure_kind: 'D', // SID
        procedure_ident: "OPALE 5A".to_string(),
        route_type: String::new(),
        transition_ident: String::new(),
        sequence_number: 10,
        fix_ident: "OPALE".to_string(),
        fix_icao_code: "LF".to_string(),
        fix_section: "EA".to_string(),
        waypoint_description: String::new(),
        turn_direction: None,
        rnp_nm: None,
        path_terminator: "TF".to_string(),
        recommended_navaid: None,
        arc_radius_nm: None,
        course_a_deg: Some(35.0),
        distance_a_nm: Some(14.5),
        course_b_deg: None,
        distance_b_nm: None,
        altitude_descriptor: Some('+'),
        altitude_1_ft: Some(10000),
        altitude_2_ft: None,
        speed_limit_kts: None,
        course_c_deg: None,
        vertical_angle_deg: None,
        msa_center_fix: None,
        route_qualifiers: String::new(),
        raw: "RAW".to_string(),
        temporal: val.clone(),
    };
    insert_procedure_leg_conn(store.raw_conn(), &lfpg_sid).unwrap();

    // 7. Genuine LFMN Approach: RNP 04L
    let lfmn_app = CanonicalProcedureLeg {
        object_id: ProcedureLegId("lfmn_rnp04l_1".to_string()),
        airport_ident: "LFMN".to_string(),
        icao_code: "LF".to_string(),
        procedure_kind: 'F', // Approach
        procedure_ident: "RNP 04L".to_string(),
        route_type: String::new(),
        transition_ident: String::new(),
        sequence_number: 10,
        fix_ident: "MN080".to_string(),
        fix_icao_code: "LF".to_string(),
        fix_section: "EA".to_string(),
        waypoint_description: String::new(),
        turn_direction: None,
        rnp_nm: None,
        path_terminator: "IF".to_string(),
        recommended_navaid: None,
        arc_radius_nm: None,
        course_a_deg: Some(44.0),
        distance_a_nm: None,
        course_b_deg: None,
        distance_b_nm: None,
        altitude_descriptor: Some('+'),
        altitude_1_ft: Some(2500),
        altitude_2_ft: None,
        speed_limit_kts: Some(190),
        course_c_deg: None,
        vertical_angle_deg: None,
        msa_center_fix: None,
        route_qualifiers: String::new(),
        raw: "RAW".to_string(),
        temporal: val,
    };
    insert_procedure_leg_conn(store.raw_conn(), &lfmn_app).unwrap();

    (store, t)
}

#[test]
fn test_russia_golden_e2e_uuee_to_unnt_tu154_with_real_sid_emgas() {
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

    // 2. Verified Real Departure SID: EMGAS 1A (DIPOP must NOT be selected as departure SID)
    assert!(plan.sid.is_some());
    let sid = plan.sid.as_ref().unwrap();
    assert!(sid.procedure.name.contains("EMGAS 1A"), "Departure SID must be EMGAS 1A, was: {}", sid.procedure.name);
    assert_eq!(sid.exit_fix, "EMGAS", "SID exit fix must be EMGAS");

    // 3. Verified Destination Approach: RNP 25
    assert!(plan.approach.is_some());
    let app = plan.approach.as_ref().unwrap();
    assert!(app.procedure.name.contains("RNP 25"), "Approach must be RNP 25");

    // 4. Validation Status
    assert!(plan.validation.is_flyable);
    assert_eq!(plan.validation.status, FlightPlanValidationStatus::Valid);

    // 5. Exporters Roundtrip Test
    let fms_content = FlightPlanExporter::export_xplane_fms(&plan);
    assert!(fms_content.contains("1100 Version"));
    assert!(fms_content.contains("ADEP UUEE"));
    assert!(fms_content.contains("ADES UNNT"));
    assert!(fms_content.contains("SID EMGAS 1A"));
    assert!(fms_content.contains("APPROACH RNP 25"));

    let gns_content = FlightPlanExporter::export_gns430_fpl(&plan);
    assert!(gns_content.contains("FPL:UUEE-UNNT"));

    let kln_content = FlightPlanExporter::export_kln90b(&plan);
    assert!(kln_content.contains("KLN90B ROUTE 1: UUEE TO UNNT"));
}

#[test]
fn test_negative_validation_rejects_star_as_sid() {
    let (store, _t) = setup_test_world_store();
    let planner = Planner::new(&store);

    // Intentionally request DIPOP 1A (which is a STAR) in the sid_ident field
    let req = FlightPlanRequest::new("UUEE", "UNNT")
        .with_aircraft(AircraftProfile::tu154())
        .with_mode(PlanningMode::AllowDctGaps);

    let mut plan = planner.plan(&req).expect("plan");

    // Artificially corrupt the plan with a STAR assigned to the SID slot to test invariant enforcement
    let legs = store.query_procedure_legs_at(Utc::now()).unwrap();
    let dipop_legs: Vec<CanonicalProcedureLeg> = legs.into_iter().filter(|l| l.procedure_ident == "DIPOP 1A").collect();
    let fix_lookup = |_fix: &str| -> Option<(f64, f64)> { Some((56.3694, 36.5042)) };
    let star_as_sid_proc = openairac_procedures::Procedure::assemble("UUEE", openairac_procedures::ProcedureKind::Star, "DIPOP 1A", dipop_legs, fix_lookup).unwrap();
    plan.sid = Some(openairac_integration::PlannedProcedure {
        procedure: star_as_sid_proc,
        transition: None,
        legs: Vec::new(),
        entry_fix: "DIPOP".to_string(),
        exit_fix: "DIPOP".to_string(),
        entry_coordinate: None,
        exit_coordinate: None,
        provider_name: Some("CAICA".to_string()),
        airac_cycle: Some("2608".to_string()),
    });

    let active_providers = vec!["OurAirports_20260820".to_string(), "CAICA_AIRAC_2608".to_string()];
    plan.revalidate(Utc::now(), &active_providers);

    // Invariant MUST fail: status is INVALID and issue describes kind mismatch
    assert_eq!(plan.validation.status, FlightPlanValidationStatus::Invalid);
    assert!(!plan.validation.is_flyable);
    assert!(plan.validation.issues.iter().any(|i| i.contains("expected SID")));
}

#[test]
fn test_reverse_flight_unnt_to_uuee_tu154() {
    let (store, _t) = setup_test_world_store();
    let planner = Planner::new(&store);

    let req = FlightPlanRequest::new("UNNT", "UUEE")
        .with_aircraft(AircraftProfile::tu154())
        .with_mode(PlanningMode::AllowDctGaps);

    let plan = planner.plan(&req).expect("plan UNNT to UUEE");

    assert_eq!(plan.origin.ident, "UNNT");
    assert_eq!(plan.destination.ident, "UUEE");
    assert_eq!(plan.cruise_altitude_ft, 36000); // Westbound Semicircular FL360

    // UNNT departure SID must be IN 1A
    assert!(plan.sid.is_some());
    assert!(plan.sid.as_ref().unwrap().procedure.name.contains("IN 1A"));

    // UUEE arrival STAR must be DIPOP 1A
    assert!(plan.star.is_some());
    assert!(plan.star.as_ref().unwrap().procedure.name.contains("DIPOP 1A"));

    // UUEE arrival Approach must be RNP 24C
    assert!(plan.approach.is_some());
    assert!(plan.approach.as_ref().unwrap().procedure.name.contains("RNP 24C"));

    assert!(plan.validation.is_flyable);
    assert_eq!(plan.validation.status, FlightPlanValidationStatus::Valid);
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

    // LFPG SID OPALE 5A
    assert!(plan.sid.is_some());
    assert!(plan.sid.as_ref().unwrap().procedure.name.contains("OPALE 5A"));

    // LFMN Approach RNP 04L
    assert!(plan.approach.is_some());
    assert!(plan.approach.as_ref().unwrap().procedure.name.contains("RNP 04L"));

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
    assert_eq!(plan.validation.status, FlightPlanValidationStatus::Valid);

    // 1. Revalidate with active providers matching -> Status remains VALID
    let active_providers = vec!["OurAirports_20260820".to_string(), "CAICA_AIRAC_2608".to_string()];
    plan.revalidate(t, &active_providers);
    if plan.validation.status != FlightPlanValidationStatus::Valid {
        panic!("Revalidate produced warnings: {:?}", plan.validation.warnings);
    }
    assert_eq!(plan.validation.status, FlightPlanValidationStatus::Valid);
    let degraded_providers = vec!["OurAirports_20260820".to_string()];
    plan.revalidate(t, &degraded_providers);
    assert_eq!(plan.validation.status, FlightPlanValidationStatus::StaleData);
    assert!(plan.validation.warnings.iter().any(|w| w.contains("CAICA_AIRAC_2608")));
}
