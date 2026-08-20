//! Comprehensive Integration & Acceptance Tests for OpenAIRAC Weather Layer.

use chrono::{Duration, TimeZone, Utc};
use openairac_weather::briefing::{AirportWeatherBriefing, FlightBriefing};
use openairac_weather::cache::WeatherCache;
use openairac_weather::corridor::RouteCorridor;
use openairac_weather::model::{
    CloudLayer, FlightCategory, MetarReport, PirepReport, Sigmet, SigmetHazard, TafForecastPeriod,
    TafReport, WeatherStaleness,
};
use openairac_weather::providers::AviationWeatherProvider;

const SAMPLE_MULTI_METARS: &str = r#"[
    {
        "icaoId": "KJFK",
        "obsTime": 1787229000,
        "reportTime": "2026-08-20T12:30:00.000Z",
        "temp": 25.0,
        "dewp": 17.0,
        "wdir": 220,
        "wspd": 14,
        "wgst": 22,
        "visib": "10+",
        "altim": 1013,
        "fltcat": "VFR",
        "rawOb": "METAR KJFK 201230Z 22014G22KT 10SM FEW050 SCT250 25/17 A2992",
        "clouds": [
            {"cover": "FEW", "base": 5000},
            {"cover": "SCT", "base": 25000}
        ]
    },
    {
        "icaoId": "LFPG",
        "obsTime": 1787229000,
        "reportTime": "2026-08-20T12:30:00.000Z",
        "temp": 20.0,
        "dewp": 12.0,
        "wdir": 260,
        "wspd": 8,
        "visib": "10+",
        "altim": 1018,
        "fltcat": "VFR",
        "rawOb": "METAR LFPG 201230Z 26008KT 9999 BKN040 20/12 Q1018",
        "clouds": [
            {"cover": "BKN", "base": 4000}
        ]
    },
    {
        "icaoId": "EGLL",
        "obsTime": 1787229000,
        "reportTime": "2026-08-20T12:30:00.000Z",
        "temp": 18.0,
        "dewp": 14.0,
        "wdir": 240,
        "wspd": 12,
        "visib": "4.0",
        "altim": 1015,
        "fltcat": "MVFR",
        "rawOb": "METAR EGLL 201230Z 24012KT 4SM OVC015 18/14 Q1015",
        "clouds": [
            {"cover": "OVC", "base": 1500}
        ]
    }
]"#;

const SAMPLE_TAF_JSON: &str = r#"[
    {
        "icaoId": "KJFK",
        "issueTime": "2026-08-20T11:32:00.000Z",
        "validTimeFrom": 1787227200,
        "validTimeTo": 1787335200,
        "rawTAF": "TAF KJFK 201132Z 2012/2118 22014KT P6SM SCT050 FM201800 24016G24KT 5SM -SHRA OVC030",
        "fcsts": [
            {
                "timeFrom": 1787227200,
                "timeTo": 1787248800,
                "change": "FM",
                "wdir": 220,
                "wspd": 14,
                "visib": 6.0,
                "rawWx": "2012/2018 22014KT P6SM SCT050"
            },
            {
                "timeFrom": 1787248800,
                "timeTo": 1787335200,
                "change": "FM",
                "wdir": 240,
                "wspd": 16,
                "wgst": 24,
                "visib": 5.0,
                "rawWx": "FM201800 24016G24KT 5SM -SHRA OVC030"
            }
        ]
    }
]"#;

const SAMPLE_SIGMET_GEOJSON: &str = r#"{
    "type": "FeatureCollection",
    "features": [
        {
            "type": "Feature",
            "properties": {
                "firId": "CZQX",
                "firName": "GANDER OCEANIC",
                "seriesId": "01",
                "hazard": "TURB",
                "qualifier": "SEV",
                "validTimeFrom": "2026-08-20T11:00:00.000Z",
                "validTimeTo": "2026-08-20T15:00:00.000Z",
                "base": 28000,
                "top": 38000,
                "rawSigmet": "CZQX SIGMET 01 VALID 201100/201500 CZQX- GANDER OCEANIC FIR SEV TURB FCST FL280/380"
            },
            "geometry": {
                "type": "Polygon",
                "coordinates": [
                    [
                        [-50.0, 48.0],
                        [-40.0, 48.0],
                        [-40.0, 54.0],
                        [-50.0, 54.0],
                        [-50.0, 48.0]
                    ]
                ]
            }
        }
    ]
}"#;

#[test]
fn test_weather_parsing_and_staleness() {
    let prov = AviationWeatherProvider::new();
    let metars = prov.parse_metar_json(SAMPLE_MULTI_METARS).unwrap();
    assert_eq!(metars.len(), 3);

    // JFK
    let jfk = &metars[0];
    assert_eq!(jfk.station_id, "KJFK");
    assert_eq!(jfk.flight_category, FlightCategory::Vfr);
    assert_eq!(jfk.temp_c, Some(25.0));
    assert_eq!(jfk.dewpoint_c, Some(17.0));
    assert_eq!(jfk.wind_speed_kts, Some(14));
    assert_eq!(jfk.wind_gust_kts, Some(22));

    // EGLL
    let egll = &metars[2];
    assert_eq!(egll.station_id, "EGLL");
    assert_eq!(egll.flight_category, FlightCategory::Mvfr);

    // Staleness
    let now = Utc::now();
    assert_eq!(WeatherStaleness::evaluate_metar(now - Duration::minutes(15), now), WeatherStaleness::Fresh);
    assert_eq!(WeatherStaleness::evaluate_metar(now - Duration::minutes(45), now), WeatherStaleness::Aging);
    assert_eq!(WeatherStaleness::evaluate_metar(now - Duration::minutes(90), now), WeatherStaleness::Stale);
    assert_eq!(WeatherStaleness::evaluate_metar(now - Duration::minutes(150), now), WeatherStaleness::Expired);
}

#[test]
fn test_taf_parsing_and_eta_matching() {
    let prov = AviationWeatherProvider::new();
    let tafs = prov.parse_taf_json(SAMPLE_TAF_JSON).unwrap();
    assert_eq!(tafs.len(), 1);

    let jfk_taf = &tafs[0];
    assert_eq!(jfk_taf.station_id, "KJFK");
    assert_eq!(jfk_taf.forecast_periods.len(), 2);

    let eta_early = Utc.timestamp_opt(1787230000, 0).single().unwrap();
    let p_early = jfk_taf.forecast_at_eta(eta_early).unwrap();
    assert_eq!(p_early.wind_speed_kts, Some(14));
    assert_eq!(p_early.wind_gust_kts, None);

    let eta_late = Utc.timestamp_opt(1787255000, 0).single().unwrap();
    let p_late = jfk_taf.forecast_at_eta(eta_late).unwrap();
    assert_eq!(p_late.wind_speed_kts, Some(16));
    assert_eq!(p_late.wind_gust_kts, Some(24));
}

#[test]
fn test_sigmet_geojson_and_route_corridor_intersection() {
    let prov = AviationWeatherProvider::new();
    let sigmets = prov.parse_isigmet_geojson(SAMPLE_SIGMET_GEOJSON).unwrap();
    assert_eq!(sigmets.len(), 1);

    let gander_turb = &sigmets[0];
    assert_eq!(gander_turb.fir_id, "CZQX");
    assert_eq!(gander_turb.hazard, SigmetHazard::Turbulence);
    assert_eq!(gander_turb.base_altitude_ft, Some(28000));
    assert_eq!(gander_turb.top_altitude_ft, Some(38000));

    // Route from KJFK to LFPG across North Atlantic track passing through Gander Oceanic
    let nat_route = RouteCorridor::new(vec![
        (-73.778, 40.639),  // KJFK
        (-60.000, 45.000),
        (-45.000, 50.000),  // Directly through CZQX polygon (-50 to -40 lon, 48 to 54 lat)
        (-30.000, 52.000),
        (-10.000, 50.000),
        (2.550, 49.012),    // LFPG
    ]).with_width(50.0);

    let intersections = nat_route.filter_intersecting_sigmets(&sigmets);
    assert_eq!(intersections.len(), 1);
    assert_eq!(intersections[0].id, gander_turb.id);

    // Pacific route (KSFO to RJTT) should NOT intersect Gander SIGMET
    let pac_route = RouteCorridor::new(vec![
        (-122.375, 37.618),
        (-160.000, 45.000),
        (140.386, 35.764),
    ]);
    let pac_intersections = pac_route.filter_intersecting_sigmets(&sigmets);
    assert_eq!(pac_intersections.len(), 0);
}

#[test]
fn test_flight_briefing_generation() {
    let now = Utc::now();
    let eta = now + Duration::hours(7);

    let metar_jfk = MetarReport {
        station_id: "KJFK".to_string(),
        observation_time: now - Duration::minutes(15),
        report_time: Some(now - Duration::minutes(15)),
        raw_text: "METAR KJFK 201200Z 22014KT 10SM CLR 25/18 A2992".to_string(),
        flight_category: FlightCategory::Vfr,
        temp_c: Some(25.0),
        dewpoint_c: Some(18.0),
        wind_dir_deg: Some(220),
        wind_speed_kts: Some(14),
        wind_gust_kts: None,
        wind_variable: false,
        visibility_sm: Some(10.0),
        altimeter_hpa: Some(1013.2),
        altimeter_inhg: Some(29.92),
        clouds: Vec::new(),
        weather_phenomena: Vec::new(),
        fetch_time: now,
        provider_id: "NOAA_AWC".to_string(),
        is_stale: false,
    };

    let briefing = FlightBriefing {
        departure_icao: "KJFK".to_string(),
        destination_icao: "LFPG".to_string(),
        alternate_icaos: vec!["LFPO".to_string(), "EGLL".to_string()],
        planned_departure_time: now,
        estimated_time_enroute_minutes: 420,
        estimated_time_of_arrival: eta,
        departure: AirportWeatherBriefing {
            icao: "KJFK".to_string(),
            metar: Some(metar_jfk),
            taf: None,
            taf_at_eta: None,
            charts_count: 38,
            navdata_procedures_available: true,
            navdata_note: "FAA CIFP SIDs & STARs active".to_string(),
        },
        destination: AirportWeatherBriefing {
            icao: "LFPG".to_string(),
            metar: None,
            taf: None,
            taf_at_eta: None,
            charts_count: 9,
            navdata_procedures_available: false,
            navdata_note: "Public SIA dataset contains 0 procedures; eAIP charts active".to_string(),
        },
        alternates: Vec::new(),
        route_sigmets: Vec::new(),
        route_pireps: Vec::new(),
        navdata_cycle: "2608".to_string(),
        charts_cycle: "2608".to_string(),
        generated_at: now,
    };

    let text = briefing.format_text();
    assert!(text.contains("OPENAIRAC FLIGHT BRIEFING"));
    assert!(text.contains("KJFK → LFPG"));
    assert!(text.contains("FAA CIFP"));
    assert!(text.contains("Public SIA dataset contains 0 procedures"));

    let html = briefing.format_html();
    assert!(html.contains("&rarr;"));
    assert!(html.contains("KJFK"));
    assert!(html.contains("LFPG"));
}
