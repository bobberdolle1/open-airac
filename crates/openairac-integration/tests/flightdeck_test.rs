//! Integration and unit tests for OpenAIRAC FlightdeckOS & AI Crew Integration.

use chrono::{Duration, Utc};
use openairac_integration::execution::{
    FlightExecutionSession, FlightPhase, FlightProgress, TelemetryUpdate,
};
use openairac_integration::flightdeck::{
    AdvisoryLevel, COMPACT_AI_SNAPSHOT_SCHEMA_V1, CrewAdvisoryEngine,
    FLIGHTDECK_SNAPSHOT_SCHEMA_V2, FlightEventStream, FlightEventType, FlightStateDeltaDetector,
    FlightdeckOsAdapter, FlightdeckRunwayWind, FlightdeckToolRegistry, FlightdeckWeatherSummary,
};
use openairac_integration::{
    FlightPlan, FlightPlanLeg, FlightPlanLegKind, FlightPlanValidationReport,
    FlightPlanValidationStatus, PlanningMode,
};
use openairac_model::{AirportId, CanonicalAirport, SourceSnapshotId, TemporalValidity};
use openairac_routing::Coordinate;
use openairac_routing::random_flight::AircraftProfile;

fn make_airport(ident: &str, name: &str, lat: f64, lon: f64, elev: f64) -> CanonicalAirport {
    CanonicalAirport {
        id: AirportId(format!("apt-{}", ident.to_lowercase())),
        ident: ident.to_string(),
        name: name.to_string(),
        airport_type: "large_airport".to_string(),
        latitude: lat,
        longitude: lon,
        elevation_ft: Some(elev),
        iso_country: Some("RU".to_string()),
        municipality: Some(name.to_string()),
        runways: Vec::new(),
        temporal: TemporalValidity {
            valid_from: Utc::now() - Duration::days(10),
            valid_until: None,
            source_snapshot_id: SourceSnapshotId("snap-test".to_string()),
        },
    }
}

fn create_test_flight_plan(origin_ident: &str, dest_ident: &str) -> FlightPlan {
    let origin = make_airport(origin_ident, "Origin Airport", 55.97, 37.41, 623.0);
    let dest = make_airport(dest_ident, "Destination Airport", 45.05, 33.98, 639.0);

    let legs = vec![
        FlightPlanLeg {
            leg_index: 0,
            kind: FlightPlanLegKind::Sid,
            from_fix: origin_ident.to_string(),
            to_fix: "EMGAS".to_string(),
            from_coordinate: Some(Coordinate::new(55.97, 37.41).unwrap()),
            to_coordinate: Some(Coordinate::new(55.20, 37.80).unwrap()),
            distance_nm: 48.0,
            course_true_deg: Some(160.0),
            route_ident: None,
            procedure_ident: Some("EMGAS3H".to_string()),
            altitude_constraint_str: Some("FL120".to_string()),
            speed_constraint_kts: Some(250),
            provider_name: Some("CAICA".to_string()),
            airac_cycle: Some("2608".to_string()),
            source_provenance: None,
        },
        FlightPlanLeg {
            leg_index: 1,
            kind: FlightPlanLegKind::AtsRoute,
            from_fix: "EMGAS".to_string(),
            to_fix: "BURUD".to_string(),
            from_coordinate: Some(Coordinate::new(55.20, 37.80).unwrap()),
            to_coordinate: Some(Coordinate::new(46.00, 34.50).unwrap()),
            distance_nm: 560.0,
            course_true_deg: Some(195.0),
            route_ident: Some("W109".to_string()),
            procedure_ident: None,
            altitude_constraint_str: None,
            speed_constraint_kts: None,
            provider_name: Some("CAICA".to_string()),
            airac_cycle: Some("2608".to_string()),
            source_provenance: None,
        },
        FlightPlanLeg {
            leg_index: 2,
            kind: FlightPlanLegKind::Star,
            from_fix: "BURUD".to_string(),
            to_fix: dest_ident.to_string(),
            from_coordinate: Some(Coordinate::new(46.00, 34.50).unwrap()),
            to_coordinate: Some(Coordinate::new(45.05, 33.98).unwrap()),
            distance_nm: 65.0,
            course_true_deg: Some(210.0),
            route_ident: None,
            procedure_ident: Some("BURUD2Y".to_string()),
            altitude_constraint_str: Some("3000".to_string()),
            speed_constraint_kts: Some(210),
            provider_name: Some("CAICA".to_string()),
            airac_cycle: Some("2608".to_string()),
            source_provenance: None,
        },
    ];

    let mut diagnostics = Vec::new();
    if dest_ident == "URAS" {
        diagnostics.push(
            "URAS terminal procedures: SOURCE_REQUIRED (CAICA official AIP required)".to_string(),
        );
    }

    FlightPlan {
        flight_id: format!("{}-{}", origin_ident, dest_ident),
        created_at: Utc::now(),
        origin,
        destination: dest,
        alternates: Vec::new(),
        aircraft_profile: AircraftProfile::b747_class(),
        departure_runway: Some("24C".to_string()),
        sid: None,
        sid_transition: None,
        enroute_legs: vec![legs[1].clone()],
        all_legs: legs,
        star: None,
        star_transition: None,
        approach: None,
        arrival_runway: Some("19R".to_string()),
        cruise_altitude_ft: 36000,
        total_distance_nm: 673.0,
        estimated_flight_time_min: 95,
        planning_mode: PlanningMode::StrictAts,
        active_provider_datasets: vec!["CAICA".to_string(), "WORLD_OPEN".to_string()],
        validation: FlightPlanValidationReport {
            status: FlightPlanValidationStatus::Valid,
            is_flyable: true,
            issues: Vec::new(),
            warnings: Vec::new(),
            active_providers_at_planning: vec!["CAICA".to_string()],
        },
        diagnostics,
    }
}

#[test]
fn test_flightdeck_snapshot_v2_and_compact_ai_snapshot() {
    let plan = create_test_flight_plan("UUEE", "URFF");
    let mut session = FlightExecutionSession::new(plan);

    // Initial telem in Cruise
    let telem = TelemetryUpdate {
        timestamp: Utc::now(),
        latitude_deg: 52.41,
        longitude_deg: 37.89,
        altitude_msl_ft: 36000.0,
        altitude_agl_ft: Some(35400.0),
        groundspeed_kts: 460.0,
        track_true_deg: 195.0,
        vertical_speed_fpm: 0.0,
        on_ground: false,
        paused: false,
        sim_rate: 1.0,
    };

    let progress = session.update_telemetry(telem).unwrap();

    let wx = FlightdeckWeatherSummary {
        origin_metar: Some("UUEE 24008KT 9999 FEW040 18/10 Q1018".to_string()),
        destination_metar: Some("URFF 19012KT 9999 SCT030 22/14 Q1013".to_string()),
        destination_runway_wind: Some(FlightdeckRunwayWind {
            runway_ident: "19R".to_string(),
            headwind_kts: 12.0,
            crosswind_kts: 0.0,
            is_tailwind: false,
            is_recommended: true,
        }),
        ..Default::default()
    };

    let snap_v2 = session.snapshot_v2(
        Some(&progress),
        Some(wx),
        Vec::new(),
        Some("X-Plane 12 UDP"),
    );

    // Verify Snapshot v2 Schema and fields
    assert_eq!(snap_v2.schema_version, FLIGHTDECK_SNAPSHOT_SCHEMA_V2);
    assert_eq!(snap_v2.origin.ident, "UUEE");
    assert_eq!(snap_v2.destination.ident, "URFF");
    assert!(!snap_v2.destination.is_source_required);
    assert_eq!(snap_v2.aircraft.cruise_altitude_ft, 36000);
    assert!(snap_v2.position.is_some());
    assert_eq!(snap_v2.position.as_ref().unwrap().altitude_msl_ft, 36000.0);

    // Test JSON serialization of Snapshot v2
    let json_val = serde_json::to_value(&snap_v2).unwrap();
    assert_eq!(json_val["schema_version"], "flightdeck_snapshot_v2");
    assert_eq!(json_val["origin"]["ident"], "UUEE");

    // Convert to Compact AI Snapshot
    let compact = snap_v2.to_compact();
    assert_eq!(compact.schema_version, COMPACT_AI_SNAPSHOT_SCHEMA_V1);
    assert_eq!(compact.flight, "UUEE -> URFF");
    assert!(compact.position.contains("36000 ft MSL"));
    assert!(compact.destination_weather.contains("URFF 19012KT"));
    assert!(compact.provenance.contains("CAICA"));

    // Test translation to FlightdeckOS Context
    let os_ctx = FlightdeckOsAdapter::translate(&snap_v2);
    assert_eq!(os_ctx.flightdeck_os_version, "1.0");
    assert_eq!(os_ctx.destination_brief.icao, "URFF");
    assert!(!os_ctx.destination_brief.is_source_required);
}

#[test]
fn test_crew_advisory_engine_rules() {
    let plan = create_test_flight_plan("UUEE", "URFF");
    let session = FlightExecutionSession::new(plan);

    // 1. TOD Approaching rule
    let mut progress = FlightProgress {
        active_leg_index: 1,
        prev_fix: Some("EMGAS".to_string()),
        active_leg_name: "EMGAS -> BURUD".to_string(),
        next_fix: Some("BURUD".to_string()),
        xtk_nm: 0.2,
        desired_track_deg: 195.0,
        track_error_deg: 0.0,
        distance_to_next_fix_nm: 84.0,
        remaining_route_distance_nm: 385.0,
        direct_distance_to_destination_nm: 400.0,
        ete_next_fix_sec: Some(650),
        eta_next_fix: None,
        ete_destination_sec: Some(3000),
        eta_destination: None,
        current_phase: FlightPhase::Cruise,
        phase_evidence: "Cruise".to_string(),
        procedure_context: "W109".to_string(),
        tod_distance_nm: Some(12.0), // <= 15 NM -> TOD_APPROACHING
        tod_coordinate: None,
        descent_profile_deviation_ft: Some(0.0),
        required_descent_rate_fpm: Some(-1850.0),
        is_off_route: false,
        telemetry_stale: false,
        sim_connected: true,
    };

    let advs = CrewAdvisoryEngine::evaluate(&session, Some(&progress), None, &[]);
    assert!(
        advs.iter()
            .any(|a| a.code == "TOD_APPROACHING" && a.level == AdvisoryLevel::Caution)
    );

    // 2. Off route rule
    progress.is_off_route = true;
    progress.xtk_nm = 6.5;
    let advs_off = CrewAdvisoryEngine::evaluate(&session, Some(&progress), None, &[]);
    assert!(
        advs_off
            .iter()
            .any(|a| a.code == "OFF_ROUTE" && a.level == AdvisoryLevel::Warning)
    );

    // 3. Tailwind rule
    let wx_tailwind = FlightdeckWeatherSummary {
        destination_runway_wind: Some(FlightdeckRunwayWind {
            runway_ident: "19R".to_string(),
            headwind_kts: -8.0, // 8 kt tailwind
            crosswind_kts: 2.0,
            is_tailwind: true,
            is_recommended: false,
        }),
        ..Default::default()
    };
    let advs_wind =
        CrewAdvisoryEngine::evaluate(&session, Some(&progress), Some(&wx_tailwind), &[]);
    assert!(
        advs_wind
            .iter()
            .any(|a| a.code == "SIGNIFICANT_TAILWIND" && a.level == AdvisoryLevel::Caution)
    );
}

#[test]
fn test_flight_event_stream_ring_buffer() {
    let mut stream = FlightEventStream::new(4);
    assert_eq!(stream.len(), 0);

    let id1 = stream.push(
        FlightEventType::SimConnected,
        "Simulator connected",
        serde_json::json!({}),
    );
    let _id2 = stream.push(
        FlightEventType::Takeoff,
        "Takeoff rotation detected",
        serde_json::json!({}),
    );
    let _id3 = stream.push(
        FlightEventType::PhaseChanged,
        "Entered climb",
        serde_json::json!({}),
    );
    let id4 = stream.push(
        FlightEventType::FixSequenced,
        "Sequenced EMGAS",
        serde_json::json!({}),
    );
    assert_eq!(stream.len(), 4);
    assert_eq!(id1, 1);
    assert_eq!(id4, 4);

    // Push 5th event -> evicts 1st event
    let id5 = stream.push(
        FlightEventType::TodReached,
        "Reached TOD",
        serde_json::json!({}),
    );
    assert_eq!(id5, 5);
    assert_eq!(stream.len(), 4);

    // Query events with since_id
    let evts = stream.get_events(Some(2), 10);
    assert_eq!(evts.len(), 3);
    assert_eq!(evts[0].id, 3);
    assert_eq!(evts[1].id, 4);
    assert_eq!(evts[2].id, 5);
}

#[test]
fn test_flight_state_delta_detector() {
    let plan = create_test_flight_plan("UUEE", "URFF");
    let session = FlightExecutionSession::new(plan);

    let snap1 = session.snapshot_v2(None, None, Vec::new(), None);
    let mut snap2 = snap1.clone();

    snap2.flight_phase = FlightPhase::Climb;
    snap2.navigation_geometry.is_off_route = true;

    let delta = FlightStateDeltaDetector::compute_delta(&snap1, &snap2);
    assert!(delta.phase_changed.is_some());
    assert_eq!(delta.off_route_changed, Some(true));
}

#[test]
fn test_airport_multi_identity_and_source_required_canary() {
    // 1. Sukhumi URAS / UGSS / SUI Multi-identity resolution
    let identity_uras = FlightdeckToolRegistry::resolve_airport_identity("URAS");
    assert_eq!(identity_uras.authoritative_ident, "URAS");
    assert_eq!(identity_uras.iata_code, Some("SUI".to_string()));
    assert_eq!(identity_uras.primary_provider, "CAICA");
    assert_eq!(identity_uras.terminal_procedures_status, "SOURCE_REQUIRED");
    assert!(
        identity_uras
            .alternate_identities
            .iter()
            .any(|alt| alt.ident == "UGSS")
    );

    let identity_ugss = FlightdeckToolRegistry::resolve_airport_identity("UGSS");
    assert_eq!(identity_ugss.authoritative_ident, "URAS");
    assert_eq!(identity_ugss.terminal_procedures_status, "SOURCE_REQUIRED");

    // 2. Strict SOURCE_REQUIRED arrival brief verification
    let plan_uras = create_test_flight_plan("URSS", "URAS");
    let session_uras = FlightExecutionSession::new(plan_uras);
    let snap_uras = session_uras.snapshot_v2(None, None, Vec::new(), None);

    assert!(snap_uras.destination.is_source_required);
    let arr_brief = FlightdeckToolRegistry::get_arrival_brief(&snap_uras).unwrap();
    assert!(arr_brief.is_source_required);
    assert!(arr_brief.star_procedure.is_none());
    assert!(arr_brief.approach_procedure.is_none());
    assert!(arr_brief.briefing_text.contains("SOURCE_REQUIRED"));
}
