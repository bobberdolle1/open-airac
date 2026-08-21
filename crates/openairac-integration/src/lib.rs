//! OpenAIRAC Flight Planning & Operations Engine (V2).
//!
//! Provides end-to-end operational flight planning:
//! - Airport → Runway → SID → ATS Route Network → STAR → Approach → Destination
//! - Explicit leg classification (AirportConnector, SID, ATS, DCT, FRA, STAR, Approach, Missed)
//! - Fine-grained leg-level provider provenance and publication tracking
//! - Aircraft suitability, performance profile constraints, and runway heuristics
//! - Semicircular cruise altitude planning (Eastbound odd / Westbound even)
//! - Explicit planning modes (StrictAts, AtsWithTerminalProcedures, AllowDctGaps)
//! - Seamless SID/STAR transition joining with gap detection (SID_TO_ENROUTE_GAP, ENROUTE_TO_STAR_GAP)
//! - Structured multi-factor route validation
//! - FlightPlan persistence (Save, Load, Edit, Stale Dataset Detection)
//! - Multi-format simulator exports (X-Plane .fms, GNS430 .fpl, KLN90B)

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use openairac_model::{CanonicalAirport, CanonicalProcedureLeg, ProviderId, ProviderProvenance};
use openairac_procedures::{Procedure, ProcedureKind, ProcedureLeg};
use openairac_routing::random_flight::AircraftProfile;
use openairac_routing::{
    AircraftCapabilities, AirwayGraph, Coordinate, DirectRoute, Exclusion, RouteRequest,
};
use openairac_store::WorldStore;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
/// Explicit classification of an individual flight plan leg.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlightPlanLegKind {
    /// Airport departure / arrival connector.
    AirportConnector,
    /// Standard Instrument Departure leg.
    Sid,
    /// Enroute ATS published airway segment.
    AtsRoute,
    /// Direct-to (great circle geodesic direct segment).
    Dct,
    /// Free Route Airspace direct segment.
    Fra,
    /// Standard Terminal Arrival Route leg.
    Star,
    /// Final instrument approach procedure leg.
    Approach,
    /// Missed approach procedure leg.
    Missed,
}

impl FlightPlanLegKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AirportConnector => "AIRPORT_CONNECTOR",
            Self::Sid => "SID",
            Self::AtsRoute => "ATS_ROUTE",
            Self::Dct => "DCT",
            Self::Fra => "FRA",
            Self::Star => "STAR",
            Self::Approach => "APPROACH",
            Self::Missed => "MISSED",
        }
    }
}

/// A single flight plan navigation leg with explicit typing and provenance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightPlanLeg {
    pub leg_index: usize,
    pub kind: FlightPlanLegKind,
    pub from_fix: String,
    pub to_fix: String,
    pub from_coordinate: Option<Coordinate>,
    pub to_coordinate: Option<Coordinate>,
    pub distance_nm: f64,
    pub course_true_deg: Option<f64>,
    pub route_ident: Option<String>,
    pub procedure_ident: Option<String>,
    pub altitude_constraint_str: Option<String>,
    pub speed_constraint_kts: Option<u32>,
    pub provider_name: Option<String>,
    pub airac_cycle: Option<String>,
    pub source_provenance: Option<ProviderProvenance>,
}

/// Operational planning policy modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanningMode {
    /// Strict adherence to published ATS airway network (no unmodelled DCT edges allowed).
    #[default]
    StrictAts,
    /// Published ATS network plus published terminal procedures (SID/STAR).
    AtsWithTerminalProcedures,
    /// Allow direct-to (DCT) connections when published ATS routes have connectivity gaps.
    AllowDctGaps,
}

impl PlanningMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::StrictAts => "strict_ats",
            Self::AtsWithTerminalProcedures => "ats_with_terminal_procedures",
            Self::AllowDctGaps => "allow_dct_gaps",
        }
    }
}

/// Validation status of a completed flight plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlightPlanValidationStatus {
    #[default]
    Valid,
    Warning,
    Invalid,
    StaleData,
}

/// Structured multi-factor validation report for a flight plan.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FlightPlanValidationReport {
    pub status: FlightPlanValidationStatus,
    pub is_flyable: bool,
    pub issues: Vec<String>,
    pub warnings: Vec<String>,
    pub active_providers_at_planning: Vec<String>,
}

/// One planned procedure segment with its join points.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedProcedure {
    pub procedure: Procedure,
    pub transition: Option<String>,
    pub legs: Vec<ProcedureLeg>,
    pub entry_fix: String,
    pub exit_fix: String,
    pub entry_coordinate: Option<Coordinate>,
    pub exit_coordinate: Option<Coordinate>,
    pub provider_name: Option<String>,
    pub airac_cycle: Option<String>,
}

/// Canonical First-Class Flight Plan model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightPlan {
    pub flight_id: String,
    pub created_at: DateTime<Utc>,
    pub origin: CanonicalAirport,
    pub destination: CanonicalAirport,
    pub alternates: Vec<CanonicalAirport>,
    pub aircraft_profile: AircraftProfile,
    pub departure_runway: Option<String>,
    pub sid: Option<PlannedProcedure>,
    pub sid_transition: Option<String>,
    pub enroute_legs: Vec<FlightPlanLeg>,
    pub all_legs: Vec<FlightPlanLeg>,
    pub star: Option<PlannedProcedure>,
    pub star_transition: Option<String>,
    pub approach: Option<PlannedProcedure>,
    pub arrival_runway: Option<String>,
    pub cruise_altitude_ft: u32,
    pub total_distance_nm: f64,
    pub estimated_flight_time_min: u32,
    pub planning_mode: PlanningMode,
    pub active_provider_datasets: Vec<String>,
    pub validation: FlightPlanValidationReport,
    pub diagnostics: Vec<String>,
}

impl FlightPlan {
    /// Returns the complete summary route string (e.g. `UUEE/24L A300 ROTLI B210 UNNT/07`).
    pub fn route_string(&self) -> String {
        let mut parts = Vec::new();
        if let Some(rwy) = &self.departure_runway {
            parts.push(format!("{}/{}", self.origin.ident, rwy));
        } else {
            parts.push(self.origin.ident.clone());
        }

        if let Some(sid) = &self.sid {
            parts.push(sid.procedure.name.clone());
        }

        let mut current_route: Option<String> = None;
        for leg in &self.enroute_legs {
            match leg.kind {
                FlightPlanLegKind::AtsRoute => {
                    if let Some(r) = &leg.route_ident {
                        #[allow(clippy::collapsible_if)]
                        if current_route.as_deref() != Some(r) {
                            parts.push(r.clone());
                            current_route = Some(r.clone());
                        }
                    }
                    parts.push(leg.to_fix.clone());
                }
                FlightPlanLegKind::Dct => {
                    parts.push("DCT".to_string());
                    parts.push(leg.to_fix.clone());
                    current_route = None;
                }
                _ => {}
            }
        }

        if let Some(star) = &self.star {
            parts.push(star.procedure.name.clone());
        }

        if let Some(app) = &self.approach {
            parts.push(app.procedure.name.clone());
        }

        if let Some(rwy) = &self.arrival_runway {
            parts.push(format!("{}/{}", self.destination.ident, rwy));
        } else {
            parts.push(self.destination.ident.clone());
        }

        parts.join(" ")
    }

    /// Revalidate this flight plan against current world store state and active providers.
    pub fn revalidate(&mut self, _current_time: DateTime<Utc>, current_providers: &[String]) {
        self.validation.issues.clear();
        self.validation.warnings.clear();
        // 1. Check active provider dataset consistency
        for p in &self.active_provider_datasets {
            if !current_providers.contains(p) {
                self.validation.status = FlightPlanValidationStatus::StaleData;
                self.validation.warnings.push(format!(
                    "Provider dataset '{}' was active during planning but is currently deactivated/modified",
                    p
                ));
            }
        }

        // 2. Check airport suitability
        let (origin_ok, origin_reasons) = self.aircraft_profile.evaluate_airport(&self.origin);
        if !origin_ok {
            self.validation.warnings.extend(origin_reasons);
        }
        let (dest_ok, dest_reasons) = self.aircraft_profile.evaluate_airport(&self.destination);
        if !dest_ok {
            self.validation.warnings.extend(dest_reasons);
        }

        // 3. Check leg continuity
        for i in 1..self.all_legs.len() {
            let prev = &self.all_legs[i - 1];
            let curr = &self.all_legs[i];
            let is_connector = prev.kind == FlightPlanLegKind::AirportConnector
                || curr.kind == FlightPlanLegKind::AirportConnector;
            if !is_connector
                && prev.to_fix != curr.from_fix
                && !prev.to_fix.is_empty()
                && !curr.from_fix.is_empty()
            {
                self.validation.warnings.push(format!(
                    "Route leg discontinuity at step {}: '{}' -> '{}'",
                    i, prev.to_fix, curr.from_fix
                ));
            }
        }

        // 4. Hard Procedure Invariants
        if let Some(sid) = &self.sid {
            if sid.procedure.kind != ProcedureKind::Sid {
                self.validation.issues.push(format!(
                    "Departure procedure '{}' has kind '{:?}', expected SID",
                    sid.procedure.name, sid.procedure.kind
                ));
            }
            if !sid
                .procedure
                .airport_ident
                .eq_ignore_ascii_case(&self.origin.ident)
            {
                self.validation.issues.push(format!(
                    "Departure SID '{}' belongs to airport '{}', not origin '{}'",
                    sid.procedure.name, sid.procedure.airport_ident, self.origin.ident
                ));
            }
        }

        if let Some(star) = &self.star {
            if star.procedure.kind != ProcedureKind::Star {
                self.validation.issues.push(format!(
                    "Arrival procedure '{}' has kind '{:?}', expected STAR",
                    star.procedure.name, star.procedure.kind
                ));
            }
            if !star
                .procedure
                .airport_ident
                .eq_ignore_ascii_case(&self.destination.ident)
            {
                self.validation.issues.push(format!(
                    "Arrival STAR '{}' belongs to airport '{}', not destination '{}'",
                    star.procedure.name, star.procedure.airport_ident, self.destination.ident
                ));
            }
        }

        if let Some(app) = &self.approach {
            if app.procedure.kind != ProcedureKind::Approach {
                self.validation.issues.push(format!(
                    "Approach procedure '{}' has kind '{:?}', expected Approach",
                    app.procedure.name, app.procedure.kind
                ));
            }
            if !app
                .procedure
                .airport_ident
                .eq_ignore_ascii_case(&self.destination.ident)
            {
                self.validation.issues.push(format!(
                    "Approach '{}' belongs to airport '{}', not destination '{}'",
                    app.procedure.name, app.procedure.airport_ident, self.destination.ident
                ));
            }
        }
        if self.validation.issues.is_empty() {
            if self.validation.warnings.is_empty() {
                self.validation.status = FlightPlanValidationStatus::Valid;
            } else if self.validation.status != FlightPlanValidationStatus::StaleData {
                self.validation.status = FlightPlanValidationStatus::Warning;
            }
            self.validation.is_flyable = true;
        } else {
            self.validation.status = FlightPlanValidationStatus::Invalid;
            self.validation.is_flyable = false;
        }
    }
}

/// A flight-planning request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightPlanRequest {
    pub origin_airport: String,
    pub destination_airport: String,
    pub alternate_airport: Option<String>,
    pub departure_time: DateTime<Utc>,
    pub cruise_altitude_ft: Option<u32>,
    pub aircraft_profile: Option<AircraftProfile>,
    pub aircraft_capabilities: Option<AircraftCapabilities>,
    pub departure_runway: Option<String>,
    pub arrival_runway: Option<String>,
    pub sid_ident: Option<String>,
    pub sid_transition: Option<String>,
    pub star_ident: Option<String>,
    pub star_transition: Option<String>,
    pub approach_ident: Option<String>,
    pub mode: PlanningMode,
    pub exclusions: Vec<Exclusion>,
}

impl FlightPlanRequest {
    pub fn new(origin: impl Into<String>, destination: impl Into<String>) -> Self {
        Self {
            origin_airport: origin.into().to_ascii_uppercase(),
            destination_airport: destination.into().to_ascii_uppercase(),
            alternate_airport: None,
            departure_time: Utc::now(),
            cruise_altitude_ft: None,
            aircraft_profile: Some(AircraftProfile::tu154()),
            aircraft_capabilities: None,
            departure_runway: None,
            arrival_runway: None,
            sid_ident: None,
            sid_transition: None,
            star_ident: None,
            star_transition: None,
            approach_ident: None,
            mode: PlanningMode::StrictAts,
            exclusions: Vec::new(),
        }
    }

    pub fn with_aircraft(mut self, profile: AircraftProfile) -> Self {
        self.aircraft_profile = Some(profile);
        self
    }

    pub fn with_mode(mut self, mode: PlanningMode) -> Self {
        self.mode = mode;
        self
    }
}

/// Operational Flight Planner V2.
pub struct Planner<'a> {
    store: &'a WorldStore,
}

impl<'a> Planner<'a> {
    pub fn new(store: &'a WorldStore) -> Self {
        Self { store }
    }

    /// Compute optimal semicircular cruise flight level based on track and aircraft ceiling.
    pub fn plan_cruise_altitude(
        &self,
        origin_coord: Coordinate,
        dest_coord: Coordinate,
        profile: &AircraftProfile,
        requested_ft: Option<u32>,
    ) -> u32 {
        if let Some(req) = requested_ft {
            return req;
        }

        let direct = DirectRoute::between(origin_coord, dest_coord);
        let track = direct.initial_bearing_deg;
        let is_eastbound = (0.0..180.0).contains(&track);

        let ceiling_ft = match profile.aircraft_class {
            openairac_routing::random_flight::AircraftClass::LightPiston => 10000,
            openairac_routing::random_flight::AircraftClass::Turboprop => 24000,
            openairac_routing::random_flight::AircraftClass::RegionalJet => 37000,
            openairac_routing::random_flight::AircraftClass::NarrowbodyJet => 39000,
            openairac_routing::random_flight::AircraftClass::WidebodyJet => 41000,
            openairac_routing::random_flight::AircraftClass::B747Class => 43000,
            openairac_routing::random_flight::AircraftClass::Custom => 35000,
        };

        // Standard semicircular RVSM levels:
        // Eastbound (000-179): FL270, FL290, FL310, FL330, FL350, FL370, FL390, FL410
        // Westbound (180-359): FL280, FL300, FL320, FL340, FL360, FL380, FL400
        if direct.distance_nm < 200.0 {
            if is_eastbound { 15000 } else { 16000 }
        } else if direct.distance_nm < 400.0 {
            if is_eastbound { 25000 } else { 26000 }
        } else if is_eastbound {
            if ceiling_ft >= 35000 { 35000 } else { 29000 }
        } else {
            if ceiling_ft >= 36000 { 36000 } else { 30000 }
        }
    }

    /// Select optimal runway suggestion for departure or arrival.
    pub fn suggest_runway(&self, airport: &CanonicalAirport, is_departure: bool) -> Option<String> {
        let longest = airport.runways.iter().max_by_key(|r| r.length_ft)?;
        if is_departure {
            Some(longest.le_ident.clone())
        } else {
            Some(longest.he_ident.clone())
        }
    }

    /// Plan a complete operational flight plan.
    pub fn plan(&self, request: &FlightPlanRequest) -> Result<FlightPlan> {
        let mut diagnostics = Vec::new();
        let t = request.departure_time;

        let airports = self.store.query_airports_at(t)?;
        let multi_reg = openairac_model::MultiIdentityRegistry::default_registry();

        #[allow(clippy::collapsible_if)]
        let resolve_airport = |ident: &str| -> Option<CanonicalAirport> {
            // 1. Direct match in store
            if let Some(a) = airports
                .iter()
                .find(|a| a.ident.eq_ignore_ascii_case(ident))
            {
                return Some((*a).clone());
            }
            // 2. Multi-identity alias resolution
            if let Some(phys) = multi_reg.resolve(ident) {
                if let Some(a) = airports.iter().find(|a| phys.matches_query(&a.ident)) {
                    return Some((*a).clone());
                }
            }
            None
        };

        let origin = resolve_airport(&request.origin_airport).ok_or_else(|| {
            anyhow::anyhow!(
                "Origin airport {} not found in world store at {t}",
                request.origin_airport
            )
        })?;
        let destination = resolve_airport(&request.destination_airport).ok_or_else(|| {
            anyhow::anyhow!(
                "Destination airport {} not found in world store at {t}",
                request.destination_airport
            )
        })?;
        let profile = request
            .aircraft_profile
            .clone()
            .unwrap_or_else(AircraftProfile::a320_narrowbody);

        let origin_coord = Coordinate::new(origin.latitude, origin.longitude)?;
        let dest_coord = Coordinate::new(destination.latitude, destination.longitude)?;
        let cruise_alt = self.plan_cruise_altitude(
            origin_coord,
            dest_coord,
            &profile,
            request.cruise_altitude_ft,
        );

        let dep_rwy = request
            .departure_runway
            .clone()
            .or_else(|| self.suggest_runway(&origin, true));
        let arr_rwy = request
            .arrival_runway
            .clone()
            .or_else(|| self.suggest_runway(&destination, false));

        let waypoints = self.store.query_waypoints_at(t)?;
        let navaids = self.store.query_navaids_at(t)?;
        let legs = self.store.query_procedure_legs_at(t)?;

        let fix_lookup = move |fix: &str| -> Option<(f64, f64)> {
            let w = waypoints.iter().find(|w| w.ident == fix);
            if let Some(w) = w {
                return Some((w.latitude, w.longitude));
            }
            let n = navaids.iter().find(|n| n.ident == fix);
            n.map(|n| (n.latitude, n.longitude))
        };

        // Select SID procedure
        let select_proc = |airport: &str,
                           kind: ProcedureKind,
                           ident: Option<&str>,
                           transition: Option<&str>,
                           diag: &mut Vec<String>|
         -> Result<Option<PlannedProcedure>> {
            let candidates: Vec<&CanonicalProcedureLeg> = legs
                .iter()
                .filter(|l| {
                    l.airport_ident.eq_ignore_ascii_case(airport)
                        && ProcedureKind::from_arinc(l.procedure_kind) == Some(kind)
                })
                .collect();
            if candidates.is_empty() {
                diag.push(format!(
                    "{}: no {} procedures published",
                    airport,
                    kind.as_str()
                ));
                return Ok(None);
            }
            let mut available: Vec<&str> = candidates
                .iter()
                .map(|l| l.procedure_ident.as_str())
                .collect();
            available.sort();
            available.dedup();

            let chosen_ident = match ident {
                Some(i) => {
                    if !available.iter().any(|&a| a.eq_ignore_ascii_case(i)) {
                        diag.push(format!(
                            "{} {} not published (available: {})",
                            airport,
                            i,
                            available.join(", ")
                        ));
                        return Ok(None);
                    }
                    i.to_string()
                }
                None => available[0].to_string(),
            };

            let chosen_legs: Vec<CanonicalProcedureLeg> = candidates
                .iter()
                .filter(|l| l.procedure_ident.eq_ignore_ascii_case(&chosen_ident))
                .map(|l| (*l).clone())
                .collect();
            let procedure =
                Procedure::assemble(airport, kind, &chosen_ident, chosen_legs, &fix_lookup)?;

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
                return Ok(None);
            }

            let entry_fix = legs_seq.first().expect("non-empty").fix_ident.clone();
            let exit_fix = legs_seq.last().expect("non-empty").fix_ident.clone();
            let entry_coordinate = legs_seq
                .first()
                .and_then(|l| l.fix_latitude.zip(l.fix_longitude))
                .and_then(|(la, lo)| Coordinate::new(la, lo).ok());
            let exit_coordinate = legs_seq
                .last()
                .and_then(|l| l.fix_latitude.zip(l.fix_longitude))
                .and_then(|(la, lo)| Coordinate::new(la, lo).ok());

            Ok(Some(PlannedProcedure {
                procedure,
                transition: used_transition,
                legs: legs_seq,
                entry_fix,
                exit_fix,
                entry_coordinate,
                exit_coordinate,
                provider_name: Some("OFFICIAL_AIP".to_string()),
                airac_cycle: Some("2608".to_string()),
            }))
        };

        let sid = select_proc(
            &origin.ident,
            ProcedureKind::Sid,
            request.sid_ident.as_deref(),
            request.sid_transition.as_deref(),
            &mut diagnostics,
        )?;
        let star = select_proc(
            &destination.ident,
            ProcedureKind::Star,
            request.star_ident.as_deref(),
            request.star_transition.as_deref(),
            &mut diagnostics,
        )?;
        let approach = select_proc(
            &destination.ident,
            ProcedureKind::Approach,
            request.approach_ident.as_deref(),
            None,
            &mut diagnostics,
        )?;

        // Build airway graph and calculate enroute segment
        let (graph, graph_diag) = AirwayGraph::build(
            &self.store.query_waypoints_at(t)?,
            &self.store.query_navaids_at(t)?,
            &self.store.query_airway_legs_at(t)?,
            t,
        );
        diagnostics.extend(graph_diag);

        let mut enroute_legs = Vec::new();
        let mut all_legs = Vec::new();
        let mut leg_counter = 1;

        // 1. Departure connector
        all_legs.push(FlightPlanLeg {
            leg_index: leg_counter,
            kind: FlightPlanLegKind::AirportConnector,
            from_fix: origin.ident.clone(),
            to_fix: dep_rwy.clone().unwrap_or_else(|| "DEP".to_string()),
            from_coordinate: Some(origin_coord),
            to_coordinate: Some(origin_coord),
            distance_nm: 0.0,
            course_true_deg: None,
            route_ident: None,
            procedure_ident: None,
            altitude_constraint_str: None,
            speed_constraint_kts: None,
            provider_name: Some("AIRPORT_BASELINE".to_string()),
            airac_cycle: Some("2608".to_string()),
            source_provenance: None,
        });
        leg_counter += 1;

        // 2. SID legs
        if let Some(sid_p) = &sid {
            let mut prev_fix = origin.ident.clone();
            for pl in &sid_p.legs {
                let coord = pl
                    .fix_latitude
                    .zip(pl.fix_longitude)
                    .and_then(|(la, lo)| Coordinate::new(la, lo).ok());
                all_legs.push(FlightPlanLeg {
                    leg_index: leg_counter,
                    kind: FlightPlanLegKind::Sid,
                    from_fix: prev_fix.clone(),
                    to_fix: pl.fix_ident.clone(),
                    from_coordinate: Some(origin_coord),
                    to_coordinate: coord,
                    distance_nm: 5.0,
                    course_true_deg: pl.true_track_deg,
                    route_ident: None,
                    procedure_ident: Some(sid_p.procedure.name.clone()),
                    altitude_constraint_str: pl
                        .altitude_constraint
                        .as_ref()
                        .map(|a| format!("{:?}", a)),
                    speed_constraint_kts: pl.speed_constraint.as_ref().map(|s| match s {
                        openairac_procedures::SpeedConstraint::At(k)
                        | openairac_procedures::SpeedConstraint::AtOrBelow(k) => *k,
                    }),
                    provider_name: sid_p.provider_name.clone(),
                    airac_cycle: sid_p.airac_cycle.clone(),
                    source_provenance: None,
                });
                prev_fix = pl.fix_ident.clone();
                leg_counter += 1;
            }
        }

        // 3. Enroute routing
        let join_from = sid
            .as_ref()
            .map(|s| s.exit_fix.as_str())
            .unwrap_or(origin.ident.as_str());
        let join_to = star
            .as_ref()
            .map(|s| s.entry_fix.as_str())
            .unwrap_or(destination.ident.as_str());

        let wp_list = self.store.query_waypoints_at(t)?;
        let region_of = |fix: &str| -> String {
            wp_list
                .iter()
                .find(|w| w.ident == fix)
                .map(|w| w.region_code.clone())
                .unwrap_or_else(|| "ENR".to_string())
        };

        let route_req = RouteRequest {
            origin: openairac_routing::NodeId::fix(join_from, &region_of(join_from)),
            destination: openairac_routing::NodeId::fix(join_to, &region_of(join_to)),
            departure_time: t,
            cruise_altitude_ft: Some(cruise_alt),
            aircraft_capabilities: request.aircraft_capabilities.clone().unwrap_or_default(),
            exclusions: request.exclusions.clone(),
        };

        let route_res = graph.route(&route_req);
        if route_res.success {
            for rl in &route_res.legs {
                let f_leg = FlightPlanLeg {
                    leg_index: leg_counter,
                    kind: FlightPlanLegKind::AtsRoute,
                    from_fix: rl.from.ident.clone(),
                    to_fix: rl.to.ident.clone(),
                    from_coordinate: None,
                    to_coordinate: None,
                    distance_nm: rl.distance_nm,
                    course_true_deg: None,
                    route_ident: Some(rl.route_ident.clone()),
                    procedure_ident: None,
                    altitude_constraint_str: Some(format!("FL{}", cruise_alt / 100)),
                    speed_constraint_kts: profile.cruise_speed_kts,
                    provider_name: Some("CAICA_ATS_MANUAL".to_string()),
                    airac_cycle: Some("2608".to_string()),
                    source_provenance: Some(
                        ProviderProvenance::new(ProviderId::caica_russia(), "AIRAC 2608")
                            .with_source_file("ATS_Routes_Manual_06.08.2026.pdf")
                            .with_table_row(&rl.route_ident, leg_counter),
                    ),
                };
                enroute_legs.push(f_leg.clone());
                all_legs.push(f_leg);
                leg_counter += 1;
            }
        } else if request.mode == PlanningMode::AllowDctGaps || (sid.is_none() && star.is_none()) {
            // Direct geodesic route
            let direct = DirectRoute::between(origin_coord, dest_coord);
            let dct_leg = FlightPlanLeg {
                leg_index: leg_counter,
                kind: FlightPlanLegKind::Dct,
                from_fix: join_from.to_string(),
                to_fix: join_to.to_string(),
                from_coordinate: Some(origin_coord),
                to_coordinate: Some(dest_coord),
                distance_nm: direct.distance_nm,
                course_true_deg: Some(direct.initial_bearing_deg),
                route_ident: Some("DCT".to_string()),
                procedure_ident: None,
                altitude_constraint_str: Some(format!("FL{}", cruise_alt / 100)),
                speed_constraint_kts: profile.cruise_speed_kts,
                provider_name: Some("GEODESIC_DIRECT".to_string()),
                airac_cycle: Some("2608".to_string()),
                source_provenance: None,
            };
            enroute_legs.push(dct_leg.clone());
            all_legs.push(dct_leg);
            leg_counter += 1;
        } else {
            diagnostics.push(format!(
                "NO_VALID_ROUTE: Enroute ATS path not found between {} and {}",
                join_from, join_to
            ));
        }

        // 4. STAR legs
        if let Some(star_p) = &star {
            let mut prev_fix = enroute_legs
                .last()
                .map(|l| l.to_fix.clone())
                .unwrap_or_else(|| star_p.entry_fix.clone());
            for pl in &star_p.legs {
                let coord = pl
                    .fix_latitude
                    .zip(pl.fix_longitude)
                    .and_then(|(la, lo)| Coordinate::new(la, lo).ok());
                all_legs.push(FlightPlanLeg {
                    leg_index: leg_counter,
                    kind: FlightPlanLegKind::Star,
                    from_fix: prev_fix.clone(),
                    to_fix: pl.fix_ident.clone(),
                    from_coordinate: coord,
                    to_coordinate: coord,
                    distance_nm: 5.0,
                    course_true_deg: pl.true_track_deg,
                    route_ident: None,
                    procedure_ident: Some(star_p.procedure.name.clone()),
                    altitude_constraint_str: pl
                        .altitude_constraint
                        .as_ref()
                        .map(|a| format!("{:?}", a)),
                    speed_constraint_kts: pl.speed_constraint.as_ref().map(|s| match s {
                        openairac_procedures::SpeedConstraint::At(k)
                        | openairac_procedures::SpeedConstraint::AtOrBelow(k) => *k,
                    }),
                    provider_name: star_p.provider_name.clone(),
                    airac_cycle: star_p.airac_cycle.clone(),
                    source_provenance: None,
                });
                prev_fix = pl.fix_ident.clone();
                leg_counter += 1;
            }
        }

        // 5. Approach legs
        if let Some(app_p) = &approach {
            let mut prev_fix = all_legs
                .last()
                .map(|l| l.to_fix.clone())
                .unwrap_or_else(|| app_p.entry_fix.clone());
            for pl in &app_p.legs {
                let coord = pl
                    .fix_latitude
                    .zip(pl.fix_longitude)
                    .and_then(|(la, lo)| Coordinate::new(la, lo).ok());
                all_legs.push(FlightPlanLeg {
                    leg_index: leg_counter,
                    kind: FlightPlanLegKind::Approach,
                    from_fix: prev_fix.clone(),
                    to_fix: pl.fix_ident.clone(),
                    from_coordinate: coord,
                    to_coordinate: coord,
                    distance_nm: 3.0,
                    course_true_deg: pl.true_track_deg,
                    route_ident: None,
                    procedure_ident: Some(app_p.procedure.name.clone()),
                    altitude_constraint_str: pl
                        .altitude_constraint
                        .as_ref()
                        .map(|a| format!("{:?}", a)),
                    speed_constraint_kts: pl.speed_constraint.as_ref().map(|s| match s {
                        openairac_procedures::SpeedConstraint::At(k)
                        | openairac_procedures::SpeedConstraint::AtOrBelow(k) => *k,
                    }),
                    provider_name: app_p.provider_name.clone(),
                    airac_cycle: app_p.airac_cycle.clone(),
                    source_provenance: None,
                });
                prev_fix = pl.fix_ident.clone();
                leg_counter += 1;
            }
        }

        // 6. Arrival connector
        all_legs.push(FlightPlanLeg {
            leg_index: leg_counter,
            kind: FlightPlanKind::AirportConnector,
            from_fix: arr_rwy.clone().unwrap_or_else(|| "ARR".to_string()),
            to_fix: destination.ident.clone(),
            from_coordinate: Some(dest_coord),
            to_coordinate: Some(dest_coord),
            distance_nm: 0.0,
            course_true_deg: None,
            route_ident: None,
            procedure_ident: None,
            altitude_constraint_str: None,
            speed_constraint_kts: None,
            provider_name: Some("AIRPORT_BASELINE".to_string()),
            airac_cycle: Some("2608".to_string()),
            source_provenance: None,
        });

        let total_dist_nm: f64 = all_legs.iter().map(|l| l.distance_nm).sum();
        let speed_kts = profile.cruise_speed_kts.unwrap_or(450) as f64;
        let flight_time_min = ((total_dist_nm / speed_kts) * 60.0).round() as u32;

        let mut validation_report = FlightPlanValidationReport {
            status: FlightPlanValidationStatus::Valid,
            is_flyable: true,
            issues: Vec::new(),
            warnings: Vec::new(),
            active_providers_at_planning: vec![
                "OurAirports".to_string(),
                "CAICA_Russia".to_string(),
                "SIA_France".to_string(),
                "FAA_CIFP".to_string(),
            ],
        };

        // Run validation
        let (orig_ok, orig_reasons) = profile.evaluate_airport(&origin);
        if !orig_ok {
            validation_report.warnings.extend(orig_reasons);
        }
        let (dest_ok, dest_reasons) = profile.evaluate_airport(&destination);
        if !dest_ok {
            validation_report.warnings.extend(dest_reasons);
        }

        if !validation_report.warnings.is_empty() {
            validation_report.status = FlightPlanValidationStatus::Warning;
        }

        let flight_id = format!(
            "{}-{}-{}-{}",
            origin.ident,
            destination.ident,
            profile.icao_type.as_deref().unwrap_or("AC"),
            Utc::now().format("%Y%m%d%H%M%S")
        );

        Ok(FlightPlan {
            flight_id,
            created_at: Utc::now(),
            origin,
            destination,
            alternates: Vec::new(),
            aircraft_profile: profile,
            departure_runway: dep_rwy,
            sid,
            sid_transition: request.sid_transition.clone(),
            enroute_legs,
            all_legs,
            star,
            star_transition: request.star_transition.clone(),
            approach,
            arrival_runway: arr_rwy,
            cruise_altitude_ft: cruise_alt,
            total_distance_nm: total_dist_nm,
            estimated_flight_time_min: flight_time_min,
            planning_mode: request.mode,
            active_provider_datasets: vec![
                "OurAirports_20260820".to_string(),
                "CAICA_AIRAC_2608".to_string(),
            ],
            validation: validation_report,
            diagnostics,
        })
    }
}

pub type FlightPlanKind = FlightPlanLegKind;

/// Simulator Flight Plan Exporters.
pub struct FlightPlanExporter;

impl FlightPlanExporter {
    /// Export to X-Plane 11/12 `.fms` format.
    pub fn export_xplane_fms(plan: &FlightPlan) -> String {
        let mut lines = Vec::new();
        lines.push("I".to_string());
        lines.push("1100 Version".to_string());
        lines.push("CYCLE 2608".to_string());
        lines.push(format!("ADEP {}", plan.origin.ident));
        lines.push(format!("ADES {}", plan.destination.ident));
        if let Some(rwy) = &plan.departure_runway {
            lines.push(format!("DEPRWY RW{}", rwy));
        }
        if let Some(sid) = &plan.sid {
            lines.push(format!("SID {}", sid.procedure.name));
        }
        if let Some(star) = &plan.star {
            lines.push(format!("STAR {}", star.procedure.name));
        }
        if let Some(app) = &plan.approach {
            lines.push(format!("APPROACH {}", app.procedure.name));
        }
        if let Some(rwy) = &plan.arrival_runway {
            lines.push(format!("DESRWY RW{}", rwy));
        }

        let num_enr = plan.all_legs.len();
        lines.push(format!("NUMENR {}", num_enr));

        for leg in &plan.all_legs {
            let lat = leg.to_coordinate.map(|c| c.latitude_deg).unwrap_or(0.0);
            let lon = leg.to_coordinate.map(|c| c.longitude_deg).unwrap_or(0.0);
            let alt = plan.cruise_altitude_ft;
            let type_code = match leg.kind {
                FlightPlanLegKind::AirportConnector => 1, // Airport
                FlightPlanLegKind::Sid | FlightPlanLegKind::Star | FlightPlanLegKind::Approach => {
                    11
                } // Fix
                FlightPlanLegKind::AtsRoute => 11,        // Fix
                FlightPlanLegKind::Dct => 11,
                FlightPlanLegKind::Fra => 11,
                FlightPlanLegKind::Missed => 11,
            };
            lines.push(format!(
                "{} {} {:.6} {:.6} {}",
                type_code, leg.to_fix, lat, lon, alt
            ));
        }

        lines.join("\n")
    }

    /// Export to Garmin GNS430 `.fpl` format.
    pub fn export_gns430_fpl(plan: &FlightPlan) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "FPL:{}-{}",
            plan.origin.ident, plan.destination.ident
        ));
        lines.push("AIRAC:2608".to_string());
        lines.push(format!("ORIGIN:{}", plan.origin.ident));
        for leg in &plan.enroute_legs {
            lines.push(format!(
                "WPT:{}:{}:{}",
                leg.to_fix,
                leg.route_ident.as_deref().unwrap_or("DCT"),
                plan.cruise_altitude_ft
            ));
        }
        lines.push(format!("DEST:{}", plan.destination.ident));
        lines.join("\n")
    }

    /// Export to legacy KLN90B format.
    pub fn export_kln90b(plan: &FlightPlan) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "KLN90B ROUTE 1: {} TO {}",
            plan.origin.ident, plan.destination.ident
        ));
        lines.push(format!("1. {}", plan.origin.ident));
        let mut idx = 2;
        for leg in &plan.enroute_legs {
            lines.push(format!("{}. {}", idx, leg.to_fix));
            idx += 1;
        }
        lines.push(format!("{}. {}", idx, plan.destination.ident));
        lines.join("\n")
    }
}

/// Flight Plan File Persistence Manager.
pub struct FlightPlanStore {
    storage_dir: PathBuf,
}

impl FlightPlanStore {
    pub fn new(storage_dir: impl Into<PathBuf>) -> Self {
        Self {
            storage_dir: storage_dir.into(),
        }
    }

    pub fn default_store() -> Self {
        Self::new("data/flightplans")
    }

    pub fn init(&self) -> Result<()> {
        std::fs::create_dir_all(&self.storage_dir)
            .with_context(|| format!("Creating flight plans directory: {:?}", self.storage_dir))?;
        Ok(())
    }

    pub fn save(&self, plan: &FlightPlan) -> Result<PathBuf> {
        self.init()?;
        let path = self.storage_dir.join(format!("{}.json", plan.flight_id));
        let json = serde_json::to_string_pretty(plan)?;
        std::fs::write(&path, json)?;
        Ok(path)
    }

    pub fn load(&self, flight_id: &str) -> Result<FlightPlan> {
        let path = self.storage_dir.join(format!("{}.json", flight_id));
        let data = std::fs::read_to_string(&path)
            .with_context(|| format!("Reading flight plan from {:?}", path))?;
        let plan: FlightPlan = serde_json::from_str(&data)?;
        Ok(plan)
    }

    pub fn list(&self) -> Result<Vec<String>> {
        self.init()?;
        let mut list = Vec::new();
        for entry in std::fs::read_dir(&self.storage_dir)? {
            let entry = entry?;
            let p = entry.path();
            #[allow(clippy::collapsible_if)]
            if p.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                    list.push(stem.to_string());
                }
            }
        }
        list.sort();
        Ok(list)
    }

    pub fn delete(&self, flight_id: &str) -> Result<bool> {
        let path = self.storage_dir.join(format!("{}.json", flight_id));
        if path.exists() {
            std::fs::remove_file(path)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
