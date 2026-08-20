//! Acceptance & Unit Tests for EFB Automation, Flight Phase Engine, and Geospatial Rasters.

use chrono::{Duration, TimeZone, Utc};
use openairac_charts::efb::{
    AircraftTelemetry, ChartSuggestion, FlightPhase, FlightPhaseEngine, calculate_cross_track_nm,
    calculate_planning_tod_nm, calculate_runway_wind_components,
};
use openairac_charts::georaster::{AffineTransform, GeoBounds, GeoRasterAsset};
use openairac_charts::model::{GeoreferenceStatus, NormalizedChartType};

#[test]
fn test_affine_transform_roundtrip() {
    // New York Sectional raster calibration example
    // Scale: ~0.0005 deg/pixel, Origin: (-76.0 lon, 43.0 lat)
    let tf = AffineTransform::new(0.0005, 0.0, -76.0, 0.0, -0.0004, 43.0)
        .expect("valid affine transform");

    let (lon, lat) = tf.pixel_to_geo(1000.0, 2000.0);
    assert!((lon - (-75.5)).abs() < 1e-6);
    assert!((lat - 42.2).abs() < 1e-6);

    let (px, py) = tf.geo_to_pixel(lon, lat).expect("invert point");
    assert!((px - 1000.0).abs() < 1e-4);
    assert!((py - 2000.0).abs() < 1e-4);

    let sample_points = vec![
        (0.0, 0.0),
        (500.0, 1500.0),
        (3200.0, 4800.0),
        (10000.0, 8000.0),
    ];
    tf.validate_round_trip(&sample_points, 0.001)
        .expect("round trip validation");
}

#[test]
fn test_georaster_asset_ownship_overlay() {
    let bounds = GeoBounds::new(-76.0, 39.0, -71.0, 44.0).expect("valid bounds");
    let tf = AffineTransform::new(0.0005, 0.0, -76.0, 0.0, -0.0005, 44.0).expect("valid transform");

    let now = Utc.with_ymd_and_hms(2026, 8, 20, 12, 0, 0).unwrap();
    let asset = GeoRasterAsset {
        id: "faa:sec:new_york:112".to_string(),
        provider: "FAA_AERONAVID".to_string(),
        product_name: "FAA_SECTIONAL_NEW_YORK".to_string(),
        edition: "112".to_string(),
        effective_from: now,
        effective_to: Some(now + Duration::days(56)),
        crs_epsg: 4326,
        bounds,
        pixel_width: 10000,
        pixel_height: 10000,
        affine_transform: tf,
        sha256_hash: "abcd1234ef".to_string(),
        status: GeoreferenceStatus::Georeferenced,
        source_url: Some("https://aeronav.faa.gov/visual/sectional/New_York.tif".to_string()),
    };

    // KJFK position: (-73.7789, 40.6398) -> inside bounds
    assert!(asset.covers_position(-73.7789, 40.6398));
    let pixel_pos = asset.ownship_pixel_position(-73.7789, 40.6398);
    assert!(pixel_pos.is_some());
    let (px, py) = pixel_pos.unwrap();
    assert!(px > 4000.0 && px < 5000.0);
    assert!(py > 6000.0 && py < 7000.0);

    // Paris LFPG position: (2.5478, 49.0097) -> outside bounds
    assert!(!asset.covers_position(2.5478, 49.0097));
    assert_eq!(asset.ownship_pixel_position(2.5478, 49.0097), None);
}

#[test]
fn test_flight_phase_engine_complete_flight_lifecycle() {
    let mut engine = FlightPhaseEngine::new();
    let mut t0 = Utc.with_ymd_and_hms(2026, 8, 20, 14, 0, 0).unwrap();

    // 1. Preflight: on ground, stationary
    let telem_preflight = AircraftTelemetry {
        on_ground: true,
        altitude_msl_ft: 13.0,
        altitude_agl_ft: Some(0.0),
        groundspeed_kt: 0.0,
        vertical_speed_fpm: 0.0,
        distance_to_dest_nm: Some(3150.0),
        distance_from_dep_nm: Some(0.0),
        active_procedure_kind: None,
        timestamp: t0,
    };
    let res = engine.evaluate(&telem_preflight);
    assert_eq!(res.phase, FlightPhase::Preflight);

    // 2. Taxi-out
    t0 += Duration::seconds(10);
    let mut telem_taxi = telem_preflight.clone();
    telem_taxi.groundspeed_kt = 18.0;
    telem_taxi.timestamp = t0;
    engine.evaluate(&telem_taxi);
    t0 += Duration::seconds(10);
    telem_taxi.timestamp = t0;
    let res = engine.evaluate(&telem_taxi);
    assert_eq!(res.phase, FlightPhase::TaxiOut);

    // 3. Takeoff roll
    t0 += Duration::seconds(10);
    let mut telem_to = telem_taxi.clone();
    telem_to.groundspeed_kt = 110.0;
    telem_to.timestamp = t0;
    engine.evaluate(&telem_to);
    t0 += Duration::seconds(5);
    telem_to.groundspeed_kt = 145.0;
    telem_to.timestamp = t0;
    let res = engine.evaluate(&telem_to);
    assert_eq!(res.phase, FlightPhase::Takeoff);

    // 4. Initial Climb (Liftoff)
    t0 += Duration::seconds(5);
    let mut telem_climb = telem_to.clone();
    telem_climb.on_ground = false;
    telem_climb.altitude_msl_ft = 650.0;
    telem_climb.altitude_agl_ft = Some(637.0);
    telem_climb.groundspeed_kt = 175.0;
    telem_climb.vertical_speed_fpm = 2500.0;
    telem_climb.timestamp = t0;
    let res = engine.evaluate(&telem_climb);
    assert_eq!(res.phase, FlightPhase::InitialClimb);

    // 5. Departure (Flying SID)
    t0 += Duration::seconds(30);
    let mut telem_dep = telem_climb.clone();
    telem_dep.altitude_msl_ft = 4500.0;
    telem_dep.altitude_agl_ft = Some(4487.0);
    telem_dep.groundspeed_kt = 250.0;
    telem_dep.vertical_speed_fpm = 2200.0;
    telem_dep.active_procedure_kind = Some('D');
    telem_dep.timestamp = t0;
    engine.evaluate(&telem_dep);
    t0 += Duration::seconds(10);
    telem_dep.timestamp = t0;
    let res = engine.evaluate(&telem_dep);
    assert_eq!(res.phase, FlightPhase::Departure);

    // 6. Cruise (FL350, level)
    t0 += Duration::minutes(30);
    let mut telem_cruise = telem_dep.clone();
    telem_cruise.altitude_msl_ft = 35000.0;
    telem_cruise.altitude_agl_ft = Some(35000.0);
    telem_cruise.groundspeed_kt = 480.0;
    telem_cruise.vertical_speed_fpm = 0.0;
    telem_cruise.active_procedure_kind = None;
    telem_cruise.distance_to_dest_nm = Some(1500.0);
    telem_cruise.timestamp = t0;
    engine.evaluate(&telem_cruise);
    t0 += Duration::seconds(10);
    telem_cruise.timestamp = t0;
    let res = engine.evaluate(&telem_cruise);
    assert_eq!(res.phase, FlightPhase::Cruise);

    // 7. Arrival (STAR, descending into destination terminal area)
    t0 += Duration::hours(5);
    let mut telem_arr = telem_cruise.clone();
    telem_arr.altitude_msl_ft = 9000.0;
    telem_arr.altitude_agl_ft = Some(8608.0);
    telem_arr.groundspeed_kt = 250.0;
    telem_arr.vertical_speed_fpm = -1800.0;
    telem_arr.active_procedure_kind = Some('E');
    telem_arr.distance_to_dest_nm = Some(35.0);
    telem_arr.timestamp = t0;
    engine.evaluate(&telem_arr);
    t0 += Duration::seconds(10);
    telem_arr.timestamp = t0;
    let res = engine.evaluate(&telem_arr);
    assert_eq!(res.phase, FlightPhase::Arrival);

    // 8. Approach (ILS final approach)
    t0 += Duration::minutes(10);
    let mut telem_app = telem_arr.clone();
    telem_app.altitude_msl_ft = 2200.0;
    telem_app.altitude_agl_ft = Some(1808.0);
    telem_app.groundspeed_kt = 160.0;
    telem_app.vertical_speed_fpm = -750.0;
    telem_app.active_procedure_kind = Some('F');
    telem_app.distance_to_dest_nm = Some(8.0);
    telem_app.timestamp = t0;
    engine.evaluate(&telem_app);
    t0 += Duration::seconds(10);
    telem_app.timestamp = t0;
    let res = engine.evaluate(&telem_app);
    assert_eq!(res.phase, FlightPhase::Approach);

    // 9. Final segment (3 NM out)
    t0 += Duration::minutes(2);
    let mut telem_final = telem_app.clone();
    telem_final.altitude_msl_ft = 950.0;
    telem_final.altitude_agl_ft = Some(558.0);
    telem_final.distance_to_dest_nm = Some(2.5);
    telem_final.groundspeed_kt = 142.0;
    telem_final.vertical_speed_fpm = -700.0;
    telem_final.timestamp = t0;
    engine.evaluate(&telem_final);
    t0 += Duration::seconds(5);
    telem_final.timestamp = t0;
    let res = engine.evaluate(&telem_final);
    assert_eq!(res.phase, FlightPhase::Final);

    // 10. Landing rollout (touchdown on runway)
    t0 += Duration::seconds(45);
    let mut telem_land = telem_final.clone();
    telem_land.on_ground = true;
    telem_land.altitude_msl_ft = 392.0;
    telem_land.altitude_agl_ft = Some(0.0);
    telem_land.groundspeed_kt = 85.0;
    telem_land.vertical_speed_fpm = 0.0;
    telem_land.timestamp = t0;
    let res = engine.evaluate(&telem_land);
    assert_eq!(res.phase, FlightPhase::Landing);

    // 11. Taxi-in to gate
    t0 += Duration::minutes(1);
    let mut telem_taxiin = telem_land.clone();
    telem_taxiin.groundspeed_kt = 15.0;
    telem_taxiin.timestamp = t0;
    engine.evaluate(&telem_taxiin);
    t0 += Duration::seconds(10);
    telem_taxiin.timestamp = t0;
    let res = engine.evaluate(&telem_taxiin);
    assert_eq!(res.phase, FlightPhase::TaxiIn);

    // 12. Parked at stand
    t0 += Duration::minutes(5);
    let mut telem_park = telem_taxiin.clone();
    telem_park.groundspeed_kt = 0.0;
    telem_park.timestamp = t0;
    engine.evaluate(&telem_park);
    t0 += Duration::seconds(10);
    telem_park.timestamp = t0;
    let res = engine.evaluate(&telem_park);
    assert_eq!(res.phase, FlightPhase::Parked);
}

#[test]
fn test_flight_phase_slew_teleport_protection() {
    let mut engine = FlightPhaseEngine::new();
    let t0 = Utc.with_ymd_and_hms(2026, 8, 20, 14, 0, 0).unwrap();

    let telem_cruise = AircraftTelemetry {
        on_ground: false,
        altitude_msl_ft: 35000.0,
        altitude_agl_ft: Some(35000.0),
        groundspeed_kt: 450.0,
        vertical_speed_fpm: 0.0,
        distance_to_dest_nm: Some(500.0),
        distance_from_dep_nm: Some(2500.0),
        active_procedure_kind: None,
        timestamp: t0,
    };
    engine.evaluate(&telem_cruise);
    engine.evaluate(&telem_cruise);
    assert_eq!(engine.current_phase(), FlightPhase::Cruise);

    // Slew: sudden instantaneous drop to ground level within 1 second!
    let telem_slew = AircraftTelemetry {
        on_ground: true,
        altitude_msl_ft: 13.0,
        altitude_agl_ft: Some(0.0),
        groundspeed_kt: 0.0,
        vertical_speed_fpm: 0.0,
        distance_to_dest_nm: Some(3000.0),
        distance_from_dep_nm: Some(0.0),
        active_procedure_kind: None,
        timestamp: t0 + Duration::seconds(1),
    };
    let assessment = engine.evaluate(&telem_slew);
    assert_eq!(assessment.phase, FlightPhase::Preflight);
    assert!(assessment.evidence.contains("Teleport/Slew detected"));
}

#[test]
fn test_runway_wind_components() {
    // Runway 22R (heading ~224°), Wind 260° at 20 kt
    let (headwind, crosswind) = calculate_runway_wind_components(224.0, 260.0, 20.0);
    // Angular difference: 36°
    // Headwind = 20 * cos(36°) = 16.18 kt
    // Crosswind = 20 * sin(36°) = 11.75 kt (Right crosswind)
    assert!((headwind - 16.18).abs() < 0.1);
    assert!((crosswind - 11.75).abs() < 0.1);

    // Direct tailwind: Runway 04L (heading 44°), Wind 224° at 15 kt
    let (headwind_opp, crosswind_opp) = calculate_runway_wind_components(44.0, 224.0, 15.0);
    assert!((headwind_opp - (-15.0)).abs() < 0.1); // -15 kt = 15 kt tailwind
    assert!(crosswind_opp.abs() < 0.1);
}

#[test]
fn test_planning_tod_calculation() {
    // Current alt 35,000 ft, Destination elevation 392 ft (LFPG), 10 NM decel buffer
    let tod_nm = calculate_planning_tod_nm(35000.0, 392.0, 10.0);
    // (35000 - 392) / 1000 * 3.0 + 10.0 = 34.608 * 3 + 10 = 103.82 + 10 = 113.82 NM
    assert!((tod_nm - 113.824).abs() < 0.1);
}

#[test]
fn test_cross_track_distance() {
    let p1 = (-74.0, 40.0);
    let p2 = (-70.0, 40.0);

    // Aircraft directly on line
    let (d1, side1) = calculate_cross_track_nm((-72.0, 40.0), p1, p2);
    assert!(d1 < 0.01);
    assert_eq!(side1, "ON");

    // Aircraft 1 degree north (~60 NM off route)
    let (d2, side2) = calculate_cross_track_nm((-72.0, 41.0), p1, p2);
    assert!((d2 - 60.0).abs() < 1.0);
    assert_eq!(side2, "L");
}

#[test]
fn test_chart_context_suggestions() {
    let s_taxi = ChartSuggestion::for_phase(FlightPhase::TaxiOut);
    assert_eq!(
        s_taxi.suggested_category,
        NormalizedChartType::AirportDiagram
    );
    assert_eq!(s_taxi.airport_target, "departure");

    let s_dep = ChartSuggestion::for_phase(FlightPhase::Departure);
    assert_eq!(s_dep.suggested_category, NormalizedChartType::Sid);

    let s_arr = ChartSuggestion::for_phase(FlightPhase::Arrival);
    assert_eq!(s_arr.suggested_category, NormalizedChartType::Star);

    let s_app = ChartSuggestion::for_phase(FlightPhase::Approach);
    assert_eq!(s_app.suggested_category, NormalizedChartType::Approach);

    let s_taxiin = ChartSuggestion::for_phase(FlightPhase::TaxiIn);
    assert_eq!(
        s_taxiin.suggested_category,
        NormalizedChartType::AirportDiagram
    );
    assert_eq!(s_taxiin.airport_target, "destination");
}
