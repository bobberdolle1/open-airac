//! Deterministic Telemetry Replay Harness and E2E Scenario Verification.
//! Tests end-to-end flight execution across Russia (UUEE->URFF),
//! Abkhazia (URSS->URAS with SOURCE_REQUIRED semantics), and France (LFPG->LFMN).

use chrono::{Duration, Utc};
use openairac_integration::{
    AircraftProfile, FlightExecutionSession, FlightPhase, FlightPlan, FlightPlanLeg,
    FlightPlanLegKind, FlightPlanValidationReport, FlightPlanValidationStatus, PlanningMode,
    TelemetryUpdate,
};
use openairac_model::{AirportId, CanonicalAirport, TemporalValidity};
use openairac_routing::Coordinate;

fn make_airport(ident: &str, lat: f64, lon: f64, elev: f64, country: &str) -> CanonicalAirport {
    CanonicalAirport {
        id: AirportId(ident.to_lowercase()),
        ident: ident.to_string(),
        name: format!("{} Airport", ident),
        airport_type: "large_airport".to_string(),
        latitude: lat,
        longitude: lon,
        elevation_ft: Some(elev),
        iso_country: Some(country.to_string()),
        municipality: Some("Test City".to_string()),
        runways: Vec::new(),
        temporal: TemporalValidity {
            valid_from: Utc::now(),
            valid_until: None,
            source_snapshot_id: openairac_model::SourceSnapshotId("replay_test".to_string()),
        },
    }
}

#[test]
fn test_replay_e2e_uuee_urff_tu154() {
    let uuee = make_airport("UUEE", 55.9726, 37.4146, 622.0, "RU");
    let urff = make_airport("URFF", 45.0522, 33.9751, 639.0, "RU");

    let leg_dep = FlightPlanLeg {
        leg_index: 0,
        kind: FlightPlanLegKind::AirportConnector,
        from_fix: "UUEE".to_string(),
        to_fix: "EMGAS".to_string(),
        from_coordinate: Some(Coordinate::new(55.9726, 37.4146).unwrap()),
        to_coordinate: Some(Coordinate::new(55.5000, 37.2000).unwrap()),
        distance_nm: 29.0,
        course_true_deg: Some(195.0),
        route_ident: None,
        procedure_ident: Some("EMGAS 3H".to_string()),
        altitude_constraint_str: None,
        speed_constraint_kts: None,
        provider_name: Some("CAICA".to_string()),
        airac_cycle: Some("2608".to_string()),
        source_provenance: None,
    };

    let leg_enroute = FlightPlanLeg {
        leg_index: 1,
        kind: FlightPlanLegKind::AtsRoute,
        from_fix: "EMGAS".to_string(),
        to_fix: "BURUD".to_string(),
        from_coordinate: Some(Coordinate::new(55.5000, 37.2000).unwrap()),
        to_coordinate: Some(Coordinate::new(46.0000, 34.5000).unwrap()),
        distance_nm: 580.0,
        course_true_deg: Some(190.0),
        route_ident: Some("W109".to_string()),
        procedure_ident: None,
        altitude_constraint_str: None,
        speed_constraint_kts: None,
        provider_name: Some("CAICA".to_string()),
        airac_cycle: Some("2608".to_string()),
        source_provenance: None,
    };

    let leg_arr = FlightPlanLeg {
        leg_index: 2,
        kind: FlightPlanLegKind::Approach,
        from_fix: "BURUD".to_string(),
        to_fix: "URFF".to_string(),
        from_coordinate: Some(Coordinate::new(46.0000, 34.5000).unwrap()),
        to_coordinate: Some(Coordinate::new(45.0522, 33.9751).unwrap()),
        distance_nm: 60.0,
        course_true_deg: Some(195.0),
        route_ident: None,
        procedure_ident: Some("ILS 19R".to_string()),
        altitude_constraint_str: None,
        speed_constraint_kts: None,
        provider_name: Some("CAICA".to_string()),
        airac_cycle: Some("2608".to_string()),
        source_provenance: None,
    };

    let plan = FlightPlan {
        flight_id: "AFL1820".to_string(),
        created_at: Utc::now(),
        origin: uuee,
        destination: urff,
        alternates: Vec::new(),
        aircraft_profile: AircraftProfile::tu154(),
        departure_runway: Some("24C".to_string()),
        sid: None,
        sid_transition: None,
        enroute_legs: vec![leg_enroute.clone()],
        all_legs: vec![leg_dep, leg_enroute, leg_arr],
        star: None,
        star_transition: None,
        approach: None,
        arrival_runway: Some("19R".to_string()),
        cruise_altitude_ft: 36000,
        total_distance_nm: 669.0,
        estimated_flight_time_min: 90,
        planning_mode: PlanningMode::StrictAts,
        active_provider_datasets: vec!["CAICA_2608".to_string()],
        validation: FlightPlanValidationReport {
            status: FlightPlanValidationStatus::Valid,
            is_flyable: true,
            issues: Vec::new(),
            warnings: Vec::new(),
            active_providers_at_planning: vec!["CAICA_2608".to_string()],
        },
        diagnostics: Vec::new(),
    };

    let mut session = FlightExecutionSession::new(plan);
    let mut t = Utc::now();

    // 1. Ramp / Preflight
    let telem_gate = TelemetryUpdate {
        timestamp: t,
        latitude_deg: 55.9726,
        longitude_deg: 37.4146,
        altitude_msl_ft: 622.0,
        altitude_agl_ft: Some(0.0),
        groundspeed_kts: 0.0,
        track_true_deg: 240.0,
        vertical_speed_fpm: 0.0,
        on_ground: true,
        paused: false,
        sim_rate: 1.0,
    };
    let p_gate = session.update_telemetry(telem_gate).unwrap();
    assert_eq!(p_gate.current_phase, FlightPhase::Preflight);

    // 2. Climb out
    t += Duration::seconds(120);
    let telem_climb = TelemetryUpdate {
        timestamp: t,
        latitude_deg: 55.7000,
        longitude_deg: 37.3000,
        altitude_msl_ft: 12000.0,
        altitude_agl_ft: Some(11378.0),
        groundspeed_kts: 300.0,
        track_true_deg: 195.0,
        vertical_speed_fpm: 2200.0,
        on_ground: false,
        paused: false,
        sim_rate: 1.0,
    };
    session.update_telemetry(telem_climb.clone()).unwrap();
    session.update_telemetry(telem_climb.clone()).unwrap();
    let p_climb = session.update_telemetry(telem_climb).unwrap();
    assert!(
        p_climb.current_phase == FlightPhase::Climb
            || p_climb.current_phase == FlightPhase::InitialClimb
    );
    // 3. Cruise at FL360
    t += Duration::seconds(1200);
    let telem_cruise = TelemetryUpdate {
        timestamp: t,
        latitude_deg: 51.0000,
        longitude_deg: 35.5000,
        altitude_msl_ft: 36000.0,
        altitude_agl_ft: Some(35400.0),
        groundspeed_kts: 460.0,
        track_true_deg: 190.0,
        vertical_speed_fpm: 0.0,
        on_ground: false,
        paused: false,
        sim_rate: 1.0,
    };
    session.update_telemetry(telem_cruise.clone()).unwrap();
    session.update_telemetry(telem_cruise.clone()).unwrap();
    let p_cruise = session.update_telemetry(telem_cruise).unwrap();
    assert_eq!(p_cruise.current_phase, FlightPhase::Cruise);
    assert!(p_cruise.tod_distance_nm.unwrap() > 0.0);

    // 4. Descent towards URFF
    t += Duration::seconds(800);
    let telem_descent = TelemetryUpdate {
        timestamp: t,
        latitude_deg: 46.5000,
        longitude_deg: 34.6000,
        altitude_msl_ft: 18000.0,
        altitude_agl_ft: Some(17361.0),
        groundspeed_kts: 340.0,
        track_true_deg: 195.0,
        vertical_speed_fpm: -1800.0,
        on_ground: false,
        paused: false,
        sim_rate: 1.0,
    };
    session.update_telemetry(telem_descent.clone()).unwrap();
    session.update_telemetry(telem_descent.clone()).unwrap();
    let p_desc = session.update_telemetry(telem_descent).unwrap();
    assert_eq!(p_desc.current_phase, FlightPhase::Descent);
    assert!(p_desc.descent_profile_deviation_ft.is_some());

    // 5. Landing at URFF
    t += Duration::seconds(400);
    let telem_land = TelemetryUpdate {
        timestamp: t,
        latitude_deg: 45.0522,
        longitude_deg: 33.9751,
        altitude_msl_ft: 640.0,
        altitude_agl_ft: Some(1.0),
        groundspeed_kts: 120.0,
        track_true_deg: 195.0,
        vertical_speed_fpm: -100.0,
        on_ground: true,
        paused: false,
        sim_rate: 1.0,
    };
    let p_land = session.update_telemetry(telem_land).unwrap();
    assert_eq!(p_land.current_phase, FlightPhase::Landing);

    let log = session.complete_flight("SUCCESS_LANDED");
    assert_eq!(log.origin_icao, "UUEE");
    assert_eq!(log.destination_icao, "URFF");
    assert_eq!(log.aircraft_ident, "T154");
}

#[test]
fn test_replay_e2e_urss_uras_an24_source_required_arrival() {
    let urss = make_airport("URSS", 43.4444, 39.9566, 89.0, "RU");
    let uras = make_airport("URAS", 42.8583, 41.1283, 53.0, "ABKHAZIA");

    let leg_dep = FlightPlanLeg {
        leg_index: 0,
        kind: FlightPlanLegKind::Sid,
        from_fix: "URSS".to_string(),
        to_fix: "ADNET".to_string(),
        from_coordinate: Some(Coordinate::new(43.4444, 39.9566).unwrap()),
        to_coordinate: Some(Coordinate::new(43.2000, 40.2000).unwrap()),
        distance_nm: 22.0,
        course_true_deg: Some(140.0),
        route_ident: None,
        procedure_ident: Some("ADNET 1D".to_string()),
        altitude_constraint_str: None,
        speed_constraint_kts: None,
        provider_name: Some("CAICA".to_string()),
        airac_cycle: Some("2608".to_string()),
        source_provenance: None,
    };

    let leg_enroute = FlightPlanLeg {
        leg_index: 1,
        kind: FlightPlanLegKind::AtsRoute,
        from_fix: "ADNET".to_string(),
        to_fix: "TABES".to_string(),
        from_coordinate: Some(Coordinate::new(43.2000, 40.2000).unwrap()),
        to_coordinate: Some(Coordinate::new(42.9500, 40.9000).unwrap()),
        distance_nm: 35.0,
        course_true_deg: Some(125.0),
        route_ident: Some("G247".to_string()),
        procedure_ident: None,
        altitude_constraint_str: None,
        speed_constraint_kts: None,
        provider_name: Some("CAICA".to_string()),
        airac_cycle: Some("2608".to_string()),
        source_provenance: None,
    };

    let leg_arr_connector = FlightPlanLeg {
        leg_index: 2,
        kind: FlightPlanLegKind::AirportConnector,
        from_fix: "TABES".to_string(),
        to_fix: "URAS".to_string(),
        from_coordinate: Some(Coordinate::new(42.9500, 40.9000).unwrap()),
        to_coordinate: Some(Coordinate::new(42.8583, 41.1283).unwrap()),
        distance_nm: 12.0,
        course_true_deg: Some(120.0),
        route_ident: None,
        procedure_ident: None,
        altitude_constraint_str: None,
        speed_constraint_kts: None,
        provider_name: None,
        airac_cycle: None,
        source_provenance: None,
    };

    let plan = FlightPlan {
        flight_id: "SU9102".to_string(),
        created_at: Utc::now(),
        origin: urss,
        destination: uras,
        alternates: Vec::new(),
        aircraft_profile: AircraftProfile::turboprop(),
        departure_runway: Some("24".to_string()),
        sid: None,
        sid_transition: None,
        enroute_legs: vec![leg_enroute.clone()],
        all_legs: vec![leg_dep, leg_enroute, leg_arr_connector],
        star: None, // 0 fabricated STAR
        star_transition: None,
        approach: None, // 0 fabricated Approach (SOURCE_REQUIRED)
        arrival_runway: Some("12".to_string()),
        cruise_altitude_ft: 15000,
        total_distance_nm: 69.0,
        estimated_flight_time_min: 25,
        planning_mode: PlanningMode::StrictAts,
        active_provider_datasets: vec!["CAICA_2608".to_string()],
        validation: FlightPlanValidationReport {
            status: FlightPlanValidationStatus::Valid,
            is_flyable: true,
            issues: Vec::new(),
            warnings: vec!["URAS terminal procedures SOURCE_REQUIRED".to_string()],
            active_providers_at_planning: vec!["CAICA_2608".to_string()],
        },
        diagnostics: Vec::new(),
    };

    let mut session = FlightExecutionSession::new(plan);
    assert!(session.flight_plan.star.is_none());
    assert!(session.flight_plan.approach.is_none());

    let telem_app = TelemetryUpdate {
        timestamp: Utc::now(),
        latitude_deg: 42.9000,
        longitude_deg: 41.0500,
        altitude_msl_ft: 3000.0,
        altitude_agl_ft: Some(2947.0),
        groundspeed_kts: 180.0,
        track_true_deg: 120.0,
        vertical_speed_fpm: -600.0,
        on_ground: false,
        paused: false,
        sim_rate: 1.0,
    };
    let progress = session.update_telemetry(telem_app).unwrap();
    // Verify no fabricated STAR was auto-invented during flight
    assert!(session.flight_plan.star.is_none());
    assert!(session.flight_plan.approach.is_none());
    assert!(progress.procedure_context.contains("ATS") || progress.procedure_context == "AIRPORT");
}

#[test]
fn test_replay_e2e_lfpg_lfmn_a320() {
    let lfpg = make_airport("LFPG", 49.0097, 2.5479, 392.0, "FR");
    let lfmn = make_airport("LFMN", 43.6584, 7.2159, 12.0, "FR");

    let leg_dep = FlightPlanLeg {
        leg_index: 0,
        kind: FlightPlanLegKind::Sid,
        from_fix: "LFPG".to_string(),
        to_fix: "OPALE".to_string(),
        from_coordinate: Some(Coordinate::new(49.0097, 2.5479).unwrap()),
        to_coordinate: Some(Coordinate::new(48.5000, 3.0000).unwrap()),
        distance_nm: 35.0,
        course_true_deg: Some(150.0),
        route_ident: None,
        procedure_ident: Some("OPALE 5A".to_string()),
        altitude_constraint_str: None,
        speed_constraint_kts: None,
        provider_name: Some("SIA_FRANCE".to_string()),
        airac_cycle: Some("2608".to_string()),
        source_provenance: None,
    };

    let leg_enroute = FlightPlanLeg {
        leg_index: 1,
        kind: FlightPlanLegKind::AtsRoute,
        from_fix: "OPALE".to_string(),
        to_fix: "RIVIE".to_string(),
        from_coordinate: Some(Coordinate::new(48.5000, 3.0000).unwrap()),
        to_coordinate: Some(Coordinate::new(44.0000, 7.0000).unwrap()),
        distance_nm: 380.0,
        course_true_deg: Some(155.0),
        route_ident: Some("UN853".to_string()),
        procedure_ident: None,
        altitude_constraint_str: None,
        speed_constraint_kts: None,
        provider_name: Some("SIA_FRANCE".to_string()),
        airac_cycle: Some("2608".to_string()),
        source_provenance: None,
    };

    let plan = FlightPlan {
        flight_id: "AFR6122".to_string(),
        created_at: Utc::now(),
        origin: lfpg,
        destination: lfmn,
        alternates: Vec::new(),
        aircraft_profile: AircraftProfile::a320_narrowbody(),
        departure_runway: Some("26L".to_string()),
        sid: None,
        sid_transition: None,
        enroute_legs: vec![leg_enroute.clone()],
        all_legs: vec![leg_dep, leg_enroute],
        star: None,
        star_transition: None,
        approach: None,
        arrival_runway: Some("04L".to_string()),
        cruise_altitude_ft: 33000,
        total_distance_nm: 415.0,
        estimated_flight_time_min: 65,
        planning_mode: PlanningMode::StrictAts,
        active_provider_datasets: vec!["SIA_FRANCE_2608".to_string()],
        validation: FlightPlanValidationReport {
            status: FlightPlanValidationStatus::Valid,
            is_flyable: true,
            issues: Vec::new(),
            warnings: Vec::new(),
            active_providers_at_planning: vec!["SIA_FRANCE_2608".to_string()],
        },
        diagnostics: Vec::new(),
    };

    let mut session = FlightExecutionSession::new(plan);
    let telem = TelemetryUpdate {
        timestamp: Utc::now(),
        latitude_deg: 46.0000,
        longitude_deg: 5.0000,
        altitude_msl_ft: 33000.0,
        altitude_agl_ft: Some(32000.0),
        groundspeed_kts: 440.0,
        track_true_deg: 155.0,
        vertical_speed_fpm: 0.0,
        on_ground: false,
        paused: false,
        sim_rate: 1.0,
    };
    session.update_telemetry(telem.clone()).unwrap();
    session.update_telemetry(telem.clone()).unwrap();
    let p = session.update_telemetry(telem).unwrap();
    assert_eq!(p.current_phase, FlightPhase::Cruise);
    assert!(p.remaining_route_distance_nm > 50.0);
}
