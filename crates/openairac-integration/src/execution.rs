//! OpenAIRAC Flight Execution Engine: active leg tracking, sequencing,
//! flight phase automation with hysteresis, TOD profile monitoring,
//! telemetry quality assurance, and FlightdeckOS-ready execution snapshots.

use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use openairac_routing::Coordinate;
use serde::{Deserialize, Serialize};

use crate::{FlightPlan, FlightPlanLeg, FlightPlanLegKind, PlannedProcedure};

/// Standard simulator flight phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlightPhase {
    #[default]
    Preflight,
    TaxiOut,
    Takeoff,
    InitialClimb,
    Climb,
    Cruise,
    Descent,
    Approach,
    Landing,
    TaxiIn,
    Parked,
    GoAround,
    Unknown,
}

impl FlightPhase {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Preflight => "preflight",
            Self::TaxiOut => "taxi_out",
            Self::Takeoff => "takeoff",
            Self::InitialClimb => "initial_climb",
            Self::Climb => "climb",
            Self::Cruise => "cruise",
            Self::Descent => "descent",
            Self::Approach => "approach",
            Self::Landing => "landing",
            Self::TaxiIn => "taxi_in",
            Self::Parked => "parked",
            Self::GoAround => "go_around",
            Self::Unknown => "unknown",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Preflight => "PREFLIGHT",
            Self::TaxiOut => "TAXI OUT",
            Self::Takeoff => "TAKEOFF",
            Self::InitialClimb => "INITIAL CLIMB",
            Self::Climb => "CLIMB",
            Self::Cruise => "CRUISE",
            Self::Descent => "DESCENT",
            Self::Approach => "APPROACH",
            Self::Landing => "LANDING",
            Self::TaxiIn => "TAXI IN",
            Self::Parked => "PARKED",
            Self::GoAround => "GO AROUND",
            Self::Unknown => "UNKNOWN",
        }
    }

    pub fn is_airborne(&self) -> bool {
        matches!(
            self,
            Self::InitialClimb
                | Self::Climb
                | Self::Cruise
                | Self::Descent
                | Self::Approach
                | Self::GoAround
        )
    }

    pub fn is_terminal_arrival(&self) -> bool {
        matches!(self, Self::Approach | Self::Landing | Self::GoAround)
    }
}

/// Incoming telemetry sample from simulator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryUpdate {
    pub timestamp: DateTime<Utc>,
    pub latitude_deg: f64,
    pub longitude_deg: f64,
    pub altitude_msl_ft: f64,
    pub altitude_agl_ft: Option<f64>,
    pub groundspeed_kts: f64,
    pub track_true_deg: f64,
    pub vertical_speed_fpm: f64,
    pub on_ground: bool,
    pub paused: bool,
    pub sim_rate: f64,
}

impl TelemetryUpdate {
    pub fn coordinate(&self) -> Result<Coordinate> {
        if self.latitude_deg.is_nan() || self.longitude_deg.is_nan() {
            return Err(anyhow!("Invalid NaN coordinate"));
        }
        Coordinate::new(self.latitude_deg, self.longitude_deg)
    }
}

/// Flight phase inference engine with hysteresis to prevent phase flapping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightPhaseEngine {
    current_phase: FlightPhase,
    evidence: String,
    consecutive_ticks: usize,
    has_been_airborne: bool,
    max_altitude_seen_ft: f64,
    departure_elevation_ft: f64,
    destination_elevation_ft: f64,
    cruise_altitude_ft: f64,
}

impl FlightPhaseEngine {
    pub fn new(
        departure_elevation_ft: f64,
        destination_elevation_ft: f64,
        cruise_altitude_ft: f64,
    ) -> Self {
        Self {
            current_phase: FlightPhase::Preflight,
            evidence: "Initialized preflight on ground".to_string(),
            consecutive_ticks: 0,
            has_been_airborne: false,
            max_altitude_seen_ft: departure_elevation_ft,
            departure_elevation_ft,
            destination_elevation_ft,
            cruise_altitude_ft,
        }
    }

    pub fn current_phase(&self) -> FlightPhase {
        self.current_phase
    }

    pub fn evidence(&self) -> &str {
        &self.evidence
    }

    pub fn process_telemetry(
        &mut self,
        telem: &TelemetryUpdate,
        distance_to_dest_nm: f64,
    ) -> (FlightPhase, Option<String>) {
        if telem.altitude_msl_ft > self.max_altitude_seen_ft {
            self.max_altitude_seen_ft = telem.altitude_msl_ft;
        }

        let raw_phase = self.infer_raw(telem, distance_to_dest_nm);
        let mut phase_transition_event = None;

        if raw_phase == self.current_phase {
            self.consecutive_ticks = 0;
        } else {
            // Apply hysteresis rules
            let required_ticks = match raw_phase {
                FlightPhase::Takeoff | FlightPhase::Landing | FlightPhase::InitialClimb => 1,
                FlightPhase::GoAround => 1,
                FlightPhase::Climb | FlightPhase::Descent => 3,
                FlightPhase::Cruise => 3,
                FlightPhase::TaxiOut | FlightPhase::TaxiIn | FlightPhase::Parked => 2,
                _ => 2,
            };

            self.consecutive_ticks += 1;
            if self.consecutive_ticks >= required_ticks {
                let old_phase = self.current_phase;
                self.current_phase = raw_phase;
                self.consecutive_ticks = 0;

                if self.current_phase.is_airborne() {
                    self.has_been_airborne = true;
                }

                let desc = format!(
                    "Flight phase transitioned from {} to {}: {}",
                    old_phase.display_name(),
                    self.current_phase.display_name(),
                    self.evidence
                );
                phase_transition_event = Some(desc);
            }
        }

        (self.current_phase, phase_transition_event)
    }

    fn infer_raw(&mut self, telem: &TelemetryUpdate, distance_to_dest_nm: f64) -> FlightPhase {
        let agl = telem
            .altitude_agl_ft
            .unwrap_or(telem.altitude_msl_ft - self.departure_elevation_ft);

        if telem.on_ground {
            if !self.has_been_airborne {
                if telem.groundspeed_kts < 5.0 {
                    self.evidence = "Stationary at departure airport".to_string();
                    FlightPhase::Preflight
                } else if telem.groundspeed_kts < 40.0 {
                    self.evidence = format!("Taxiing at {:.0} kts", telem.groundspeed_kts);
                    FlightPhase::TaxiOut
                } else {
                    self.evidence = format!("Takeoff roll at {:.0} kts", telem.groundspeed_kts);
                    FlightPhase::Takeoff
                }
            } else if telem.groundspeed_kts > 35.0 {
                self.evidence = format!("Landing rollout at {:.0} kts", telem.groundspeed_kts);
                FlightPhase::Landing
            } else if telem.groundspeed_kts > 3.0 {
                self.evidence = format!("Taxiing in at {:.0} kts", telem.groundspeed_kts);
                FlightPhase::TaxiIn
            } else {
                self.evidence = "Parked at destination".to_string();
                FlightPhase::Parked
            }
        } else {
            // Airborne
            let alt_above_dep = telem.altitude_msl_ft - self.departure_elevation_ft;
            let alt_above_dest = telem.altitude_msl_ft - self.destination_elevation_ft;

            if !self.has_been_airborne && alt_above_dep < 1500.0 && telem.vertical_speed_fpm > 300.0
            {
                self.evidence = format!(
                    "Initial climb out: {:.0} ft AGL, VS +{:.0} FPM",
                    alt_above_dep, telem.vertical_speed_fpm
                );
                FlightPhase::InitialClimb
            } else if self.has_been_airborne
                && distance_to_dest_nm < 15.0
                && alt_above_dest < 3000.0
                && telem.vertical_speed_fpm > 700.0
            {
                self.evidence = format!(
                    "Go-around detected: +{:.0} FPM near destination",
                    telem.vertical_speed_fpm
                );
                FlightPhase::GoAround
            } else if distance_to_dest_nm < 25.0
                && (alt_above_dest < 4500.0 || agl < 4000.0)
                && telem.vertical_speed_fpm <= 100.0
            {
                self.evidence = format!(
                    "Terminal approach: {:.1} NM to dest, alt {:.0} ft",
                    distance_to_dest_nm, telem.altitude_msl_ft
                );
                FlightPhase::Approach
            } else if telem.vertical_speed_fpm < -350.0
                && (distance_to_dest_nm < 150.0
                    || telem.altitude_msl_ft < self.cruise_altitude_ft - 1000.0)
            {
                self.evidence = format!(
                    "Descent active: VS {:.0} FPM towards destination ({:.1} NM)",
                    telem.vertical_speed_fpm, distance_to_dest_nm
                );
                FlightPhase::Descent
            } else if telem.vertical_speed_fpm > 350.0
                && telem.altitude_msl_ft < self.cruise_altitude_ft - 500.0
            {
                self.evidence = format!(
                    "Climbing: VS +{:.0} FPM, current alt {:.0} ft",
                    telem.vertical_speed_fpm, telem.altitude_msl_ft
                );
                FlightPhase::Climb
            } else {
                self.evidence = format!(
                    "Cruise: alt {:.0} ft (target {:.0} ft), GS {:.0} kts",
                    telem.altitude_msl_ft, self.cruise_altitude_ft, telem.groundspeed_kts
                );
                FlightPhase::Cruise
            }
        }
    }
}

/// A structured runtime execution event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightExecutionEvent {
    pub timestamp: DateTime<Utc>,
    pub event_type: String,
    pub description: String,
    pub metadata: serde_json::Value,
}

/// High-level flight progress and navigation telemetry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightProgress {
    pub active_leg_index: usize,
    pub prev_fix: Option<String>,
    pub active_leg_name: String,
    pub next_fix: Option<String>,
    pub xtk_nm: f64,
    pub desired_track_deg: f64,
    pub track_error_deg: f64,
    pub distance_to_next_fix_nm: f64,
    pub remaining_route_distance_nm: f64,
    pub direct_distance_to_destination_nm: f64,
    pub ete_next_fix_sec: Option<u32>,
    pub eta_next_fix: Option<DateTime<Utc>>,
    pub ete_destination_sec: Option<u32>,
    pub eta_destination: Option<DateTime<Utc>>,
    pub current_phase: FlightPhase,
    pub phase_evidence: String,
    pub procedure_context: String,
    pub tod_distance_nm: Option<f64>,
    pub tod_coordinate: Option<Coordinate>,
    pub descent_profile_deviation_ft: Option<f64>,
    pub required_descent_rate_fpm: Option<f64>,
    pub is_off_route: bool,
    pub telemetry_stale: bool,
    pub sim_connected: bool,
}

/// Lightweight completed flight record for persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletedFlightRecord {
    pub flight_id: String,
    pub origin_icao: String,
    pub destination_icao: String,
    pub aircraft_ident: String,
    pub departure_time: DateTime<Utc>,
    pub arrival_time: DateTime<Utc>,
    pub duration_minutes: u32,
    pub total_distance_nm: f64,
    pub max_altitude_ft: f64,
    pub route_string: String,
    pub provider_datasets: Vec<String>,
    pub completion_status: String,
}

/// First-class in-flight execution manager.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightExecutionSession {
    pub session_id: String,
    pub flight_plan: FlightPlan,
    pub start_time: DateTime<Utc>,
    pub last_telemetry_time: Option<DateTime<Utc>>,
    pub active_leg_index: usize,
    pub flown_leg_indices: Vec<usize>,
    pub phase_engine: FlightPhaseEngine,
    pub events: Vec<FlightExecutionEvent>,
    pub is_connected: bool,
    pub max_altitude_seen_ft: f64,
    pub total_distance_flown_nm: f64,
    pub last_position: Option<Coordinate>,
    pub last_telemetry: Option<TelemetryUpdate>,
}

impl FlightExecutionSession {
    /// Create a new execution session for a flight plan.
    pub fn new(flight_plan: FlightPlan) -> Self {
        let dep_elev = flight_plan.origin.elevation_ft.unwrap_or(0.0);
        let dest_elev = flight_plan.destination.elevation_ft.unwrap_or(0.0);
        let cruise_alt = flight_plan.cruise_altitude_ft as f64;

        let session_id = format!(
            "exec_{}_{}_{}",
            flight_plan.flight_id,
            flight_plan.origin.ident,
            Utc::now().timestamp()
        );

        let mut session = Self {
            session_id,
            flight_plan,
            start_time: Utc::now(),
            last_telemetry_time: None,
            active_leg_index: 0,
            flown_leg_indices: Vec::new(),
            phase_engine: FlightPhaseEngine::new(dep_elev, dest_elev, cruise_alt),
            events: Vec::new(),
            is_connected: false,
            max_altitude_seen_ft: dep_elev,
            total_distance_flown_nm: 0.0,
            last_position: None,
            last_telemetry: None,
        };

        session.record_event(
            "SESSION_CREATED",
            "Flight execution session initialized",
            serde_json::json!({
                "flight_id": session.flight_plan.flight_id,
                "origin": session.flight_plan.origin.ident,
                "destination": session.flight_plan.destination.ident,
            }),
        );

        session
    }

    pub fn record_event(
        &mut self,
        event_type: &str,
        description: &str,
        metadata: serde_json::Value,
    ) {
        self.events.push(FlightExecutionEvent {
            timestamp: Utc::now(),
            event_type: event_type.to_string(),
            description: description.to_string(),
            metadata,
        });
    }

    /// Process a new telemetry update and compute deterministic flight progress.
    pub fn update_telemetry(&mut self, telem: TelemetryUpdate) -> Result<FlightProgress> {
        let cur_coord = telem.coordinate()?;
        self.is_connected = true;
        self.last_telemetry_time = Some(telem.timestamp);

        if telem.altitude_msl_ft > self.max_altitude_seen_ft {
            self.max_altitude_seen_ft = telem.altitude_msl_ft;
        }

        if let Some(last_pos) = self.last_position {
            let delta = last_pos.distance_nm(&cur_coord);
            if delta < 50.0 {
                // Ignore teleports
                self.total_distance_flown_nm += delta;
            }
        }
        self.last_position = Some(cur_coord);

        // Compute direct destination distance
        let dest_geo = Coordinate::new(
            self.flight_plan.destination.latitude,
            self.flight_plan.destination.longitude,
        )?;
        let direct_dest_dist_nm = cur_coord.distance_nm(&dest_geo);

        // Process flight phase engine
        let (phase, phase_evt) = self
            .phase_engine
            .process_telemetry(&telem, direct_dest_dist_nm);
        if let Some(desc) = phase_evt {
            self.record_event(
                "PHASE_CHANGED",
                &desc,
                serde_json::json!({
                    "phase": phase.as_str(),
                    "altitude_ft": telem.altitude_msl_ft,
                    "groundspeed_kts": telem.groundspeed_kts,
                }),
            );
        }

        // Advance and compute active leg sequencing
        self.sequence_active_leg(&cur_coord);

        // Extract active leg geometry
        let legs = &self.flight_plan.all_legs;
        let mut xtk_nm = 0.0;
        let mut desired_track_deg = 0.0;
        let mut dist_to_next_fix = direct_dest_dist_nm;
        let mut prev_fix = None;
        let mut next_fix = None;
        let mut active_leg_name = "DIRECT TO DEST".to_string();
        let mut procedure_context = "ENROUTE".to_string();

        if let Some(active_leg) = legs.get(self.active_leg_index) {
            prev_fix = Some(active_leg.from_fix.clone());
            next_fix = Some(active_leg.to_fix.clone());
            active_leg_name = format!("{} -> {}", active_leg.from_fix, active_leg.to_fix);

            procedure_context = match active_leg.kind {
                FlightPlanLegKind::AirportConnector => "AIRPORT".to_string(),
                FlightPlanLegKind::Sid => "SID".to_string(),
                FlightPlanLegKind::AtsRoute => {
                    format!("ATS ({})", active_leg.route_ident.as_deref().unwrap_or(""))
                }
                FlightPlanLegKind::Dct | FlightPlanLegKind::Fra => "DCT".to_string(),
                FlightPlanLegKind::Star => "STAR".to_string(),
                FlightPlanLegKind::Approach => "APPROACH".to_string(),
                FlightPlanLegKind::Missed => "MISSED".to_string(),
            };

            if let (Some(from_geo), Some(to_geo)) =
                (active_leg.from_coordinate, active_leg.to_coordinate)
            {
                xtk_nm = Coordinate::cross_track_distance_nm(&from_geo, &to_geo, &cur_coord);
                desired_track_deg = from_geo.bearing_to(&to_geo);
                dist_to_next_fix = cur_coord.distance_nm(&to_geo);
            }
        }

        // Track error
        let mut track_error_deg = (telem.track_true_deg - desired_track_deg).abs();
        if track_error_deg > 180.0 {
            track_error_deg = 360.0 - track_error_deg;
        }

        // Remaining route distance
        let mut remaining_route_dist_nm = dist_to_next_fix;
        for leg in legs.iter().skip(self.active_leg_index + 1) {
            remaining_route_dist_nm += leg.distance_nm;
        }

        // ETE / ETA calculations (clean handling of stationary / low GS)
        let (ete_next_sec, eta_next, ete_dest_sec, eta_dest) =
            if telem.groundspeed_kts >= 15.0 && !telem.paused {
                let ete_n = ((dist_to_next_fix / telem.groundspeed_kts) * 3600.0) as u32;
                let eta_n = telem.timestamp + chrono::Duration::seconds(ete_n as i64);

                let ete_d = ((remaining_route_dist_nm / telem.groundspeed_kts) * 3600.0) as u32;
                let eta_d = telem.timestamp + chrono::Duration::seconds(ete_d as i64);

                (Some(ete_n), Some(eta_n), Some(ete_d), Some(eta_d))
            } else {
                (None, None, None, None)
            };

        // TOD calculation
        let dest_elev = self.flight_plan.destination.elevation_ft.unwrap_or(0.0);
        let delta_alt_ft = (telem.altitude_msl_ft - (dest_elev + 1500.0)).max(0.0);
        let required_descent_dist_nm = (delta_alt_ft / 1000.0) * 3.0 + 5.0;
        let tod_dist_nm = (remaining_route_dist_nm - required_descent_dist_nm).max(0.0);

        // Required vertical speed for descent monitor
        let required_fpm = if remaining_route_dist_nm > 0.1 && telem.groundspeed_kts >= 50.0 {
            let flight_time_min = remaining_route_dist_nm / (telem.groundspeed_kts / 60.0);
            Some(-(delta_alt_ft / flight_time_min))
        } else {
            None
        };

        // Ideal profile altitude
        let ideal_profile_alt = dest_elev + 1500.0 + (remaining_route_dist_nm / 3.0) * 1000.0;
        let profile_dev_ft = if remaining_route_dist_nm < 120.0 {
            Some(telem.altitude_msl_ft - ideal_profile_alt)
        } else {
            None
        };

        let is_off_route = xtk_nm.abs() > 5.0;
        self.last_telemetry = Some(telem);

        Ok(FlightProgress {
            active_leg_index: self.active_leg_index,
            prev_fix,
            active_leg_name,
            next_fix,
            xtk_nm,
            desired_track_deg,
            track_error_deg,
            distance_to_next_fix_nm: dist_to_next_fix,
            remaining_route_distance_nm: remaining_route_dist_nm,
            direct_distance_to_destination_nm: direct_dest_dist_nm,
            ete_next_fix_sec: ete_next_sec,
            eta_next_fix: eta_next,
            ete_destination_sec: ete_dest_sec,
            eta_destination: eta_dest,
            current_phase: phase,
            phase_evidence: self.phase_engine.evidence().to_string(),
            procedure_context,
            tod_distance_nm: Some(tod_dist_nm),
            tod_coordinate: None,
            descent_profile_deviation_ft: profile_dev_ft,
            required_descent_rate_fpm: required_fpm,
            is_off_route,
            telemetry_stale: false,
            sim_connected: true,
        })
    }

    /// Sequence active leg based on aircraft position and geometry.
    fn sequence_active_leg(&mut self, cur_coord: &Coordinate) {
        let legs = &self.flight_plan.all_legs;
        if self.active_leg_index >= legs.len() {
            return;
        }

        let cur_leg = &legs[self.active_leg_index];
        if let (Some(from_geo), Some(to_geo)) = (cur_leg.from_coordinate, cur_leg.to_coordinate) {
            let seg_dist = from_geo.distance_nm(&to_geo);
            let at_dist = Coordinate::along_track_distance_nm(&from_geo, &to_geo, cur_coord);
            let dist_to_next = cur_coord.distance_nm(&to_geo);

            let sequencing_radius = match cur_leg.kind {
                FlightPlanLegKind::Approach => 0.8,
                FlightPlanLegKind::Sid | FlightPlanLegKind::Star => 1.5,
                _ => 2.5,
            };

            // Sequence if along-track reached end of segment or within radius and passing
            if (at_dist >= seg_dist - 0.2 || dist_to_next < sequencing_radius)
                && self.active_leg_index + 1 < legs.len()
            {
                let old_leg = self.active_leg_index;
                self.flown_leg_indices.push(old_leg);
                self.active_leg_index += 1;

                self.record_event(
                    "FIX_SEQUENCED",
                    &format!(
                        "Sequenced waypoint {} -> {}",
                        cur_leg.to_fix, legs[self.active_leg_index].to_fix
                    ),
                    serde_json::json!({
                        "sequenced_fix": cur_leg.to_fix,
                        "new_active_leg": self.active_leg_index,
                    }),
                );
            }
        }
    }

    /// Handle simulator disconnection.
    pub fn handle_disconnect(&mut self) {
        self.is_connected = false;
        self.record_event(
            "SIM_DISCONNECTED",
            "Simulator telemetry stream disconnected",
            serde_json::json!({
                "last_active_leg": self.active_leg_index,
                "max_altitude_ft": self.max_altitude_seen_ft,
            }),
        );
    }

    /// Handle simulator reconnection.
    pub fn handle_reconnect(&mut self) {
        self.is_connected = true;
        self.record_event(
            "SIM_RECONNECTED",
            "Simulator telemetry stream restored",
            serde_json::json!({
                "active_leg": self.active_leg_index,
            }),
        );
    }

    /// Activate direct-to a target waypoint in the flight plan.
    pub fn activate_direct_to(&mut self, fix_ident: &str) -> Result<()> {
        let clean = fix_ident.trim().to_uppercase();
        let target_idx = self
            .flight_plan
            .all_legs
            .iter()
            .position(|l| l.to_fix.eq_ignore_ascii_case(&clean))
            .ok_or_else(|| anyhow!("Target fix '{}' not found in flight plan", fix_ident))?;

        for i in self.active_leg_index..target_idx {
            self.flown_leg_indices.push(i);
        }
        self.active_leg_index = target_idx;

        self.record_event(
            "DIRECT_TO_ACTIVATED",
            &format!("Direct-To {} activated", clean),
            serde_json::json!({
                "target_fix": clean,
                "new_active_leg": target_idx,
            }),
        );

        Ok(())
    }

    /// Mid-flight arrival replanning (runway, STAR, Approach).
    pub fn replan_arrival(
        &mut self,
        new_star: Option<PlannedProcedure>,
        new_approach: Option<PlannedProcedure>,
        new_runway: Option<String>,
    ) -> Result<()> {
        self.flight_plan.star = new_star;
        self.flight_plan.approach = new_approach;
        self.flight_plan.arrival_runway = new_runway.clone();

        // Reconstruct all_legs while preserving flown enroute legs
        let mut new_all_legs = Vec::new();
        // 1. Keep departure & SID
        if let Some(dep_leg) = self
            .flight_plan
            .all_legs
            .first()
            .filter(|l| l.kind == FlightPlanLegKind::AirportConnector)
            .cloned()
        {
            new_all_legs.push(dep_leg);
        }
        if let Some(sid) = &self.flight_plan.sid {
            for l in &sid.legs {
                let coord = l.fix_latitude.and_then(|lat| {
                    l.fix_longitude
                        .and_then(|lon| Coordinate::new(lat, lon).ok())
                });
                let mut leg = FlightPlanLeg {
                    leg_index: new_all_legs.len(),
                    kind: FlightPlanLegKind::Sid,
                    from_fix: l.fix_ident.clone(),
                    to_fix: l.fix_ident.clone(),
                    from_coordinate: coord,
                    to_coordinate: coord,
                    distance_nm: 0.0,
                    course_true_deg: None,
                    route_ident: None,
                    procedure_ident: Some(sid.procedure.name.clone()),
                    altitude_constraint_str: None,
                    speed_constraint_kts: None,
                    provider_name: sid.provider_name.clone(),
                    airac_cycle: sid.airac_cycle.clone(),
                    source_provenance: None,
                };
                if let Some(prev) = new_all_legs.last() {
                    leg.from_fix = prev.to_fix.clone();
                    leg.from_coordinate = prev.to_coordinate;
                    if let (Some(f_geo), Some(t_geo)) = (leg.from_coordinate, leg.to_coordinate) {
                        leg.distance_nm = f_geo.distance_nm(&t_geo);
                    }
                }
                new_all_legs.push(leg);
            }
        }

        // 2. Enroute legs
        for leg in &self.flight_plan.enroute_legs {
            let mut l = leg.clone();
            l.leg_index = new_all_legs.len();
            new_all_legs.push(l);
        }

        // 3. New STAR
        if let Some(star) = &self.flight_plan.star {
            for l in &star.legs {
                let coord = l.fix_latitude.and_then(|lat| {
                    l.fix_longitude
                        .and_then(|lon| Coordinate::new(lat, lon).ok())
                });
                let mut leg = FlightPlanLeg {
                    leg_index: new_all_legs.len(),
                    kind: FlightPlanLegKind::Star,
                    from_fix: l.fix_ident.clone(),
                    to_fix: l.fix_ident.clone(),
                    from_coordinate: coord,
                    to_coordinate: coord,
                    distance_nm: 0.0,
                    course_true_deg: None,
                    route_ident: None,
                    procedure_ident: Some(star.procedure.name.clone()),
                    altitude_constraint_str: None,
                    speed_constraint_kts: None,
                    provider_name: star.provider_name.clone(),
                    airac_cycle: star.airac_cycle.clone(),
                    source_provenance: None,
                };
                if let Some(prev) = new_all_legs.last() {
                    leg.from_fix = prev.to_fix.clone();
                    leg.from_coordinate = prev.to_coordinate;
                    if let (Some(f_geo), Some(t_geo)) = (leg.from_coordinate, leg.to_coordinate) {
                        leg.distance_nm = f_geo.distance_nm(&t_geo);
                    }
                }
                new_all_legs.push(leg);
            }
        }

        // 4. New Approach
        if let Some(app) = &self.flight_plan.approach {
            for l in &app.legs {
                let coord = l.fix_latitude.and_then(|lat| {
                    l.fix_longitude
                        .and_then(|lon| Coordinate::new(lat, lon).ok())
                });
                let mut leg = FlightPlanLeg {
                    leg_index: new_all_legs.len(),
                    kind: FlightPlanLegKind::Approach,
                    from_fix: l.fix_ident.clone(),
                    to_fix: l.fix_ident.clone(),
                    from_coordinate: coord,
                    to_coordinate: coord,
                    distance_nm: 0.0,
                    course_true_deg: None,
                    route_ident: None,
                    procedure_ident: Some(app.procedure.name.clone()),
                    altitude_constraint_str: None,
                    speed_constraint_kts: None,
                    provider_name: app.provider_name.clone(),
                    airac_cycle: app.airac_cycle.clone(),
                    source_provenance: None,
                };
                if let Some(prev) = new_all_legs.last() {
                    leg.from_fix = prev.to_fix.clone();
                    leg.from_coordinate = prev.to_coordinate;
                    if let (Some(f_geo), Some(t_geo)) = (leg.from_coordinate, leg.to_coordinate) {
                        leg.distance_nm = f_geo.distance_nm(&t_geo);
                    }
                }
                new_all_legs.push(leg);
            }
        }

        // 5. Destination airport connector
        let dest_coord = Coordinate::new(
            self.flight_plan.destination.latitude,
            self.flight_plan.destination.longitude,
        )
        .ok();
        let mut dest_leg = FlightPlanLeg {
            leg_index: new_all_legs.len(),
            kind: FlightPlanLegKind::AirportConnector,
            from_fix: self.flight_plan.destination.ident.clone(),
            to_fix: self.flight_plan.destination.ident.clone(),
            from_coordinate: dest_coord,
            to_coordinate: dest_coord,
            distance_nm: 0.0,
            course_true_deg: None,
            route_ident: None,
            procedure_ident: None,
            altitude_constraint_str: None,
            speed_constraint_kts: None,
            provider_name: None,
            airac_cycle: None,
            source_provenance: None,
        };
        if let Some(prev) = new_all_legs.last() {
            dest_leg.from_fix = prev.to_fix.clone();
            dest_leg.from_coordinate = prev.to_coordinate;
            if let (Some(f_geo), Some(t_geo)) = (dest_leg.from_coordinate, dest_leg.to_coordinate) {
                dest_leg.distance_nm = f_geo.distance_nm(&t_geo);
            }
        }
        new_all_legs.push(dest_leg);

        self.flight_plan.all_legs = new_all_legs;

        self.record_event(
            "ARRIVAL_REPLANNED",
            &format!("Arrival replanned for runway {:?}", new_runway),
            serde_json::json!({
                "new_runway": new_runway,
                "has_star": self.flight_plan.star.is_some(),
                "has_approach": self.flight_plan.approach.is_some(),
            }),
        );

        Ok(())
    }

    /// Generate completed flight log.
    pub fn complete_flight(&mut self, status: &str) -> CompletedFlightRecord {
        let now = Utc::now();
        let duration_min = ((now - self.start_time).num_seconds().max(0) / 60) as u32;

        self.record_event(
            "FLIGHT_COMPLETED",
            &format!("Flight completed with status: {}", status),
            serde_json::json!({
                "duration_minutes": duration_min,
                "status": status,
            }),
        );

        CompletedFlightRecord {
            flight_id: self.flight_plan.flight_id.clone(),
            origin_icao: self.flight_plan.origin.ident.clone(),
            destination_icao: self.flight_plan.destination.ident.clone(),
            aircraft_ident: self
                .flight_plan
                .aircraft_profile
                .icao_type
                .clone()
                .unwrap_or_else(|| "GENERIC".to_string()),
            departure_time: self.start_time,
            arrival_time: now,
            duration_minutes: duration_min,
            total_distance_nm: self.total_distance_flown_nm,
            max_altitude_ft: self.max_altitude_seen_ft,
            route_string: self.flight_plan.route_string(),
            provider_datasets: self.flight_plan.active_provider_datasets.clone(),
            completion_status: status.to_string(),
        }
    }

    /// FlightdeckOS-ready unified snapshot.
    pub fn snapshot(&self) -> serde_json::Value {
        serde_json::json!({
            "session_id": self.session_id,
            "flight_id": self.flight_plan.flight_id,
            "origin": self.flight_plan.origin.ident,
            "destination": self.flight_plan.destination.ident,
            "aircraft": self
                .flight_plan
                .aircraft_profile
                .icao_type
                .as_deref()
                .unwrap_or("GENERIC"),
            "current_phase": self.phase_engine.current_phase().as_str(),
            "phase_evidence": self.phase_engine.evidence(),
            "active_leg_index": self.active_leg_index,
            "is_connected": self.is_connected,
            "max_altitude_ft": self.max_altitude_seen_ft,
            "distance_flown_nm": self.total_distance_flown_nm,
            "events_count": self.events.len(),
            "last_telemetry_time": self.last_telemetry_time,
        })
    }
}
