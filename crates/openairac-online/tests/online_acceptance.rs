//! Comprehensive Acceptance & Unit Tests for OpenAIRAC Online Network Subsystem.

use chrono::{TimeZone, Utc};
use openairac_online::cache::OnlineCache;
use openairac_online::model::{FacilityType, NetworkFreshness};
use openairac_online::providers::VatsimProvider;
use openairac_online::providers::vatsim_events::parse_vatsim_events_json;
use openairac_online::route::{
    AtcConfidence, RouteAtcPhase, RouteOnlineAwareness, summarize_airport_online,
};
use openairac_online::sanitize::escape_html;

const SAMPLE_VATSIM_JSON: &str = r#"{
  "general": {
    "version": 3,
    "reload": 1,
    "update_timestamp": "2026-08-20T14:30:00.000Z",
    "connected_clients": 3412,
    "unique_users": 3120
  },
  "pilots": [
    {
      "cid": 1000001,
      "name": "Jane Doe",
      "callsign": "BAW123",
      "latitude": 45.1234,
      "longitude": -40.5678,
      "altitude": 35000,
      "groundspeed": 465,
      "transponder": "2412",
      "heading": 85,
      "flight_plan": {
        "flight_rules": "I",
        "aircraft": "B789/H",
        "aircraft_short": "B789",
        "departure": "KJFK",
        "arrival": "LFPG",
        "alternate": "EGLL",
        "cruise_tas": "470",
        "altitude": "FL350",
        "route": "MERIT HFD PUT BOS N72B TAFFY 4800N 05000W 5000N 04000W 5100N 03000W 5100N 02000W DINIM UN460 ETRET",
        "remarks": "OPR/BAW /V/"
      },
      "logon_time": "2026-08-20T12:00:00.000Z",
      "last_updated": "2026-08-20T14:29:55.000Z"
    },
    {
      "cid": 1000002,
      "name": "John Smith",
      "callsign": "AFR006",
      "latitude": 40.6398,
      "longitude": -73.7789,
      "altitude": 14,
      "groundspeed": 12,
      "transponder": "1000",
      "heading": 220,
      "flight_plan": {
        "flight_rules": "I",
        "aircraft_short": "A359",
        "departure": "KJFK",
        "arrival": "LFPG",
        "altitude": "FL370",
        "route": "BETTE MERIT"
      },
      "logon_time": "2026-08-20T14:15:00.000Z",
      "last_updated": "2026-08-20T14:29:50.000Z"
    },
    {
      "cid": 1000003,
      "name": "Alice Bob",
      "callsign": "UAE202",
      "latitude": 25.2532,
      "longitude": 55.3657,
      "altitude": 5000,
      "groundspeed": 220,
      "transponder": "4521",
      "heading": 120,
      "flight_plan": {
        "flight_rules": "I",
        "aircraft_short": "B77W",
        "departure": "OMDB",
        "arrival": "KJFK",
        "altitude": "FL340"
      },
      "logon_time": "2026-08-20T14:20:00.000Z",
      "last_updated": "2026-08-20T14:29:58.000Z"
    }
  ],
  "controllers": [
    {
      "cid": 2000001,
      "name": "Controller 1",
      "callsign": "JFK_DEL",
      "frequency": "135.050",
      "facility": 1,
      "rating": 2,
      "visual_range": 20,
      "text_atis": ["JFK Clearance Delivery", "Information Bravo current"],
      "last_updated": "2026-08-20T14:25:00.000Z",
      "logon_time": "2026-08-20T13:00:00.000Z"
    },
    {
      "cid": 2000002,
      "name": "Controller 2",
      "callsign": "KJFK_TWR",
      "frequency": "119.100",
      "facility": 3,
      "rating": 3,
      "visual_range": 50,
      "text_atis": ["Kennedy Tower", "Departing runways 22R/31L"],
      "last_updated": "2026-08-20T14:28:00.000Z",
      "logon_time": "2026-08-20T13:30:00.000Z"
    },
    {
      "cid": 2000003,
      "name": "Controller 3",
      "callsign": "NY_APP",
      "frequency": "128.125",
      "facility": 4,
      "rating": 4,
      "visual_range": 150,
      "text_atis": ["New York TRACON Approach/Departure"],
      "last_updated": "2026-08-20T14:29:00.000Z",
      "logon_time": "2026-08-20T12:30:00.000Z"
    },
    {
      "cid": 2000004,
      "name": "Controller 4",
      "callsign": "NY_CTR",
      "frequency": "134.150",
      "facility": 5,
      "rating": 5,
      "visual_range": 400,
      "text_atis": ["New York Center Oceanic/Domestic"],
      "last_updated": "2026-08-20T14:29:10.000Z",
      "logon_time": "2026-08-20T11:00:00.000Z"
    },
    {
      "cid": 2000005,
      "name": "Controller 5",
      "callsign": "LFPG_TWR",
      "frequency": "118.150",
      "facility": 3,
      "rating": 3,
      "visual_range": 50,
      "text_atis": ["De Gaulle Tower"],
      "last_updated": "2026-08-20T14:28:30.000Z",
      "logon_time": "2026-08-20T14:00:00.000Z"
    },
    {
      "cid": 2000006,
      "name": "Controller 6",
      "callsign": "PARIS_APP",
      "frequency": "121.150",
      "facility": 4,
      "rating": 4,
      "visual_range": 150,
      "text_atis": ["Paris Approach Control"],
      "last_updated": "2026-08-20T14:29:20.000Z",
      "logon_time": "2026-08-20T13:45:00.000Z"
    },
    {
      "cid": 2000007,
      "name": "Controller 7",
      "callsign": "LON_CTR",
      "frequency": "127.425",
      "facility": 5,
      "rating": 5,
      "visual_range": 450,
      "text_atis": ["London Area Control Center"],
      "last_updated": "2026-08-20T14:29:40.000Z",
      "logon_time": "2026-08-20T12:00:00.000Z"
    }
  ],
  "atis": [
    {
      "cid": 3000001,
      "name": "KJFK ATIS",
      "callsign": "KJFK_ATIS",
      "frequency": "128.725",
      "facility": 7,
      "rating": 1,
      "visual_range": 10,
      "atis_code": "B",
      "text_atis": [
        "KENNEDY INTL INFO B 1430Z",
        "WIND 220 AT 14KT GUSTS 22KT",
        "VISIBILITY 10SM",
        "FEW CLOUDS 5000",
        "TEMP 25 DEWPOINT 17",
        "ALTIMETER 2992",
        "ARRIVING RUNWAY 22L, DEPARTING RUNWAY 22R",
        "ADVISE ON INITIAL CONTACT YOU HAVE INFO B"
      ],
      "last_updated": "2026-08-20T14:30:00.000Z",
      "logon_time": "2026-08-20T14:00:00.000Z"
    }
  ],
  "servers": [
    {
      "ident": "USA-EAST",
      "hostname_or_ip": "198.51.100.10",
      "location": "New York, USA",
      "name": "USA East Server",
      "clients_connection_allowed": true,
      "client_count_connections": 1250
    }
  ],
  "prefiles": [
    {
      "cid": 4000001,
      "name": "Prefile Pilot",
      "callsign": "DAL456",
      "flight_plan": {
        "aircraft_short": "A339",
        "departure": "KJFK",
        "arrival": "EGLL",
        "altitude": "FL390"
      },
      "last_updated": "2026-08-20T14:25:00.000Z"
    }
  ]
}"#;

const SAMPLE_EVENTS_JSON: &str = r#"{
  "data": [
    {
      "id": 8920,
      "name": "Cross the Pond Eastbound 2026",
      "type": { "name": "Mega Event" },
      "start_time": "2026-08-20T11:00:00.000Z",
      "end_time": "2026-08-20T23:00:00.000Z",
      "airports": [
        { "icao": "KJFK" },
        { "icao": "KBOS" },
        { "icao": "LFPG" },
        { "icao": "EGLL" }
      ],
      "routes": [
        { "route": "NAT TRACK A" }
      ],
      "organisers": [
        { "name": "VATSIM Operations" }
      ],
      "link": "https://ctp.vatsim.net",
      "short_description": "Annual Oceanic crossing connecting North America and Europe."
    }
  ]
}"#;

#[test]
fn test_vatsim_snapshot_parsing() {
    let now = Utc.with_ymd_and_hms(2026, 8, 20, 14, 30, 10).unwrap();
    let snapshot =
        VatsimProvider::parse_vatsim_json(SAMPLE_VATSIM_JSON, now).expect("parse snapshot");

    assert_eq!(snapshot.provider_name, "VATSIM");
    assert_eq!(snapshot.connected_clients, 3412);
    assert_eq!(snapshot.freshness, NetworkFreshness::Live);
    assert_eq!(snapshot.age_seconds, 10);

    // Pilots
    assert_eq!(snapshot.pilots.len(), 3);
    let p1 = &snapshot.pilots[0];
    assert_eq!(p1.callsign, "BAW123");
    assert_eq!(p1.cid, 1000001);
    assert_eq!(p1.altitude_ft, 35000);
    assert_eq!(p1.groundspeed_kt, 465);
    assert_eq!(p1.aircraft_type.as_deref(), Some("B789"));
    assert_eq!(p1.departure_icao.as_deref(), Some("KJFK"));
    assert_eq!(p1.arrival_icao.as_deref(), Some("LFPG"));
    assert!(p1.is_airborne());

    let p2 = &snapshot.pilots[1];
    assert_eq!(p2.callsign, "AFR006");
    assert!(!p2.is_airborne()); // Groundspeed 12 kt, alt 14 ft

    // Controllers
    assert_eq!(snapshot.controllers.len(), 7);
    let c_del = &snapshot.controllers[0];
    assert_eq!(c_del.callsign, "JFK_DEL");
    assert_eq!(c_del.facility_type, FacilityType::Delivery);
    assert_eq!(c_del.associated_airport.as_deref(), Some("KJFK"));
    assert!(!c_del.is_enroute);

    let c_twr = &snapshot.controllers[1];
    assert_eq!(c_twr.callsign, "KJFK_TWR");
    assert_eq!(c_twr.facility_type, FacilityType::Tower);
    assert_eq!(c_twr.associated_airport.as_deref(), Some("KJFK"));

    let c_ctr = &snapshot.controllers[6];
    assert_eq!(c_ctr.callsign, "LON_CTR");
    assert_eq!(c_ctr.facility_type, FacilityType::Center);
    assert_eq!(c_ctr.associated_airport, None); // Must NOT pretend to be an airport!
    assert!(c_ctr.is_enroute);

    // ATIS
    assert_eq!(snapshot.atis.len(), 1);
    let atis = &snapshot.atis[0];
    assert_eq!(atis.airport_ident, "KJFK");
    assert_eq!(atis.atis_code, Some('B'));
    assert_eq!(atis.frequency, "128.725");
    assert_eq!(atis.text_atis.len(), 8);

    // Servers & Prefiles
    assert_eq!(snapshot.servers.len(), 1);
    assert_eq!(snapshot.servers[0].ident, "USA-EAST");
    assert_eq!(snapshot.prefiles.len(), 1);
    assert_eq!(snapshot.prefiles[0].callsign, "DAL456");
}

#[test]
fn test_airport_online_summary() {
    let now = Utc.with_ymd_and_hms(2026, 8, 20, 14, 30, 10).unwrap();
    let snapshot =
        VatsimProvider::parse_vatsim_json(SAMPLE_VATSIM_JSON, now).expect("parse snapshot");

    let jfk_pt = Some((-73.7789, 40.6398));
    let summary = summarize_airport_online("KJFK", jfk_pt, &snapshot);

    assert_eq!(summary.airport_ident, "KJFK");
    // DEL, TWR, APP
    assert_eq!(summary.atc_controllers.len(), 3);
    assert!(
        summary
            .atc_controllers
            .iter()
            .any(|c| c.callsign == "JFK_DEL")
    );
    assert!(
        summary
            .atc_controllers
            .iter()
            .any(|c| c.callsign == "KJFK_TWR")
    );
    assert!(
        summary
            .atc_controllers
            .iter()
            .any(|c| c.callsign == "NY_APP")
    );

    // ATIS
    assert!(summary.atis.is_some());
    assert_eq!(summary.atis.as_ref().unwrap().atis_code, Some('B'));

    // Departures (BAW123, AFR006)
    assert_eq!(summary.filed_departures.len(), 2);
    // Arrivals (UAE202)
    assert_eq!(summary.filed_arrivals.len(), 1);
    // On ground (AFR006)
    assert_eq!(summary.on_ground_traffic.len(), 1);
    assert_eq!(summary.on_ground_traffic[0].callsign, "AFR006");
}

#[test]
fn test_route_online_awareness_kjfk_lfpg() {
    let now = Utc.with_ymd_and_hms(2026, 8, 20, 14, 30, 10).unwrap();
    let mut snapshot =
        VatsimProvider::parse_vatsim_json(SAMPLE_VATSIM_JSON, now).expect("parse snapshot");

    // Add events into snapshot
    let events = parse_vatsim_events_json(SAMPLE_EVENTS_JSON).expect("parse events");
    snapshot.events = events;

    let nat_waypoints = vec![
        (-73.778, 40.639), // KJFK
        (-60.000, 45.000),
        (-40.000, 50.000), // Mid-Atlantic (BAW123 at -40.5678, 45.1234 is ~300 NM, let's test corridor)
        (-20.000, 51.000),
        (2.550, 49.012), // LFPG
    ];

    let awareness = RouteOnlineAwareness::analyze("KJFK", "LFPG", &nat_waypoints, 500.0, &snapshot);

    assert_eq!(awareness.departure_icao, "KJFK");
    assert_eq!(awareness.arrival_icao, "LFPG");

    // Departure ATC: JFK_DEL, KJFK_TWR, NY_APP
    assert_eq!(awareness.departure_atc.len(), 3);
    assert_eq!(awareness.departure_atc[0].phase, RouteAtcPhase::Departure);
    assert_eq!(awareness.departure_atc[0].confidence, AtcConfidence::Exact);

    // Departure ATIS
    assert!(awareness.departure_atis.is_some());
    assert_eq!(
        awareness.departure_atis.as_ref().unwrap().atis_code,
        Some('B')
    );

    // Arrival ATC: LFPG_TWR, PARIS_APP
    assert_eq!(awareness.arrival_atc.len(), 2);
    assert_eq!(awareness.arrival_atc[0].phase, RouteAtcPhase::Arrival);

    // Enroute ATC: NY_CTR
    assert!(!awareness.enroute_atc.is_empty());
    assert!(
        awareness
            .enroute_atc
            .iter()
            .any(|c| c.controller.callsign == "NY_CTR")
    );

    // Traffic in corridor: BAW123 and AFR006
    assert!(!awareness.traffic_in_corridor.is_empty());
    assert!(
        awareness
            .traffic_in_corridor
            .iter()
            .any(|p| p.callsign == "BAW123")
    );

    // Events match: Cross the Pond Eastbound 2026
    assert_eq!(awareness.matching_events.len(), 1);
    assert_eq!(
        awareness.matching_events[0].name,
        "Cross the Pond Eastbound 2026"
    );
}

#[test]
fn test_events_api_parsing() {
    let events = parse_vatsim_events_json(SAMPLE_EVENTS_JSON).expect("parse events");
    assert_eq!(events.len(), 1);

    let ev = &events[0];
    assert_eq!(ev.id, 8920);
    assert_eq!(ev.name, "Cross the Pond Eastbound 2026");
    assert_eq!(ev.airports, vec!["KJFK", "KBOS", "LFPG", "EGLL"]);
    assert!(ev.matches_airport("KJFK"));
    assert!(ev.matches_airport("lfpg"));
    assert!(!ev.matches_airport("OMDB"));
}

#[test]
fn test_ephemeral_cache_roundtrip() {
    let now = Utc.with_ymd_and_hms(2026, 8, 20, 14, 30, 0).unwrap();
    let snapshot =
        VatsimProvider::parse_vatsim_json(SAMPLE_VATSIM_JSON, now).expect("parse snapshot");

    let mut cache = OnlineCache::open_in_memory().expect("open memory cache");
    cache.put_snapshot(&snapshot).expect("put snapshot");

    let loaded = cache
        .get_snapshot("VATSIM")
        .expect("get snapshot")
        .expect("snapshot exists");
    assert_eq!(loaded.provider_name, "VATSIM");
    assert_eq!(loaded.connected_clients, 3412);
    assert_eq!(loaded.pilots.len(), 3);
    assert_eq!(loaded.controllers.len(), 7);

    // Events cache
    let events = parse_vatsim_events_json(SAMPLE_EVENTS_JSON).expect("parse events");
    cache.put_events(&events).expect("put events");

    let active_events = cache
        .get_active_and_upcoming_events(now)
        .expect("get active events");
    assert_eq!(active_events.len(), 1);
    assert_eq!(active_events[0].name, "Cross the Pond Eastbound 2026");
}

#[test]
fn test_security_sanitization() {
    assert_eq!(
        escape_html("<script>alert('pwn')</script>&\"test\""),
        "&lt;script&gt;alert(&#39;pwn&#39;)&lt;/script&gt;&amp;&quot;test&quot;"
    );

    let invalid_json = r#"{
      "general": { "update_timestamp": "2026-08-20T14:30:00Z", "connected_clients": 100 },
      "pilots": [
        {
          "cid": 9999,
          "callsign": "MALICIOUS_CALLSIGN_EXTRA_LONG_12345",
          "latitude": 999.0,
          "longitude": -400.0,
          "altitude": 99999999
        }
      ]
    }"#;

    let now = Utc::now();
    let snapshot =
        VatsimProvider::parse_vatsim_json(invalid_json, now).expect("parse invalid json");
    // Pilot with invalid lat/lon (999.0, -400.0) MUST fail closed and be rejected!
    assert_eq!(snapshot.pilots.len(), 0);
}
