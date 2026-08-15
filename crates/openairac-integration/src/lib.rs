//! OpenAIRAC flight-plan integration: airport → SID → enroute → STAR →
//! approach, preserving procedure identity and transitions end to end.
//!
//! Contract:
//! * The planner reads only the temporal store; every entity it touches is
//!   valid at `departure_time` by construction (store queries are
//!   instant-scoped).
//! * Procedures are assembled by the semantic layer
//!   (`openairac-procedures`); their identities (airport, kind, ident,
//!   transition) are carried verbatim into the plan.
//! * The enroute segment is a fix-to-fix route over the canonical airway
//!   graph. The SID exit fix and STAR entry fix are the join points.
//! * Fail closed: when either end has no published procedure, the plan
//!   still returns the procedures it found, but the enroute segment is
//!   reported as unsuccessful with a diagnostic. The planner NEVER
//!   invents connector legs between an airport and the airway graph.

use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use openairac_model::{CanonicalAirport, CanonicalProcedureLeg};
use openairac_procedures::{Procedure, ProcedureKind, ProcedureLeg};
use openairac_routing::{
    AircraftCapabilities, AirwayGraph, Coordinate, Exclusion, RouteRequest, RouteResult,
};
use openairac_store::WorldStore;
use serde::{Deserialize, Serialize};

/// A flight-planning request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightPlanRequest {
    pub origin_airport: String,
    pub destination_airport: String,
    pub departure_time: DateTime<Utc>,
    pub cruise_altitude_ft: Option<u32>,
    pub aircraft_capabilities: AircraftCapabilities,
    /// Optional exact procedure selection; when `None` the first
    /// published procedure for the airport is used.
    pub sid_ident: Option<String>,
    pub sid_transition: Option<String>,
    pub star_ident: Option<String>,
    pub star_transition: Option<String>,
    pub approach_ident: Option<String>,
    pub exclusions: Vec<Exclusion>,
}

/// One planned procedure segment with its join points.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedProcedure {
    pub procedure: Procedure,
    /// The transition whose legs are flown (empty string = main body).
    pub transition: Option<String>,
    pub legs: Vec<ProcedureLeg>,
    /// First fix of the flown leg sequence.
    pub entry_fix: String,
    /// Last fix of the flown leg sequence.
    pub exit_fix: String,
    pub entry_coordinate: Option<Coordinate>,
    pub exit_coordinate: Option<Coordinate>,
}

/// Complete flight plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightPlan {
    pub origin: CanonicalAirport,
    pub destination: CanonicalAirport,
    pub departure: Option<PlannedProcedure>,
    pub arrival: Option<PlannedProcedure>,
    pub approach: Option<PlannedProcedure>,
    pub enroute: RouteResult,
    pub diagnostics: Vec<String>,
}

/// Compute the enroute segment between two published fixes over the
/// canonical airway graph.
fn route_enroute(
    store: &WorldStore,
    t: DateTime<Utc>,
    request: &FlightPlanRequest,
    origin_fix: &str,
    origin_region: &str,
    destination_fix: &str,
    destination_region: &str,
    diagnostics: &mut Vec<String>,
) -> Result<RouteResult> {
    let (graph, graph_diagnostics) = AirwayGraph::build(
        &store.query_waypoints_at(t)?,
        &store.query_navaids_at(t)?,
        &store.query_airway_legs_at(t)?,
        t,
    );
    diagnostics.extend(graph_diagnostics);
    let route_request = RouteRequest {
        origin: openairac_routing::NodeId::fix(origin_fix, origin_region),
        destination: openairac_routing::NodeId::fix(destination_fix, destination_region),
        departure_time: t,
        cruise_altitude_ft: request.cruise_altitude_ft,
        aircraft_capabilities: request.aircraft_capabilities.clone(),
        exclusions: request.exclusions.clone(),
    };
    let result = graph.route(&route_request);
    if result.success {
        diagnostics.push(format!(
            "enroute: {} legs, {:.1} nm",
            result.legs.len(),
            result.total_distance_nm
        ));
    } else {
        diagnostics.extend(result.diagnostics.clone());
    }
    Ok(result)
}

/// Planner over a store connection.
pub struct Planner<'a> {
    store: &'a WorldStore,
}

impl<'a> Planner<'a> {
    pub fn new(store: &'a WorldStore) -> Self {
        Self { store }
    }

    pub fn plan(&self, request: &FlightPlanRequest) -> Result<FlightPlan> {
        let mut diagnostics = Vec::new();
        let t = request.departure_time;

        let airports = self.store.query_airports_at(t)?;
        let origin = airports
            .iter()
            .find(|a| a.ident == request.origin_airport)
            .cloned()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "origin airport {} not present at {t}",
                    request.origin_airport
                )
            })?;
        let destination = airports
            .iter()
            .find(|a| a.ident == request.destination_airport)
            .cloned()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "destination airport {} not present at {t}",
                    request.destination_airport
                )
            })?;

        let waypoints = self.store.query_waypoints_at(t)?;
        let navaids = self.store.query_navaids_at(t)?;
        let legs = self.store.query_procedure_legs_at(t)?;

        // Fix lookup: waypoint coordinates first, navaids second (some
        // procedure fixes are collocated navaids).
        let fix_lookup = move |fix: &str| -> Option<(f64, f64)> {
            let w = waypoints.iter().find(|w| w.ident == fix);
            if let Some(w) = w {
                return Some((w.latitude, w.longitude));
            }
            let n = navaids.iter().find(|n| n.ident == fix);
            n.map(|n| (n.latitude, n.longitude))
        };

        // Select and assemble each procedure segment.
        let select = |airport: &str,
                      kind: ProcedureKind,
                      ident: Option<&str>,
                      transition: Option<&str>,
                      diagnostics: &mut Vec<String>|
         -> Result<Option<PlannedProcedure>> {
            let candidates: Vec<&CanonicalProcedureLeg> = legs
                .iter()
                .filter(|l| {
                    l.airport_ident == airport
                        && ProcedureKind::from_arinc(l.procedure_kind) == Some(kind)
                })
                .collect();
            if candidates.is_empty() {
                diagnostics.push(format!(
                    "{}: no {} procedures published",
                    airport,
                    kind.as_str()
                ));
                return Ok(None);
            }
            let available: Vec<&str> = {
                let mut v: Vec<&str> = candidates
                    .iter()
                    .map(|l| l.procedure_ident.as_str())
                    .collect();
                v.sort();
                v.dedup();
                v
            };
            let chosen_ident = match ident {
                Some(i) => {
                    if !available.contains(&i) {
                        bail!(
                            "{} {} not published (available: {})",
                            airport,
                            i,
                            available.join(", ")
                        );
                    }
                    i.to_string()
                }
                None => {
                    let first = available[0].to_string();
                    if available.len() > 1 {
                        diagnostics.push(format!(
                            "{}: {} procedure not specified; chose {first} (available: {})",
                            airport,
                            kind.as_str(),
                            available.join(", ")
                        ));
                    }
                    first
                }
            };
            let chosen_legs: Vec<CanonicalProcedureLeg> = candidates
                .iter()
                .filter(|l| l.procedure_ident == chosen_ident)
                .map(|l| (*l).clone())
                .collect();
            let procedure =
                Procedure::assemble(airport, kind, &chosen_ident, chosen_legs, &fix_lookup)?;

            // Flight sequence: SID = transition then main; STAR = main
            // then transition; approach = main (plus transition variants).
            let transition_ident = transition.map(|s| s.to_string());
            let (legs_seq, used_transition) = match kind {
                ProcedureKind::Sid => {
                    let trans = procedure
                        .transitions
                        .iter()
                        .find(|tr| {
                            Some(tr.transition_ident.as_str()) == transition_ident.as_deref()
                        })
                        .or_else(|| procedure.transitions.first());
                    let mut seq: Vec<ProcedureLeg> =
                        trans.map(|tr| tr.legs.clone()).unwrap_or_default();
                    seq.extend(procedure.main_legs.clone());
                    (seq, trans.map(|tr| tr.transition_ident.clone()))
                }
                ProcedureKind::Star => {
                    let trans = procedure
                        .transitions
                        .iter()
                        .find(|tr| {
                            Some(tr.transition_ident.as_str()) == transition_ident.as_deref()
                        })
                        .or_else(|| procedure.transitions.first());
                    let mut seq: Vec<ProcedureLeg> = procedure.main_legs.clone();
                    seq.extend(trans.map(|tr| tr.legs.clone()).unwrap_or_default());
                    (seq, trans.map(|tr| tr.transition_ident.clone()))
                }
                ProcedureKind::Approach => (procedure.main_legs.clone(), None),
            };
            if legs_seq.is_empty() {
                diagnostics.push(format!(
                    "{} {}: no legs after assembly",
                    airport, chosen_ident
                ));
                return Ok(None);
            }
            let entry_fix = legs_seq.first().expect("non-empty").fix_ident.clone();
            let exit_fix = legs_seq.last().expect("non-empty").fix_ident.clone();
            let entry_coordinate = legs_seq
                .first()
                .and_then(|l| l.fix_latitude.zip(l.fix_longitude))
                .map(|(la, lo)| Coordinate {
                    latitude_deg: la,
                    longitude_deg: lo,
                });
            let exit_coordinate = legs_seq
                .last()
                .and_then(|l| l.fix_latitude.zip(l.fix_longitude))
                .map(|(la, lo)| Coordinate {
                    latitude_deg: la,
                    longitude_deg: lo,
                });
            Ok(Some(PlannedProcedure {
                procedure,
                transition: used_transition,
                legs: legs_seq,
                entry_fix,
                exit_fix,
                entry_coordinate,
                exit_coordinate,
            }))
        };

        let departure = select(
            &request.origin_airport,
            ProcedureKind::Sid,
            request.sid_ident.as_deref(),
            request.sid_transition.as_deref(),
            &mut diagnostics,
        )?;
        let arrival = select(
            &request.destination_airport,
            ProcedureKind::Star,
            request.star_ident.as_deref(),
            request.star_transition.as_deref(),
            &mut diagnostics,
        )?;
        let approach = select(
            &request.destination_airport,
            ProcedureKind::Approach,
            request.approach_ident.as_deref(),
            None,
            &mut diagnostics,
        )?;

        // Enroute: SID exit fix -> STAR entry fix over the airway graph.
        let enroute = match (&departure, &arrival) {
            (Some(sid), Some(star)) => {
                // Region codes for the join fixes come from the published
                // waypoints, never guessed.
                let region_of = |fix: &str| -> Option<String> {
                    self.store
                        .query_waypoints_at(t)
                        .ok()?
                        .into_iter()
                        .find(|w| w.ident == fix)
                        .map(|w| w.region_code.clone())
                };
                match (region_of(&sid.exit_fix), region_of(&star.entry_fix)) {
                    (Some(origin_region), Some(destination_region)) => route_enroute(
                        &self.store,
                        t,
                        request,
                        &sid.exit_fix,
                        &origin_region,
                        &star.entry_fix,
                        &destination_region,
                        &mut diagnostics,
                    )?,
                    _ => {
                        diagnostics.push(format!(
                            "enroute segment unavailable: join fix region unknown for {} or {}",
                            sid.exit_fix, star.entry_fix
                        ));
                        RouteResult {
                            success: false,
                            legs: Vec::new(),
                            nodes: Vec::new(),
                            total_distance_nm: 0.0,
                            diagnostics: Vec::new(),
                        }
                    }
                }
            }
            _ => {
                diagnostics.push(
                    "enroute segment unavailable: both a SID (origin) and a STAR \
                     (destination) are required to join the airway graph"
                        .to_string(),
                );
                RouteResult {
                    success: false,
                    legs: Vec::new(),
                    nodes: Vec::new(),
                    total_distance_nm: 0.0,
                    diagnostics: Vec::new(),
                }
            }
        };

        Ok(FlightPlan {
            origin,
            destination,
            departure,
            arrival,
            approach,
            enroute,
            diagnostics,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeDelta;
    use openairac_model::{
        AirwayLegId, ProcedureLegId, SourceSnapshot, SourceSnapshotId, TemporalValidity, WaypointId,
    };
    use openairac_model::{CanonicalAirwayLeg, CanonicalWaypoint};

    fn temporal(t: DateTime<Utc>) -> TemporalValidity {
        TemporalValidity {
            valid_from: t,
            valid_until: None,
            source_snapshot_id: SourceSnapshotId("snap-test".to_string()),
        }
    }

    fn waypoint(
        ident: &str,
        lat: f64,
        lon: f64,
        t: DateTime<Utc>,
        enroute: bool,
    ) -> CanonicalWaypoint {
        CanonicalWaypoint {
            object_id: WaypointId(format!("WP-{ident}")),
            ident: ident.to_string(),
            name: ident.to_string(),
            latitude: lat,
            longitude: lon,
            is_enroute: enroute,
            region_code: "K2".to_string(),
            terminal_area_ident: if enroute {
                None
            } else {
                Some("KAAA".to_string())
            },
            waypoint_type: Some(0x202057),
            temporal: temporal(t),
        }
    }

    fn airport(ident: &str, lat: f64, lon: f64, t: DateTime<Utc>) -> CanonicalAirport {
        CanonicalAirport {
            id: openairac_model::AirportId(format!("AP-{ident}")),
            ident: ident.to_string(),
            name: format!("{ident} Test"),
            airport_type: "medium_airport".to_string(),
            latitude: lat,
            longitude: lon,
            elevation_ft: None,
            iso_country: Some("US".to_string()),
            municipality: None,
            runways: Vec::new(),
            temporal: temporal(t),
        }
    }

    fn airway_leg(route: &str, from: &str, to: &str, t: DateTime<Utc>) -> CanonicalAirwayLeg {
        CanonicalAirwayLeg {
            object_id: AirwayLegId(format!("LEG-{route}-{from}-{to}")),
            route_ident: route.to_string(),
            route_type: "O".to_string(),
            level: Some('L'),
            sequence_number: 2,
            start_fix: from.to_string(),
            start_icao_code: "K2".to_string(),
            end_fix: to.to_string(),
            end_icao_code: "K2".to_string(),
            direction: 'N',
            minimum_altitude_ft: None,
            maximum_altitude_ft: None,
            temporal: temporal(t),
        }
    }

    fn procedure_leg(
        airport: &str,
        kind: char,
        ident: &str,
        transition: &str,
        seq: u32,
        fix: &str,
        terminator: &str,
        t: DateTime<Utc>,
    ) -> CanonicalProcedureLeg {
        CanonicalProcedureLeg {
            object_id: ProcedureLegId(format!("PL-{airport}-{kind}-{ident}-{seq}")),
            airport_ident: airport.to_string(),
            icao_code: "K2".to_string(),
            procedure_kind: kind,
            procedure_ident: ident.to_string(),
            route_type: String::new(),
            transition_ident: transition.to_string(),
            sequence_number: seq,
            fix_ident: fix.to_string(),
            fix_icao_code: "K2".to_string(),
            fix_section: " ".to_string(),
            waypoint_description: "E ".to_string(),
            turn_direction: None,
            rnp_nm: None,
            path_terminator: terminator.to_string(),
            recommended_navaid: None,
            arc_radius_nm: None,
            course_a_deg: None,
            distance_a_nm: None,
            course_b_deg: None,
            distance_b_nm: None,
            altitude_descriptor: None,
            altitude_1_ft: None,
            altitude_2_ft: None,
            speed_limit_kts: None,
            course_c_deg: None,
            vertical_angle_deg: None,
            msa_center_fix: None,
            route_qualifiers: String::new(),
            raw: String::new(),
            temporal: temporal(t),
        }
    }

    fn seeded_store(t: DateTime<Utc>) -> Result<WorldStore> {
        let mut store = WorldStore::open_in_memory()?;
        store.migrate()?;
        store.insert_source_snapshot(&SourceSnapshot {
            id: SourceSnapshotId("snap-test".to_string()),
            provider: "test".to_string(),
            dataset: "fixture".to_string(),
            provider_revision: None,
            airac_cycle: None,
            effective_from: None,
            effective_until: None,
            retrieved_at: t,
            source_uri: "memory:".to_string(),
            content_sha256: "0".repeat(64),
            license_id: None,
            license_notes: None,
            parser_version: "test".to_string(),
        })?;
        let fix = |ident: &str, lat: f64, lon: f64| waypoint(ident, lat, lon, t, true);
        let fixes = vec![
            fix("W1", 37.5, -119.5),
            fix("W2", 38.0, -119.0),
            fix("W3", 38.5, -118.5),
            fix("W4", 39.0, -118.0),
        ];
        for f in &fixes {
            store.insert_waypoint(f)?;
        }
        for leg in [
            airway_leg("V1", "W1", "W2", t),
            airway_leg("V2", "W2", "W3", t),
            airway_leg("V3", "W3", "W4", t),
        ] {
            store.transact(|conn| openairac_store::insert_airway_leg_conn(conn, &leg))?;
        }
        for leg in [
            procedure_leg("KAAA", 'D', "SID1", "", 10, "W1", "TF", t),
            procedure_leg("KAAA", 'D', "SID1", "", 20, "W2", "TF", t),
            procedure_leg("KBBB", 'E', "STAR1", "", 10, "W3", "TF", t),
            procedure_leg("KBBB", 'E', "STAR1", "", 20, "W4", "TF", t),
            procedure_leg("KBBB", 'F', "ILS27", "", 10, "W4", "TF", t),
        ] {
            store.transact(|conn| openairac_store::insert_procedure_leg_conn(conn, &leg))?;
        }
        store.insert_airport(&airport("KAAA", 37.0, -120.0, t))?;
        store.insert_airport(&airport("KBBB", 39.5, -117.5, t))?;
        Ok(store)
    }

    fn request(t: DateTime<Utc>) -> FlightPlanRequest {
        FlightPlanRequest {
            origin_airport: "KAAA".to_string(),
            destination_airport: "KBBB".to_string(),
            departure_time: t,
            cruise_altitude_ft: None,
            aircraft_capabilities: AircraftCapabilities { rnav: true },
            sid_ident: None,
            sid_transition: None,
            star_ident: None,
            star_transition: None,
            approach_ident: None,
            exclusions: Vec::new(),
        }
    }

    #[test]
    fn test_full_plan_sid_enroute_star_approach() {
        let t = Utc::now();
        let store = seeded_store(t).unwrap();
        let planner = Planner::new(&store);
        let plan = planner.plan(&request(t)).unwrap();

        assert_eq!(plan.origin.ident, "KAAA");
        assert_eq!(plan.destination.ident, "KBBB");

        let sid = plan.departure.expect("SID selected");
        assert_eq!(sid.procedure.name, "SID1 SID KAAA");
        assert_eq!(sid.entry_fix, "W1");
        assert_eq!(sid.exit_fix, "W2");

        let star = plan.arrival.expect("STAR selected");
        assert_eq!(star.entry_fix, "W3");
        assert_eq!(star.exit_fix, "W4");

        let approach = plan.approach.expect("approach selected");
        assert_eq!(approach.procedure.name, "ILS27 APPROACH KBBB");

        assert!(plan.enroute.success);
        assert_eq!(plan.enroute.legs.len(), 1); // W2 -> W3 via V2
        assert_eq!(plan.enroute.legs[0].route_ident, "V2");
    }

    #[test]
    fn test_missing_procedure_fails_closed() {
        let t = Utc::now();
        let store = seeded_store(t).unwrap();
        // A STAR ident that is not published must fail closed.
        let plan = Planner::new(&store)
            .plan(&FlightPlanRequest {
                star_ident: Some("NOT_PUBLISHED".to_string()),
                ..request(t)
            })
            .unwrap_err();
        assert!(plan.to_string().contains("NOT_PUBLISHED"));
    }

    #[test]
    fn test_unpublished_airport_errors() {
        let t = Utc::now();
        let store = seeded_store(t).unwrap();
        let mut req = request(t);
        req.origin_airport = "KZZZ".to_string();
        let err = Planner::new(&store).plan(&req).unwrap_err();
        assert!(err.to_string().contains("KZZZ"));
    }

    #[test]
    fn test_temporal_exclusion() {
        // Entities from a future revision must not appear in a plan for
        // the earlier instant.
        let t = Utc::now();
        let mut store = seeded_store(t).unwrap();
        let future = t + TimeDelta::seconds(3600);
        // A procedure valid only later:
        store
            .transact(|conn| {
                openairac_store::insert_procedure_leg_conn(
                    conn,
                    &procedure_leg("KBBB", 'F', "ILS28", "", 10, "W4", "TF", future),
                )
            })
            .unwrap();
        let plan = Planner::new(&store).plan(&request(t)).unwrap();
        assert_eq!(plan.approach.unwrap().procedure.name, "ILS27 APPROACH KBBB");
    }
}
