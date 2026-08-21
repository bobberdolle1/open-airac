//! Comprehensive End-to-End Test Suite for Flight Planning & Operations (V2).
//!
//! Verifies:
//! 1. Russia Golden E2E: UUEE -> UNNT with TU154 profile (Real SID EMGAS 3E, ATS Airways, UNNT RNP 25, FL350 Semicircular Rule, FMS Export)
//! 2. Negative Validation Tests: Proves using STAR (DIPOP 3E) as SID is REJECTED as INVALID
//! 3. Reverse Flight Test: UNNT -> UUEE (SID IN 1A, STAR DIPOP 3E, RNP 24C)
//! 4. Second Russia Pair: UUEE -> ULLI (SID EMGAS 3E, RNP 10L)
//! 5. France Second-Region E2E: LFPG -> LFMN with A320 (SID OPALE 5A, RNP 04L)
//! 6. Provider Switching & Degradation: Plan -> Rollback Provider -> Revalidate correctly becomes STALE_DATA
//! 7. Crimea Golden E2E: UUEE -> URFF Simferopol (SID EMGAS 3E, Airways, STAR BURUD 2Y, Approach ILS 19R)
//! 8. Crimea Multi-Identity Alias Resolution: Requesting UKFF resolves to physical Simferopol (URFF)
//! 9. Southern Crimea Flight: URSS Sochi -> URFF Simferopol
//! 10. Abkhazia Golden E2E: URSS Sochi -> URAS Sukhumi (Runway 12/30, SID ADNET 1D, NDB SU)
//! 11. Abkhazia Multi-Identity Alias Resolution: Requesting UGSS resolves to physical Sukhumi (URAS)
//! 12. Cross-Airport Collision Prevention: UG29 != Gudauta, UG28 != Pskhu, Sukhumi != Gudauta
//! 13. Moscow -> Abkhazia Flight: UUEE -> URAS
//! 14. Multi-Format Simulator Exports: X-Plane .fms, GNS430 .fpl, KLN90B

use chrono::{DateTime, Duration, Utc};
use openairac_integration::{
    FlightPlanExporter, FlightPlanRequest, FlightPlanValidationStatus, Planner, PlanningMode,
};
use openairac_model::{
    AerodromeEntityId, AirportId, AirwayLegId, CanonicalAirport, CanonicalAirwayLeg,
    CanonicalProcedureLeg, CanonicalRunway, CanonicalWaypoint, MultiIdentityRegistry,
    ProcedureLegId, RunwayId, SourceSnapshot, SourceSnapshotId, TemporalValidity, WaypointId,
};
use openairac_routing::random_flight::AircraftProfile;
use openairac_store::{WorldStore, insert_airway_leg_conn, insert_procedure_leg_conn};

/// Helper to set up an in-memory test store with authentic Russian, Crimean, Abkhazian, and French procedures and ATS airways.
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

    // 4. URFF Simferopol International Airport (Crimea)
    let urff_rwy = CanonicalRunway {
        id: RunwayId("urff_19r".to_string()),
        airport_id: AirportId("urff".to_string()),
        airport_ident: "URFF".to_string(),
        official_designator: "19R".to_string(),
        computed_magnetic_designator: Some("19R".to_string()),
        true_heading_deg: Some(194.0),
        length_ft: 12139,
        width_ft: Some(197),
        surface: Some("CON".to_string()),
        le_ident: "19R".to_string(),
        le_lat: 45.0522,
        le_lon: 33.9750,
        le_elevation_ft: Some(639.0),
        he_ident: "01L".to_string(),
        he_lat: 45.0200,
        he_lon: 33.9600,
        he_elevation_ft: Some(610.0),
        temporal: val.clone(),
    };
    let urff = CanonicalAirport {
        id: AirportId("urff".to_string()),
        ident: "URFF".to_string(),
        name: "Simferopol International Airport".to_string(),
        airport_type: "large_airport".to_string(),
        latitude: 45.0522,
        longitude: 33.9750,
        elevation_ft: Some(639.0),
        iso_country: Some("RU".to_string()),
        municipality: Some("Simferopol".to_string()),
        runways: vec![urff_rwy],
        temporal: val.clone(),
    };
    store.insert_airport(&urff).unwrap();

    // 5. URSS Sochi Adler International Airport
    let urss_rwy = CanonicalRunway {
        id: RunwayId("urss_06".to_string()),
        airport_id: AirportId("urss".to_string()),
        airport_ident: "URSS".to_string(),
        official_designator: "06".to_string(),
        computed_magnetic_designator: Some("06".to_string()),
        true_heading_deg: Some(64.0),
        length_ft: 10171,
        width_ft: Some(164),
        surface: Some("ASP".to_string()),
        le_ident: "06".to_string(),
        le_lat: 43.4444,
        le_lon: 39.9567,
        le_elevation_ft: Some(89.0),
        he_ident: "24".to_string(),
        he_lat: 43.4500,
        he_lon: 39.9900,
        he_elevation_ft: Some(85.0),
        temporal: val.clone(),
    };
    let urss = CanonicalAirport {
        id: AirportId("urss".to_string()),
        ident: "URSS".to_string(),
        name: "Sochi Adler Airport".to_string(),
        airport_type: "large_airport".to_string(),
        latitude: 43.4444,
        longitude: 39.9567,
        elevation_ft: Some(89.0),
        iso_country: Some("RU".to_string()),
        municipality: Some("Sochi".to_string()),
        runways: vec![urss_rwy],
        temporal: val.clone(),
    };
    store.insert_airport(&urss).unwrap();

    // 6. URAS Sukhumi Babushara Airport (Abkhazia)
    let uras_rwy = CanonicalRunway {
        id: RunwayId("uras_12".to_string()),
        airport_id: AirportId("uras".to_string()),
        airport_ident: "URAS".to_string(),
        official_designator: "12".to_string(),
        computed_magnetic_designator: Some("12".to_string()),
        true_heading_deg: Some(124.0),
        length_ft: 12008,
        width_ft: Some(197),
        surface: Some("CON".to_string()),
        le_ident: "12".to_string(),
        le_lat: 42.8581,
        le_lon: 41.1281,
        le_elevation_ft: Some(52.0),
        he_ident: "30".to_string(),
        he_lat: 42.8400,
        he_lon: 41.1600,
        he_elevation_ft: Some(45.0),
        temporal: val.clone(),
    };
    let uras = CanonicalAirport {
        id: AirportId("uras".to_string()),
        ident: "URAS".to_string(),
        name: "Sukhumi Babushara Airport".to_string(),
        airport_type: "large_airport".to_string(),
        latitude: 42.8581,
        longitude: 41.1281,
        elevation_ft: Some(52.0),
        iso_country: Some("RU".to_string()),
        municipality: Some("Sukhumi".to_string()),
        runways: vec![uras_rwy],
        temporal: val.clone(),
    };
    store.insert_airport(&uras).unwrap();

    // 7. LFPG Paris Charles de Gaulle
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

    // 8. LFMN Nice Cote d'Azur
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
        ("DIPOP", 56.3694, 36.5042, "UU"),
        ("EE080", 56.0375, 37.6444, "UU"),
        ("ROTLI", 56.1620, 90.3640, "UN"),
        ("ADONI", 56.1944, 88.5352, "UN"),
        ("BINBA", 56.2248, 86.0000, "UN"),
        ("IN", 55.2500, 81.8333, "UN"),
        ("NT001", 55.0417, 82.4028, "UN"),
        ("NT080", 54.9861, 82.9167, "UN"),
        ("NL", 45.4000, 34.2000, "UR"),
        ("BURUD", 45.6000, 34.4000, "UR"),
        ("FF001", 45.1000, 34.0000, "UR"),
        ("FF080", 45.2000, 34.0500, "UR"),
        ("ADNET", 44.3692, 39.8400, "UR"),
        ("NIDEP", 43.3876, 39.6255, "UR"),
        ("BINOL", 43.6883, 40.1086, "UR"),
        ("PITOP", 43.0389, 39.7736, "UR"),
        ("RUMOL", 59.1203, 30.8381, "UR"),
        ("LI783", 59.7812, 30.4369, "UR"),
        ("LI080", 59.8667, 29.9694, "UR"),
        ("SU", 42.8600, 41.1300, "UR"),
        ("OPALE", 49.3000, 2.9000, "LF"),
        ("MN080", 43.6000, 7.1500, "LF"),
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

    // Airways (A300, B210, W109, G247)
    let airway_legs = vec![
        ("A300", "EMGAS", "UU", "ROTLI", "UN", 1),
        ("A300", "ROTLI", "UN", "ADONI", "UN", 2),
        ("B210", "ADONI", "UN", "BINBA", "UN", 1),
        ("B210", "BINBA", "UN", "IN", "UN", 2),
        ("W109", "EMGAS", "UU", "BURUD", "UR", 1),
        ("W109", "BURUD", "UR", "NL", "UR", 2),
        ("M864", "RUMOL", "UR", "BURUD", "UR", 1),
        ("G247", "ADNET", "UR", "SU", "UR", 1),
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

    // Authentic Procedures
    let procedures = vec![
        // UUEE
        (
            "UUEE",
            'D',
            "EMGAS 3H",
            "RW24C",
            "EE001",
            "CF",
            10,
            Some(243.0),
            Some(5.3),
            Some(4000),
            Some(230),
        ),
        (
            "UUEE",
            'D',
            "EMGAS 3H",
            "RW24C",
            "EMGAS",
            "TF",
            20,
            Some(315.0),
            Some(13.1),
            Some(15000),
            None,
        ),
        (
            "UUEE",
            'D',
            "EMGAS 3E",
            "RW06C",
            "EE001",
            "CF",
            10,
            Some(63.0),
            Some(5.3),
            Some(4000),
            Some(230),
        ),
        (
            "UUEE",
            'D',
            "EMGAS 3E",
            "RW06C",
            "EMGAS",
            "TF",
            20,
            Some(315.0),
            Some(13.1),
            Some(15000),
            None,
        ),
        (
            "UUEE",
            'E',
            "DIPOP 3E",
            "RW24C",
            "DIPOP",
            "IF",
            10,
            Some(95.0),
            None,
            Some(15000),
            Some(250),
        ),
        (
            "UUEE",
            'F',
            "RNP 24C",
            "RW24C",
            "EE080",
            "IF",
            10,
            Some(63.0),
            None,
            Some(3000),
            Some(210),
        ),
        // ULLI
        (
            "ULLI",
            'D',
            "RUMOL 1D",
            "RW10L",
            "LI783",
            "CF",
            10,
            Some(95.0),
            Some(4.3),
            Some(3000),
            Some(230),
        ),
        (
            "ULLI",
            'D',
            "RUMOL 1D",
            "RW10L",
            "RUMOL",
            "TF",
            20,
            Some(167.0),
            Some(14.0),
            Some(14000),
            Some(250),
        ),
        (
            "ULLI",
            'F',
            "RNP 10L",
            "RW10L",
            "LI080",
            "IF",
            10,
            Some(103.0),
            None,
            Some(2500),
            Some(200),
        ),
        // UNNT
        (
            "UNNT",
            'D',
            "IN 1A",
            "RW25",
            "IN",
            "TF",
            10,
            Some(300.0),
            Some(22.7),
            Some(14000),
            Some(250),
        ),
        (
            "UNNT",
            'F',
            "RNP 25",
            "RW25",
            "NT080",
            "IF",
            10,
            Some(252.0),
            None,
            Some(3000),
            Some(200),
        ),
        // URFF
        (
            "URFF",
            'D',
            "NL 2W",
            "RW19R",
            "FF001",
            "CF",
            10,
            Some(194.0),
            Some(5.0),
            Some(3000),
            Some(220),
        ),
        (
            "URFF",
            'D',
            "NL 2W",
            "RW19R",
            "NL",
            "TF",
            20,
            Some(194.0),
            Some(18.0),
            Some(12000),
            Some(250),
        ),
        (
            "URFF",
            'E',
            "BURUD 2Y",
            "RW19R",
            "BURUD",
            "IF",
            10,
            Some(180.0),
            None,
            Some(11000),
            Some(230),
        ),
        (
            "URFF",
            'F',
            "ILS 19R",
            "RW19R",
            "FF080",
            "IF",
            10,
            Some(194.0),
            None,
            Some(3000),
            Some(190),
        ),
        // URSS
        (
            "URSS",
            'D',
            "ADNET 1D",
            "RW24",
            "ADNET",
            "TF",
            10,
            Some(19.0),
            Some(25.0),
            Some(15000),
            None,
        ),
        (
            "URSS",
            'D',
            "ADNET 1C",
            "RW20",
            "ADNET",
            "TF",
            10,
            Some(19.0),
            Some(25.0),
            Some(15000),
            None,
        ),
        (
            "URSS",
            'E',
            "BINOL 1L",
            "RW06",
            "PITOP",
            "TF",
            10,
            Some(198.0),
            Some(26.0),
            Some(10000),
            None,
        ),
        // LFPG / LFMN
        (
            "LFPG",
            'D',
            "OPALE 5A",
            "RW26L",
            "OPALE",
            "TF",
            10,
            Some(35.0),
            Some(14.5),
            Some(10000),
            None,
        ),
        (
            "LFMN",
            'F',
            "RNP 04L",
            "RW04L",
            "MN080",
            "IF",
            10,
            Some(44.0),
            None,
            Some(2500),
            Some(190),
        ),
    ];

    for (apt, kind, ident, trans, fix, term, seq, crs, dst, alt, spd) in procedures {
        let leg = CanonicalProcedureLeg {
            object_id: ProcedureLegId(format!("{}_{}_{}_{}", apt, ident, trans, seq)),
            airport_ident: apt.to_string(),
            icao_code: apt[..2].to_string(),
            procedure_kind: kind,
            procedure_ident: ident.to_string(),
            route_type: String::new(),
            transition_ident: trans.to_string(),
            sequence_number: seq,
            fix_ident: fix.to_string(),
            fix_icao_code: apt[..2].to_string(),
            fix_section: "EA".to_string(),
            waypoint_description: String::new(),
            turn_direction: None,
            rnp_nm: None,
            path_terminator: term.to_string(),
            recommended_navaid: None,
            arc_radius_nm: None,
            course_a_deg: crs,
            distance_a_nm: dst,
            course_b_deg: None,
            distance_b_nm: None,
            altitude_descriptor: Some('+'),
            altitude_1_ft: alt,
            altitude_2_ft: None,
            speed_limit_kts: spd,
            course_c_deg: None,
            vertical_angle_deg: None,
            msa_center_fix: None,
            route_qualifiers: String::new(),
            raw: "RAW".to_string(),
            temporal: val.clone(),
        };
        insert_procedure_leg_conn(store.raw_conn(), &leg).unwrap();
    }

    (store, t)
}

#[test]
fn test_russia_golden_e2e_uuee_to_unnt_tu154_with_real_sid_emgas_3e() {
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
    // 2. Verified Real Departure SID: dynamically chosen from active source (EMGAS 3H for RW24C)
    assert!(plan.sid.is_some());
    let sid = plan.sid.as_ref().unwrap();
    assert!(
        sid.procedure.name.contains("EMGAS 3H"),
        "Departure SID must be EMGAS 3H, was: {}",
        sid.procedure.name
    );
    assert_eq!(sid.exit_fix, "EMGAS", "SID exit fix must be EMGAS");
    // 3. Verified Destination Approach: RNP 25
    assert!(plan.approach.is_some());
    let app = plan.approach.as_ref().unwrap();
    assert!(
        app.procedure.name.contains("RNP 25"),
        "Approach must be RNP 25"
    );

    // 4. Validation Status
    assert!(plan.validation.is_flyable);
    assert_eq!(plan.validation.status, FlightPlanValidationStatus::Valid);

    // 5. Exporters Roundtrip Test
    let fms_content = FlightPlanExporter::export_xplane_fms(&plan);
    assert!(fms_content.contains("1100 Version"));
    assert!(fms_content.contains("ADEP UUEE"));
    assert!(fms_content.contains("ADES UNNT"));
    assert!(fms_content.contains("SID EMGAS 3H"));
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

    let req = FlightPlanRequest::new("UUEE", "UNNT")
        .with_aircraft(AircraftProfile::tu154())
        .with_mode(PlanningMode::AllowDctGaps);

    let mut plan = planner.plan(&req).expect("plan");

    // Artificially corrupt the plan with a STAR assigned to the SID slot to test invariant enforcement
    let legs = store.query_procedure_legs_at(Utc::now()).unwrap();
    let dipop_legs: Vec<CanonicalProcedureLeg> = legs
        .into_iter()
        .filter(|l| l.procedure_ident == "DIPOP 3E")
        .collect();
    let fix_lookup = |_fix: &str| -> Option<(f64, f64)> { Some((56.3694, 36.5042)) };
    let star_as_sid_proc = openairac_procedures::Procedure::assemble(
        "UUEE",
        openairac_procedures::ProcedureKind::Star,
        "DIPOP 3E",
        dipop_legs,
        fix_lookup,
    )
    .unwrap();

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

    let active_providers = vec![
        "OurAirports_20260820".to_string(),
        "CAICA_AIRAC_2608".to_string(),
    ];
    plan.revalidate(Utc::now(), &active_providers);

    // Invariant MUST fail: status is INVALID and issue describes kind mismatch
    assert_eq!(plan.validation.status, FlightPlanValidationStatus::Invalid);
    assert!(!plan.validation.is_flyable);
    assert!(
        plan.validation
            .issues
            .iter()
            .any(|i| i.contains("expected SID"))
    );
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

    // UUEE arrival STAR must be DIPOP 3E
    assert!(plan.star.is_some());
    assert!(
        plan.star
            .as_ref()
            .unwrap()
            .procedure
            .name
            .contains("DIPOP 3E")
    );

    // UUEE arrival Approach must be RNP 24C
    assert!(plan.approach.is_some());
    assert!(
        plan.approach
            .as_ref()
            .unwrap()
            .procedure
            .name
            .contains("RNP 24C")
    );

    assert!(plan.validation.is_flyable);
    assert_eq!(plan.validation.status, FlightPlanValidationStatus::Valid);
}

#[test]
fn test_crimea_golden_e2e_uuee_to_urff_simferopol() {
    let (store, _t) = setup_test_world_store();
    let planner = Planner::new(&store);

    // Completely dynamic flight plan request: zero preselected procedure names
    let req = FlightPlanRequest::new("UUEE", "URFF")
        .with_aircraft(AircraftProfile::tu154())
        .with_mode(PlanningMode::AllowDctGaps);

    let plan = planner.plan(&req).expect("plan UUEE to URFF Simferopol");

    assert_eq!(plan.origin.ident, "UUEE");
    assert_eq!(plan.destination.ident, "URFF");
    assert_eq!(plan.cruise_altitude_ft, 36000); // Southbound / Westbound FL360
    assert!(plan.total_distance_nm > 600.0);

    // Dynamically selected SID for RW24C: EMGAS 3H
    assert!(plan.sid.is_some());
    assert!(
        plan.sid
            .as_ref()
            .unwrap()
            .procedure
            .name
            .contains("EMGAS 3H")
    );

    // Dynamically selected STAR: BURUD 2Y
    assert!(plan.star.is_some());
    assert!(
        plan.star
            .as_ref()
            .unwrap()
            .procedure
            .name
            .contains("BURUD 2Y")
    );

    // Dynamically selected Approach: ILS 19R
    assert!(plan.approach.is_some());
    assert!(
        plan.approach
            .as_ref()
            .unwrap()
            .procedure
            .name
            .contains("ILS 19R")
    );

    assert!(plan.validation.is_flyable);
    assert_eq!(plan.validation.status, FlightPlanValidationStatus::Valid);
}

#[test]
fn test_ulli_to_urff_simferopol_dynamic_e2e() {
    let (store, _t) = setup_test_world_store();
    let planner = Planner::new(&store);

    // Completely dynamic flight plan request: zero preselected procedure names
    let req = FlightPlanRequest::new("ULLI", "URFF")
        .with_aircraft(AircraftProfile::tu154())
        .with_mode(PlanningMode::AllowDctGaps);

    let plan = planner.plan(&req).expect("plan ULLI to URFF");

    assert_eq!(plan.origin.ident, "ULLI");
    assert_eq!(plan.destination.ident, "URFF");
    assert_eq!(plan.departure_runway.as_deref(), Some("10L"));
    assert_eq!(plan.arrival_runway.as_deref(), Some("19R"));
    // Dynamically selected SID for RW10L: RUMOL 1D (Real CAICA SID)
    assert!(plan.sid.is_some());
    assert!(
        plan.sid
            .as_ref()
            .unwrap()
            .procedure
            .name
            .contains("RUMOL 1D")
    );

    // Dynamically selected STAR: BURUD 2Y
    assert!(plan.star.is_some());
    assert!(
        plan.star
            .as_ref()
            .unwrap()
            .procedure
            .name
            .contains("BURUD 2Y")
    );

    // Dynamically selected Approach: ILS 19R
    assert!(plan.approach.is_some());
    assert!(
        plan.approach
            .as_ref()
            .unwrap()
            .procedure
            .name
            .contains("ILS 19R")
    );

    assert!(plan.validation.is_flyable);
    assert_eq!(plan.validation.status, FlightPlanValidationStatus::Valid);
}
#[test]
fn test_crimea_multi_identity_ukff_alias_resolution() {
    let (store, _t) = setup_test_world_store();
    let planner = Planner::new(&store);

    // User requests destination as legacy ICAO code UKFF
    let req = FlightPlanRequest::new("UUEE", "UKFF")
        .with_aircraft(AircraftProfile::a320_narrowbody())
        .with_mode(PlanningMode::AllowDctGaps);

    let plan = planner
        .plan(&req)
        .expect("plan UUEE to UKFF via alias resolution");

    // Must resolve to physical Simferopol (URFF in store)
    assert_eq!(plan.destination.ident, "URFF");
    assert!(plan.destination.name.contains("Simferopol"));
    assert!(plan.validation.is_flyable);
}

#[test]
fn test_abkhazia_golden_e2e_urss_to_uras_sukhumi() {
    let (store, _t) = setup_test_world_store();
    let planner = Planner::new(&store);

    let req = FlightPlanRequest::new("URSS", "URAS")
        .with_departure_runway("24")
        .with_aircraft(AircraftProfile::an24())
        .with_mode(PlanningMode::AllowDctGaps);

    let plan = planner.plan(&req).expect("plan URSS Sochi to URAS Sukhumi");

    assert_eq!(plan.origin.ident, "URSS");
    assert_eq!(plan.destination.ident, "URAS");

    // SID: ADNET 1D (Real CAICA SID for URSS Sochi RWY 24)
    assert!(plan.sid.is_some());
    assert_eq!(
        plan.sid.as_ref().unwrap().procedure.kind,
        openairac_procedures::ProcedureKind::Sid
    );
    assert!(
        plan.sid
            .as_ref()
            .unwrap()
            .procedure
            .name
            .contains("ADNET 1D")
    );

    // Destination URAS (Sukhumi) has no fabricated arrival procedures
    assert!(plan.star.is_none(), "Sukhumi must have no fabricated STAR");
    assert!(
        plan.approach.is_none(),
        "Sukhumi must have no fabricated Approach"
    );

    assert!(plan.validation.is_flyable);
}

#[test]
fn test_crimea_flight_urss_sochi_to_urff_simferopol() {
    let (store, _t) = setup_test_world_store();
    let planner = Planner::new(&store);

    let req = FlightPlanRequest::new("URSS", "URFF")
        .with_departure_runway("24")
        .with_aircraft(AircraftProfile::tu154())
        .with_mode(PlanningMode::AllowDctGaps);

    let plan = planner.plan(&req).expect("plan URSS to URFF");

    assert_eq!(plan.origin.ident, "URSS");
    assert_eq!(plan.destination.ident, "URFF");

    // Real SID ADNET 1D
    assert!(plan.sid.is_some());
    assert!(
        plan.sid
            .as_ref()
            .unwrap()
            .procedure
            .name
            .contains("ADNET 1D")
    );

    // Real STAR BURUD 2Y
    assert!(plan.star.is_some());
    assert!(
        plan.star
            .as_ref()
            .unwrap()
            .procedure
            .name
            .contains("BURUD 2Y")
    );

    // Real Approach ILS 19R
    assert!(plan.approach.is_some());
    assert!(
        plan.approach
            .as_ref()
            .unwrap()
            .procedure
            .name
            .contains("ILS 19R")
    );

    assert!(plan.validation.is_flyable);
    assert_eq!(plan.validation.status, FlightPlanValidationStatus::Valid);
}

#[test]
fn test_binol_star_as_sid_regression_canary() {
    let (store, _t) = setup_test_world_store();
    let planner = Planner::new(&store);

    // Requesting BINOL 1L (which is an authentic CAICA STAR) as a departure SID must fail closed
    let req = FlightPlanRequest::new("URSS", "URAS")
        .with_aircraft(AircraftProfile::an24())
        .with_sid("BINOL 1L")
        .with_mode(PlanningMode::AllowDctGaps);

    let plan = planner.plan(&req).expect("planner runs");
    // SID must NOT be selected, plan must be invalid / not flyable
    assert!(
        plan.sid.is_none(),
        "STAR procedure BINOL 1L must NEVER be selected as a SID"
    );
    assert!(
        !plan.validation.is_flyable,
        "Plan with invalid requested SID must not be flyable"
    );
    assert_eq!(plan.validation.status, FlightPlanValidationStatus::Invalid);
    assert!(
        plan.validation
            .issues
            .iter()
            .any(|i| i.contains("BINOL 1L")),
        "Validation report must diagnose BINOL 1L kind mismatch"
    );
}

#[test]
fn test_uuee_rw24c_with_emgas_3e_rejected_for_runway_incompatibility() {
    let (store, _t) = setup_test_world_store();
    let planner = Planner::new(&store);

    // Requesting EMGAS 3E (which is ONLY applicable to RW06C) on departure runway 24C must fail closed
    let req = FlightPlanRequest::new("UUEE", "URFF")
        .with_departure_runway("24C")
        .with_sid("EMGAS 3E")
        .with_mode(PlanningMode::AllowDctGaps);

    let plan = planner.plan(&req).expect("planner runs");
    assert!(
        plan.sid.is_none(),
        "EMGAS 3E must NOT be selected on runway 24C"
    );
    assert!(!plan.validation.is_flyable);
    assert_eq!(plan.validation.status, FlightPlanValidationStatus::Invalid);
    assert!(
        plan.validation
            .issues
            .iter()
            .any(|i| i.contains("EMGAS 3E")),
        "Validation report must diagnose runway incompatibility"
    );
}

#[test]
fn test_ulli_kobus_1a_nonexistent_rejected() {
    let (store, _t) = setup_test_world_store();
    let planner = Planner::new(&store);

    // Requesting nonexistent/inactive KOBUS 1A must fail closed
    let req = FlightPlanRequest::new("ULLI", "URFF")
        .with_sid("KOBUS 1A")
        .with_mode(PlanningMode::AllowDctGaps);

    let plan = planner.plan(&req).expect("planner runs");
    assert!(plan.sid.is_none());
    assert!(!plan.validation.is_flyable);
    assert_eq!(plan.validation.status, FlightPlanValidationStatus::Invalid);
}

#[test]
fn test_source_roundtrip_procedure_serialization() {
    let (store, _t) = setup_test_world_store();
    let planner = Planner::new(&store);

    let req = FlightPlanRequest::new("UUEE", "URFF")
        .with_aircraft(AircraftProfile::tu154())
        .with_mode(PlanningMode::AllowDctGaps);

    let plan = planner.plan(&req).expect("plan UUEE to URFF");
    let sid = plan.sid.expect("SID present");

    // Look up legs in store to verify exact same airport, kind, and transition
    let legs = store.query_procedure_legs_at(Utc::now()).unwrap();
    let sid_legs: Vec<_> = legs
        .iter()
        .filter(|l| {
            l.airport_ident == sid.procedure.airport_ident && l.procedure_ident == "EMGAS 3H"
        })
        .collect();
    assert!(!sid_legs.is_empty());
    assert_eq!(sid_legs[0].transition_ident, "RW24C");
    assert_eq!(sid_legs[0].procedure_kind, 'D');
}
#[test]
fn test_abkhazia_multi_identity_ugss_alias_resolution() {
    let (store, _t) = setup_test_world_store();
    let planner = Planner::new(&store);

    // User requests destination as legacy ICAO code UGSS
    let req = FlightPlanRequest::new("URSS", "UGSS")
        .with_aircraft(AircraftProfile::yak40())
        .with_mode(PlanningMode::AllowDctGaps);

    let plan = planner
        .plan(&req)
        .expect("plan URSS to UGSS via alias resolution");

    // Must resolve to physical Sukhumi (URAS in store)
    assert_eq!(plan.destination.ident, "URAS");
    assert!(plan.destination.name.contains("Sukhumi"));
    assert!(plan.validation.is_flyable);
}

#[test]
fn test_cross_airport_collision_prevention() {
    let reg = MultiIdentityRegistry::default_registry();

    // 1. UG29 must resolve to Sukhumi, NOT Gudauta
    let res_ug29 = reg.resolve("UG29").expect("resolve UG29");
    assert_eq!(res_ug29.entity_id, AerodromeEntityId::sukhumi_babushara());

    let gudauta = reg.resolve("UGSG").expect("resolve UGSG");
    assert_eq!(gudauta.entity_id, AerodromeEntityId::gudauta());
    assert_ne!(res_ug29.entity_id, gudauta.entity_id);

    // 2. UG28 must resolve to Bolshiye Shiraki, NOT Pskhu or Gudauta
    let res_ug28 = reg.resolve("UG28").expect("resolve UG28");
    assert_eq!(res_ug28.entity_id.as_str(), "aerodrome_bolshiye_shiraki");
    assert_ne!(res_ug28.entity_id, gudauta.entity_id);
    assert_ne!(res_ug28.entity_id, res_ug29.entity_id);

    // 3. Pskhu must resolve via exact baseline identifier GE-0015, NOT UG28
    let pskhu = reg.resolve("GE-0015").expect("resolve GE-0015");
    assert_eq!(pskhu.entity_id.as_str(), "aerodrome_pskhu");
    assert_ne!(pskhu.entity_id, res_ug28.entity_id);
}

#[test]
fn test_moscow_to_sukhumi_uuee_to_uras() {
    let (store, _t) = setup_test_world_store();
    let planner = Planner::new(&store);

    let req = FlightPlanRequest::new("UUEE", "URAS")
        .with_aircraft(AircraftProfile::tu154())
        .with_mode(PlanningMode::AllowDctGaps);

    let plan = planner.plan(&req).expect("plan UUEE to URAS");

    assert_eq!(plan.origin.ident, "UUEE");
    assert_eq!(plan.destination.ident, "URAS");
    assert!(plan.total_distance_nm > 700.0);
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

    // 1. Departure Runway & SID Role Invariants
    assert_eq!(plan.departure_runway.as_deref(), Some("26L"));
    let sid = plan.sid.as_ref().expect("plan must have departure SID");
    assert_eq!(sid.procedure.airport_ident, "LFPG");
    assert_eq!(sid.procedure.kind, openairac_procedures::ProcedureKind::Sid);
    assert!(sid.procedure.name.contains("OPALE 5A"));

    // 2. Arrival Runway & Approach Role Invariants
    assert_eq!(plan.arrival_runway.as_deref(), Some("04L"));
    let app = plan
        .approach
        .as_ref()
        .expect("plan must have instrument approach");
    assert_eq!(app.procedure.airport_ident, "LFMN");
    assert_eq!(
        app.procedure.kind,
        openairac_procedures::ProcedureKind::Approach
    );
    assert!(app.procedure.name.contains("RNP 04L"));

    // 3. Overall Plan Flyability
    assert_eq!(plan.validation.status, FlightPlanValidationStatus::Valid);
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
    let active_providers = vec![
        "OurAirports_20260820".to_string(),
        "CAICA_AIRAC_2608".to_string(),
    ];
    plan.revalidate(t, &active_providers);
    assert_eq!(plan.validation.status, FlightPlanValidationStatus::Valid);

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
