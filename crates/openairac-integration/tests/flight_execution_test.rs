use chrono::{Duration, Utc};
use openairac_integration::{
    AircraftProfile, FlightExecutionSession, FlightPhase, FlightPlan, FlightPlanLeg,
    FlightPlanLegKind, FlightPlanValidationReport, FlightPlanValidationStatus, PlanningMode,
    TelemetryUpdate,
};
use openairac_model::{AirportId, CanonicalAirport, TemporalValidity};
use openairac_routing::Coordinate;

fn sample_airport(ident: &str, lat: f64, lon: f64, elev: f64) -> CanonicalAirport {
    CanonicalAirport {
        id: AirportId(ident.to_lowercase()),
        ident: ident.to_string(),
        name: format!("{} Airport", ident),
        airport_type: "large_airport".to_string(),
        latitude: lat,
        longitude: lon,
        elevation_ft: Some(elev),
        iso_country: Some("RU".to_string()),
        municipality: Some("Test".to_string()),
        runways: Vec::new(),
        temporal: TemporalValidity {
            valid_from: Utc::now(),
            valid_until: None,
            source_snapshot_id: openairac_model::SourceSnapshotId("test".to_string()),
        },
    }
}

fn sample_flight_plan() -> FlightPlan {
    let origin = sample_airport("UUEE", 55.9726, 37.4146, 622.0);
    let dest = sample_airport("URFF", 45.0522, 33.9751, 639.0);

    let leg1 = FlightPlanLeg {
        leg_index: 0,
        kind: FlightPlanLegKind::AirportConnector,
        from_fix: "UUEE".to_string(),
        to_fix: "EMGAS".to_string(),
        from_coordinate: Some(Coordinate {
            latitude_deg: 55.9726,
            longitude_deg: 37.4146,
        }),
        to_coordinate: Some(Coordinate {
            latitude_deg: 55.5000,
            longitude_deg: 37.2000,
        }),
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

    let leg2 = FlightPlanLeg {
        leg_index: 1,
        kind: FlightPlanLegKind::AtsRoute,
        from_fix: "EMGAS".to_string(),
        to_fix: "ROTLI".to_string(),
        from_coordinate: Some(Coordinate {
            latitude_deg: 55.5000,
            longitude_deg: 37.2000,
        }),
        to_coordinate: Some(Coordinate {
            latitude_deg: 52.0000,
            longitude_deg: 36.0000,
        }),
        distance_nm: 215.0,
        course_true_deg: Some(190.0),
        route_ident: Some("W109".to_string()),
        procedure_ident: None,
        altitude_constraint_str: None,
        speed_constraint_kts: None,
        provider_name: Some("CAICA".to_string()),
        airac_cycle: Some("2608".to_string()),
        source_provenance: None,
    };

    let leg3 = FlightPlanLeg {
        leg_index: 2,
        kind: FlightPlanLegKind::AirportConnector,
        from_fix: "ROTLI".to_string(),
        to_fix: "URFF".to_string(),
        from_coordinate: Some(Coordinate {
            latitude_deg: 52.0000,
            longitude_deg: 36.0000,
        }),
        to_coordinate: Some(Coordinate {
            latitude_deg: 45.0522,
            longitude_deg: 33.9751,
        }),
        distance_nm: 420.0,
        course_true_deg: Some(195.0),
        route_ident: None,
        procedure_ident: None,
        altitude_constraint_str: None,
        speed_constraint_kts: None,
        provider_name: Some("CAICA".to_string()),
        airac_cycle: Some("2608".to_string()),
        source_provenance: None,
    };

    FlightPlan {
        flight_id: "AFL1820".to_string(),
        created_at: Utc::now(),
        origin,
        destination: dest,
        alternates: Vec::new(),
        aircraft_profile: AircraftProfile::tu154(),
        departure_runway: Some("24C".to_string()),
        sid: None,
        sid_transition: None,
        enroute_legs: vec![leg2.clone()],
        all_legs: vec![leg1, leg2, leg3],
        star: None,
        star_transition: None,
        approach: None,
        arrival_runway: Some("19R".to_string()),
        cruise_altitude_ft: 36000,
        total_distance_nm: 664.0,
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
    }
}

#[test]
fn test_flight_execution_session_lifecycle() {
    let plan = sample_flight_plan();
    let mut session = FlightExecutionSession::new(plan);

    assert_eq!(session.active_leg_index, 0);
    assert_eq!(session.phase_engine.current_phase(), FlightPhase::Preflight);
    assert!(!session.is_connected);

    // 1. Initial Telemetry at Departure Gate
    let mut t0 = Utc::now();
    let telem0 = TelemetryUpdate {
        timestamp: t0,
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

    let p0 = session.update_telemetry(telem0).unwrap();
    assert_eq!(p0.current_phase, FlightPhase::Preflight);
    assert_eq!(p0.active_leg_index, 0);
    assert!(p0.ete_destination_sec.is_none()); // Low GS -> None
    assert!(session.is_connected);

    // 2. Taxi Out
    t0 += Duration::seconds(30);
    let telem_taxi = TelemetryUpdate {
        timestamp: t0,
        latitude_deg: 55.9700,
        longitude_deg: 37.4100,
        altitude_msl_ft: 622.0,
        altitude_agl_ft: Some(0.0),
        groundspeed_kts: 18.0,
        track_true_deg: 240.0,
        vertical_speed_fpm: 0.0,
        on_ground: true,
        paused: false,
        sim_rate: 1.0,
    };
    // 2 consecutive ticks for TaxiOut
    session.update_telemetry(telem_taxi.clone()).unwrap();
    let p_taxi = session.update_telemetry(telem_taxi).unwrap();
    assert_eq!(p_taxi.current_phase, FlightPhase::TaxiOut);

    // 3. Takeoff Roll
    t0 += Duration::seconds(20);
    let telem_to = TelemetryUpdate {
        timestamp: t0,
        latitude_deg: 55.9650,
        longitude_deg: 37.4000,
        altitude_msl_ft: 630.0,
        altitude_agl_ft: Some(8.0),
        groundspeed_kts: 140.0,
        track_true_deg: 242.0,
        vertical_speed_fpm: 0.0,
        on_ground: true,
        paused: false,
        sim_rate: 1.0,
    };
    let p_to = session.update_telemetry(telem_to).unwrap();
    assert_eq!(p_to.current_phase, FlightPhase::Takeoff);

    // 4. Initial Climb
    t0 += Duration::seconds(15);
    let telem_climb = TelemetryUpdate {
        timestamp: t0,
        latitude_deg: 55.9000,
        longitude_deg: 37.3500,
        altitude_msl_ft: 2000.0,
        altitude_agl_ft: Some(1378.0),
        groundspeed_kts: 220.0,
        track_true_deg: 195.0,
        vertical_speed_fpm: 2500.0,
        on_ground: false,
        paused: false,
        sim_rate: 1.0,
    };
    let p_iclimb = session.update_telemetry(telem_climb).unwrap();
    assert!(
        p_iclimb.current_phase == FlightPhase::InitialClimb
            || p_iclimb.current_phase == FlightPhase::Climb
    );

    // 5. Cruise at FL360
    t0 += Duration::seconds(600);
    let telem_cruise = TelemetryUpdate {
        timestamp: t0,
        latitude_deg: 53.0000,
        longitude_deg: 36.5000,
        altitude_msl_ft: 36000.0,
        altitude_agl_ft: Some(35400.0),
        groundspeed_kts: 460.0,
        track_true_deg: 190.0,
        vertical_speed_fpm: 0.0,
        on_ground: false,
        paused: false,
        sim_rate: 1.0,
    };
    // 3 ticks for cruise hysteresis
    session.update_telemetry(telem_cruise.clone()).unwrap();
    session.update_telemetry(telem_cruise.clone()).unwrap();
    let p_cruise = session.update_telemetry(telem_cruise).unwrap();
    assert_eq!(p_cruise.current_phase, FlightPhase::Cruise);
    assert!(p_cruise.ete_destination_sec.is_some());
    assert!(p_cruise.tod_distance_nm.is_some());

    // 6. Direct To ROTLI
    session.activate_direct_to("ROTLI").unwrap();
    assert_eq!(session.active_leg_index, 1);

    // 7. Disconnect / Reconnect Resilience
    session.handle_disconnect();
    assert!(!session.is_connected);
    assert_eq!(session.active_leg_index, 1); // Plan preserved

    session.handle_reconnect();
    assert!(session.is_connected);

    // 8. Flight Completion Log
    let log = session.complete_flight("COMPLETED_LANDED");
    assert_eq!(log.origin_icao, "UUEE");
    assert_eq!(log.destination_icao, "URFF");
    assert_eq!(log.aircraft_ident, "T154");
    assert_eq!(log.completion_status, "COMPLETED_LANDED");

    // 9. Snapshot
    let snap = session.snapshot();
    assert_eq!(snap["origin"], "UUEE");
    assert_eq!(snap["destination"], "URFF");
}

#[test]
fn test_phase_hysteresis_anti_flapping() {
    let mut engine = openairac_integration::FlightPhaseEngine::new(600.0, 50.0, 35000.0);

    // Establish cruise
    let t0 = Utc::now();
    let telem_cruise = TelemetryUpdate {
        timestamp: t0,
        latitude_deg: 50.0,
        longitude_deg: 30.0,
        altitude_msl_ft: 35000.0,
        altitude_agl_ft: Some(34000.0),
        groundspeed_kts: 450.0,
        track_true_deg: 180.0,
        vertical_speed_fpm: 0.0,
        on_ground: false,
        paused: false,
        sim_rate: 1.0,
    };

    // 3 ticks to establish Cruise
    engine.process_telemetry(&telem_cruise, 300.0);
    engine.process_telemetry(&telem_cruise, 300.0);
    let (p, _) = engine.process_telemetry(&telem_cruise, 300.0);
    assert_eq!(p, FlightPhase::Cruise);

    // Momentary downdraft (VS = -800 FPM for 1 tick)
    let telem_glitch = TelemetryUpdate {
        vertical_speed_fpm: -800.0,
        altitude_msl_ft: 34950.0,
        ..telem_cruise.clone()
    };
    let (p_glitch1, _) = engine.process_telemetry(&telem_glitch, 290.0);
    assert_eq!(p_glitch1, FlightPhase::Cruise); // Must NOT immediately flap to Descent!

    // Return to level flight
    let (p_recovered, _) = engine.process_telemetry(&telem_cruise, 280.0);
    assert_eq!(p_recovered, FlightPhase::Cruise);
}

#[test]
fn test_cross_track_deviation_and_off_route() {
    let plan = sample_flight_plan();
    let mut session = FlightExecutionSession::new(plan);

    // Leg 1 is UUEE (55.9726, 37.4146) to EMGAS (55.5000, 37.2000)
    let mid_lat = (55.9726 + 55.5000) / 2.0;
    let mid_lon = (37.4146 + 37.2000) / 2.0;

    let telem_on_track = TelemetryUpdate {
        timestamp: Utc::now(),
        latitude_deg: mid_lat,
        longitude_deg: mid_lon,
        altitude_msl_ft: 15000.0,
        altitude_agl_ft: Some(14500.0),
        groundspeed_kts: 300.0,
        track_true_deg: 195.0,
        vertical_speed_fpm: 0.0,
        on_ground: false,
        paused: false,
        sim_rate: 1.0,
    };
    let p_on = session.update_telemetry(telem_on_track).unwrap();
    assert!(p_on.xtk_nm.abs() < 0.1);
    assert!(!p_on.is_off_route);

    // Deviate significantly to the right (> 5 NM off track)
    let telem_off_track = TelemetryUpdate {
        timestamp: Utc::now(),
        latitude_deg: mid_lat,
        longitude_deg: mid_lon + 0.5, // ~17 NM east (right of southward track)
        altitude_msl_ft: 15000.0,
        altitude_agl_ft: Some(14500.0),
        groundspeed_kts: 300.0,
        track_true_deg: 195.0,
        vertical_speed_fpm: 0.0,
        on_ground: false,
        paused: false,
        sim_rate: 1.0,
    };
    let p_off = session.update_telemetry(telem_off_track).unwrap();
    assert!(p_off.xtk_nm.abs() > 5.0);
    assert!(p_off.is_off_route);
}

#[test]
fn test_telemetry_failure_modes() {
    let plan = sample_flight_plan();
    let mut session = FlightExecutionSession::new(plan);

    // 1. NaN coordinate rejection
    let telem_nan = TelemetryUpdate {
        timestamp: Utc::now(),
        latitude_deg: f64::NAN,
        longitude_deg: 37.4146,
        altitude_msl_ft: 622.0,
        altitude_agl_ft: None,
        groundspeed_kts: 0.0,
        track_true_deg: 0.0,
        vertical_speed_fpm: 0.0,
        on_ground: true,
        paused: false,
        sim_rate: 1.0,
    };
    assert!(session.update_telemetry(telem_nan).is_err());

    // 2. Out of bounds coordinate rejection
    let telem_oob = TelemetryUpdate {
        timestamp: Utc::now(),
        latitude_deg: 95.0,
        longitude_deg: 37.4146,
        altitude_msl_ft: 622.0,
        altitude_agl_ft: None,
        groundspeed_kts: 0.0,
        track_true_deg: 0.0,
        vertical_speed_fpm: 0.0,
        on_ground: true,
        paused: false,
        sim_rate: 1.0,
    };
    assert!(session.update_telemetry(telem_oob).is_err());
}
