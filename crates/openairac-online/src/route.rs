//! Route and Airport Online Traffic & ATC Awareness Engine.
//!
//! Evaluates active ATC along a flight plan route, filters corridor traffic,
//! correlates active ATIS, summarizes airport operations (arrivals/departures/ground),
//! and matches active/upcoming online events without inventing artificial sector geometry.

use crate::model::{
    FacilityType, NetworkSnapshot, OnlineAtis, OnlineController, OnlineEvent, OnlinePilot,
};
use serde::{Deserialize, Serialize};

const EARTH_RADIUS_NM: f64 = 3440.065;

/// Association confidence for active ATC controllers along a route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AtcConfidence {
    /// Exact match to departure or destination airport station.
    Exact,
    /// Known radar approach/departure facility serving the terminal area.
    KnownFacility,
    /// Enroute center/FIR associated with route corridor coordinates.
    Likely,
    /// Unresolved or approximate correlation.
    Unresolved,
}

impl AtcConfidence {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Exact => "EXACT",
            Self::KnownFacility => "KNOWN_FACILITY",
            Self::Likely => "LIKELY",
            Self::Unresolved => "UNRESOLVED",
        }
    }
}

/// Relevant ATC controller along an active flight plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteController {
    pub controller: OnlineController,
    pub phase: RouteAtcPhase,
    pub confidence: AtcConfidence,
    pub note: Option<String>,
}

/// Flight phase served by an online ATC controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RouteAtcPhase {
    Departure,
    Enroute,
    Arrival,
}

impl RouteAtcPhase {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Departure => "DEPARTURE",
            Self::Enroute => "ENROUTE",
            Self::Arrival => "ARRIVAL",
        }
    }
}

/// Comprehensive online operational summary for an airport.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AirportOnlineSummary {
    pub airport_ident: String,
    pub atc_controllers: Vec<OnlineController>,
    pub atis: Option<OnlineAtis>,
    pub filed_arrivals: Vec<OnlinePilot>,
    pub filed_departures: Vec<OnlinePilot>,
    pub on_ground_traffic: Vec<OnlinePilot>,
}

/// Comprehensive route online awareness report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteOnlineAwareness {
    pub departure_icao: String,
    pub arrival_icao: String,
    pub route_corridor_nm: f64,
    pub departure_atc: Vec<RouteController>,
    pub departure_atis: Option<OnlineAtis>,
    pub enroute_atc: Vec<RouteController>,
    pub arrival_atc: Vec<RouteController>,
    pub arrival_atis: Option<OnlineAtis>,
    pub traffic_in_corridor: Vec<OnlinePilot>,
    pub traffic_near_destination: Vec<OnlinePilot>,
    pub matching_events: Vec<OnlineEvent>,
}

impl RouteOnlineAwareness {
    /// Analyze active network snapshot against a flight plan (departure, arrival, route waypoints).
    pub fn analyze(
        departure_icao: &str,
        arrival_icao: &str,
        route_waypoints: &[(f64, f64)], // (lon, lat)
        corridor_half_width_nm: f64,
        snapshot: &NetworkSnapshot,
    ) -> Self {
        let dep = departure_icao.trim().to_uppercase();
        let arr = arrival_icao.trim().to_uppercase();

        let mut departure_atc = Vec::new();
        let mut arrival_atc = Vec::new();
        let mut enroute_atc = Vec::new();

        let mut departure_atis = None;
        let mut arrival_atis = None;

        // 1. Correlate ATIS
        for a in &snapshot.atis {
            if a.airport_ident == dep {
                departure_atis = Some(a.clone());
            } else if a.airport_ident == arr {
                arrival_atis = Some(a.clone());
            }
        }

        // 2. Correlate Controllers
        for c in &snapshot.controllers {
            if let Some(apt) = &c.associated_airport {
                if apt == &dep {
                    departure_atc.push(RouteController {
                        controller: c.clone(),
                        phase: RouteAtcPhase::Departure,
                        confidence: AtcConfidence::Exact,
                        note: Some(format!(
                            "Departure airport station ({})",
                            c.facility_type.as_str()
                        )),
                    });
                    continue;
                } else if apt == &arr {
                    arrival_atc.push(RouteController {
                        controller: c.clone(),
                        phase: RouteAtcPhase::Arrival,
                        confidence: AtcConfidence::Exact,
                        note: Some(format!(
                            "Arrival airport station ({})",
                            c.facility_type.as_str()
                        )),
                    });
                    continue;
                }
            }

            // Check Approach / Radar Terminal facilities serving terminal areas
            let callsign_upper = c.callsign.to_uppercase();
            if c.facility_type == FacilityType::Approach {
                if is_approach_for_airport(&callsign_upper, &dep) {
                    departure_atc.push(RouteController {
                        controller: c.clone(),
                        phase: RouteAtcPhase::Departure,
                        confidence: AtcConfidence::KnownFacility,
                        note: Some(
                            "Departure terminal radar approach/departure control".to_string(),
                        ),
                    });
                    continue;
                } else if is_approach_for_airport(&callsign_upper, &arr) {
                    arrival_atc.push(RouteController {
                        controller: c.clone(),
                        phase: RouteAtcPhase::Arrival,
                        confidence: AtcConfidence::KnownFacility,
                        note: Some("Arrival terminal radar approach control".to_string()),
                    });
                    continue;
                }
            }

            // Check Enroute centers matching known FIRs or callsign prefixes along route
            if (c.is_enroute || c.facility_type == FacilityType::Center)
                && let Some((conf, note)) =
                    evaluate_enroute_controller_relevance(&callsign_upper, &dep, &arr)
            {
                enroute_atc.push(RouteController {
                    controller: c.clone(),
                    phase: RouteAtcPhase::Enroute,
                    confidence: conf,
                    note: Some(note),
                });
            }
        }

        // Sort departure and arrival ATC logically (DEL -> GND -> TWR -> APP)
        departure_atc.sort_by_key(|rc| match rc.controller.facility_type {
            FacilityType::Delivery => 1,
            FacilityType::Ground => 2,
            FacilityType::Tower => 3,
            FacilityType::Approach => 4,
            _ => 5,
        });
        arrival_atc.sort_by_key(|rc| match rc.controller.facility_type {
            FacilityType::Approach => 1,
            FacilityType::Tower => 2,
            FacilityType::Ground => 3,
            FacilityType::Delivery => 4,
            _ => 5,
        });

        // 3. Filter corridor traffic and destination traffic
        let mut traffic_in_corridor = Vec::new();
        let mut traffic_near_destination = Vec::new();

        for p in &snapshot.pilots {
            let pilot_pt = (p.longitude, p.latitude);

            // Corridor traffic check
            if !route_waypoints.is_empty() {
                let min_dist = min_distance_to_route(pilot_pt, route_waypoints);
                if min_dist <= corridor_half_width_nm {
                    traffic_in_corridor.push(p.clone());
                }
            }

            // Destination vicinity check (last waypoint in route or filed arrival airport)
            if let Some(dest_pt) = route_waypoints.last() {
                let dist_to_dest = distance_nm(pilot_pt, *dest_pt);
                if dist_to_dest <= 100.0 {
                    traffic_near_destination.push(p.clone());
                }
            }
        }

        // 4. Match active/upcoming events
        let now = snapshot.received_at;
        let mut matching_events = Vec::new();
        for ev in &snapshot.events {
            let is_time_match =
                ev.is_active_at(now) || ev.is_upcoming_within(now, chrono::Duration::hours(24));
            if is_time_match && (ev.matches_airport(&dep) || ev.matches_airport(&arr)) {
                matching_events.push(ev.clone());
            }
        }

        Self {
            departure_icao: dep,
            arrival_icao: arr,
            route_corridor_nm: corridor_half_width_nm * 2.0,
            departure_atc,
            departure_atis,
            enroute_atc,
            arrival_atc,
            arrival_atis,
            traffic_in_corridor,
            traffic_near_destination,
            matching_events,
        }
    }
}

/// Summarize airport online state from snapshot.
pub fn summarize_airport_online(
    airport_ident: &str,
    airport_lat_lon: Option<(f64, f64)>,
    snapshot: &NetworkSnapshot,
) -> AirportOnlineSummary {
    let icao = airport_ident.trim().to_uppercase();

    // 1. Active ATC
    let mut atc_controllers = Vec::new();
    for c in &snapshot.controllers {
        if let Some(apt) = &c.associated_airport
            && apt == &icao
        {
            atc_controllers.push(c.clone());
            continue;
        }
        if c.facility_type == FacilityType::Approach
            && is_approach_for_airport(&c.callsign.to_uppercase(), &icao)
        {
            atc_controllers.push(c.clone());
        }
    }
    atc_controllers.sort_by_key(|c| match c.facility_type {
        FacilityType::Delivery => 1,
        FacilityType::Ground => 2,
        FacilityType::Tower => 3,
        FacilityType::Approach => 4,
        _ => 5,
    });

    // 2. ATIS
    let atis = snapshot
        .atis
        .iter()
        .find(|a| a.airport_ident == icao)
        .cloned();

    // 3. Filed arrivals and departures
    let mut filed_arrivals = Vec::new();
    let mut filed_departures = Vec::new();
    let mut on_ground_traffic = Vec::new();

    for p in &snapshot.pilots {
        if p.arrival_icao.as_deref() == Some(&icao) {
            filed_arrivals.push(p.clone());
        }
        if p.departure_icao.as_deref() == Some(&icao) {
            filed_departures.push(p.clone());
        }

        if let Some(apt_pt) = airport_lat_lon {
            let pilot_pt = (p.longitude, p.latitude);
            let dist = distance_nm(pilot_pt, apt_pt);
            if dist <= 5.0 && !p.is_airborne() {
                on_ground_traffic.push(p.clone());
            }
        }
    }

    AirportOnlineSummary {
        airport_ident: icao,
        atc_controllers,
        atis,
        filed_arrivals,
        filed_departures,
        on_ground_traffic,
    }
}

/// Check if an approach callsign serves a given airport (e.g. "NY_APP" -> "KJFK", "LON_APP" -> "EGLL").
fn is_approach_for_airport(callsign: &str, airport: &str) -> bool {
    let cs = callsign.to_uppercase();
    let apt = airport.to_uppercase();

    if apt == "KJFK" || apt == "KLGA" || apt == "KEWR" {
        return cs.contains("NY_") || cs.contains("NEWYORK_") || cs.contains("JFK_");
    }
    if apt == "EGLL" || apt == "EGKK" || apt == "EGLC" || apt == "EGSS" {
        return cs.contains("LON_")
            || cs.contains("LONDON_")
            || cs.contains("LL_")
            || cs.contains("HEATHROW_");
    }
    if apt == "LFPG" || apt == "LFPO" || apt == "LFPB" {
        return cs.contains("PARIS_")
            || cs.contains("PG_")
            || cs.contains("PO_")
            || cs.contains("DEGAULLE_");
    }
    if apt == "OMDB" || apt == "OMDW" {
        return cs.contains("DUBAI_") || cs.contains("UAE_") || cs.contains("OMAE_");
    }

    // Generic match: e.g. "EDDF_APP" for "EDDF"
    cs.starts_with(&apt)
}

/// Evaluate if an enroute center is likely relevant for a route between dep and arr.
fn evaluate_enroute_controller_relevance(
    callsign: &str,
    dep: &str,
    arr: &str,
) -> Option<(AtcConfidence, String)> {
    let cs = callsign.to_uppercase();

    // New York ARTCC (KZNY / NY_CTR)
    if (cs.starts_with("NY_") || cs.starts_with("ZNY_") || cs.starts_with("KZNY_"))
        && (dep.starts_with('K') || arr.starts_with('K'))
    {
        return Some((
            AtcConfidence::Likely,
            "New York ARTCC (ZNY) enroute sector".to_string(),
        ));
    }

    // Boston ARTCC (KZBW / BOS_CTR / ZBW_CTR)
    if (cs.starts_with("BOS_") || cs.starts_with("ZBW_") || cs.starts_with("KZBW_"))
        && (dep.starts_with('K')
            || arr.starts_with('K')
            || dep.starts_with('C')
            || arr.starts_with('C'))
    {
        return Some((
            AtcConfidence::Likely,
            "Boston ARTCC (ZBW) enroute sector".to_string(),
        ));
    }

    // London ACC (LON_CTR / EGTT_CTR)
    if (cs.starts_with("LON_") || cs.starts_with("EGTT_") || cs.starts_with("EGGX_"))
        && (dep.starts_with("EG")
            || arr.starts_with("EG")
            || dep.starts_with("LF")
            || arr.starts_with("LF"))
    {
        return Some((
            AtcConfidence::Likely,
            "London Area Control Center (EGTT) enroute sector".to_string(),
        ));
    }

    // Paris ACC (LFFF_CTR / PARIS_CTR)
    if (cs.starts_with("LFFF_") || cs.starts_with("PARIS_"))
        && (dep.starts_with("LF")
            || arr.starts_with("LF")
            || dep.starts_with("EG")
            || arr.starts_with("EG"))
    {
        return Some((
            AtcConfidence::Likely,
            "Paris Area Control Center (LFFF) enroute sector".to_string(),
        ));
    }

    // Shanwick Oceanic (EGGX_O_CTR / SHANWICK)
    if (cs.contains("SHANWICK") || cs.starts_with("EGGX_"))
        && ((dep.starts_with('K') && (arr.starts_with('E') || arr.starts_with('L')))
            || (dep.starts_with('E') && arr.starts_with('K')))
    {
        return Some((
            AtcConfidence::Likely,
            "Shanwick Oceanic Control Area (EGGX)".to_string(),
        ));
    }

    // Gander Oceanic (CZQX_O_CTR / GANDER)
    if (cs.contains("GANDER") || cs.starts_with("CZQX_"))
        && ((dep.starts_with('K') && (arr.starts_with('E') || arr.starts_with('L')))
            || (dep.starts_with('E') && arr.starts_with('K')))
    {
        return Some((
            AtcConfidence::Likely,
            "Gander Oceanic Control Area (CZQX)".to_string(),
        ));
    }

    None
}

/// Great-Circle distance in NM between two (lon, lat) points.
pub fn distance_nm(p1: (f64, f64), p2: (f64, f64)) -> f64 {
    let lat1 = p1.1.to_radians();
    let lon1 = p1.0.to_radians();
    let lat2 = p2.1.to_radians();
    let lon2 = p2.0.to_radians();

    let dlat = lat2 - lat1;
    let dlon = lon2 - lon1;

    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());

    EARTH_RADIUS_NM * c
}

/// Minimum distance in NM from a point to a segmented route.
pub fn min_distance_to_route(point: (f64, f64), route: &[(f64, f64)]) -> f64 {
    if route.is_empty() {
        return f64::MAX;
    }
    if route.len() == 1 {
        return distance_nm(point, route[0]);
    }

    let mut min_d = f64::MAX;
    for i in 0..route.len() - 1 {
        let d = point_to_segment_distance_nm(point, route[i], route[i + 1]);
        if d < min_d {
            min_d = d;
        }
    }
    min_d
}

/// Minimum distance in NM from a point to a single line segment.
pub fn point_to_segment_distance_nm(
    point: (f64, f64),
    seg_start: (f64, f64),
    seg_end: (f64, f64),
) -> f64 {
    let seg_len = distance_nm(seg_start, seg_end);
    if seg_len < 1e-4 {
        return distance_nm(point, seg_start);
    }

    let px = point.0;
    let py = point.1;
    let x1 = seg_start.0;
    let y1 = seg_start.1;
    let x2 = seg_end.0;
    let y2 = seg_end.1;

    let dx = x2 - x1;
    let dy = y2 - y1;

    let t = (((px - x1) * dx + (py - y1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    let proj_x = x1 + t * dx;
    let proj_y = y1 + t * dy;

    distance_nm(point, (proj_x, proj_y))
}
