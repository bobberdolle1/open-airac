//! Reusable EFB Domain Calculations & Flight Phase Automation Engine.
//!
//! Provides deterministic flight phase state transitions, hysteresis, slew/teleport detection,
//! geodesic cross-track calculations, planning Top-of-Descent (TOD), runway wind components,
//! and contextual chart suggestion logic.

use crate::model::NormalizedChartType;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const EARTH_RADIUS_NM: f64 = 3440.065;

/// Deterministic flight phase classifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlightPhase {
    Preflight,
    TaxiOut,
    Takeoff,
    InitialClimb,
    Departure,
    Climb,
    Cruise,
    Descent,
    Arrival,
    Approach,
    Final,
    Landing,
    TaxiIn,
    Parked,
    Unknown,
}

impl FlightPhase {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Preflight => "PREFLIGHT",
            Self::TaxiOut => "TAXI_OUT",
            Self::Takeoff => "TAKEOFF",
            Self::InitialClimb => "INITIAL_CLIMB",
            Self::Departure => "DEPARTURE",
            Self::Climb => "CLIMB",
            Self::Cruise => "CRUISE",
            Self::Descent => "DESCENT",
            Self::Arrival => "ARRIVAL",
            Self::Approach => "APPROACH",
            Self::Final => "FINAL",
            Self::Landing => "LANDING",
            Self::TaxiIn => "TAXI_IN",
            Self::Parked => "PARKED",
            Self::Unknown => "UNKNOWN",
        }
    }

    pub fn is_airborne(&self) -> bool {
        matches!(
            self,
            Self::Takeoff
                | Self::InitialClimb
                | Self::Departure
                | Self::Climb
                | Self::Cruise
                | Self::Descent
                | Self::Arrival
                | Self::Approach
                | Self::Final
        )
    }

    pub fn is_terminal_arrival(&self) -> bool {
        matches!(
            self,
            Self::Arrival | Self::Approach | Self::Final | Self::Landing
        )
    }
}

/// Confidence level of flight phase inference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PhaseConfidence {
    High,
    Medium,
    Low,
    Unknown,
}

/// Flight phase assessment result with evidence trail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhaseAssessment {
    pub phase: FlightPhase,
    pub confidence: PhaseConfidence,
    pub evidence: String,
    pub timestamp: DateTime<Utc>,
}

/// Input aircraft telemetry for flight phase assessment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AircraftTelemetry {
    pub on_ground: bool,
    pub altitude_msl_ft: f64,
    pub altitude_agl_ft: Option<f64>,
    pub groundspeed_kt: f64,
    pub vertical_speed_fpm: f64,
    pub distance_to_dest_nm: Option<f64>,
    pub distance_from_dep_nm: Option<f64>,
    pub active_procedure_kind: Option<char>, // 'D'=SID, 'E'=STAR, 'F'=Approach
    pub timestamp: DateTime<Utc>,
}

/// Deterministic flight phase engine with hysteresis and slew protection.
#[derive(Debug, Clone)]
pub struct FlightPhaseEngine {
    current_phase: FlightPhase,
    consecutive_ticks: u32,
    last_telemetry: Option<AircraftTelemetry>,
    has_been_airborne: bool,
}

impl Default for FlightPhaseEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl FlightPhaseEngine {
    pub fn new() -> Self {
        Self {
            current_phase: FlightPhase::Preflight,
            consecutive_ticks: 0,
            last_telemetry: None,
            has_been_airborne: false,
        }
    }

    pub fn current_phase(&self) -> FlightPhase {
        self.current_phase
    }

    pub fn evaluate(&mut self, telem: &AircraftTelemetry) -> PhaseAssessment {
        // 1. Detect Teleportation / Slew artifacts
        if let Some(prev) = &self.last_telemetry {
            let dt_secs = (telem.timestamp - prev.timestamp).num_milliseconds() as f64 / 1000.0;
            if dt_secs > 0.0 && dt_secs < 5.0 {
                let alt_jump = (telem.altitude_msl_ft - prev.altitude_msl_ft).abs();
                if alt_jump > 10_000.0 {
                    // Sudden impossible altitude change -> slew/teleport detected!
                    self.current_phase = if telem.on_ground {
                        FlightPhase::Preflight
                    } else {
                        FlightPhase::Cruise
                    };
                    self.consecutive_ticks = 0;
                    self.last_telemetry = Some(telem.clone());
                    return PhaseAssessment {
                        phase: self.current_phase,
                        confidence: PhaseConfidence::Medium,
                        evidence: format!(
                            "Teleport/Slew detected (Altitude jump {alt_jump:.0} ft in {dt_secs:.1}s); state reset"
                        ),
                        timestamp: telem.timestamp,
                    };
                }
            }
        }
        let (candidate, evidence) = self.infer_raw_phase(telem);

        if !telem.on_ground && telem.groundspeed_kt > 50.0 {
            self.has_been_airborne = true;
        }

        // Hysteresis: require 2 consecutive ticks for major phase shift unless sudden liftoff/touchdown
        if candidate == self.current_phase {
            self.consecutive_ticks += 1;
        } else {
            let immediate_transition = (telem.on_ground
                && self.current_phase == FlightPhase::Final)
                || (!telem.on_ground
                    && matches!(
                        self.current_phase,
                        FlightPhase::Takeoff | FlightPhase::Preflight
                    ));

            if immediate_transition || self.consecutive_ticks >= 2 {
                self.current_phase = candidate;
                self.consecutive_ticks = 1;
            } else {
                self.consecutive_ticks += 1;
            }
        }
        self.last_telemetry = Some(telem.clone());

        PhaseAssessment {
            phase: self.current_phase,
            confidence: PhaseConfidence::High,
            evidence,
            timestamp: telem.timestamp,
        }
    }

    fn infer_raw_phase(&self, telem: &AircraftTelemetry) -> (FlightPhase, String) {
        if telem.on_ground {
            if !self.has_been_airborne {
                if telem.groundspeed_kt < 3.0 {
                    (
                        FlightPhase::Preflight,
                        "On ground, stationary (GS < 3 kt)".to_string(),
                    )
                } else if telem.groundspeed_kt < 45.0 {
                    (
                        FlightPhase::TaxiOut,
                        format!("On ground, taxiing out (GS {:.0} kt)", telem.groundspeed_kt),
                    )
                } else {
                    (
                        FlightPhase::Takeoff,
                        format!(
                            "On ground, takeoff roll (GS {:.0} kt)",
                            telem.groundspeed_kt
                        ),
                    )
                }
            } else if telem.groundspeed_kt > 45.0 {
                (
                    FlightPhase::Landing,
                    format!("Touchdown rollout (GS {:.0} kt)", telem.groundspeed_kt),
                )
            } else if telem.groundspeed_kt > 3.0 {
                (
                    FlightPhase::TaxiIn,
                    format!(
                        "On ground, taxiing to gate (GS {:.0} kt)",
                        telem.groundspeed_kt
                    ),
                )
            } else {
                (
                    FlightPhase::Parked,
                    "On ground, parked at destination (GS < 3 kt)".to_string(),
                )
            }
        } else {
            let agl = telem.altitude_agl_ft.unwrap_or(telem.altitude_msl_ft);
            let dist_dest = telem.distance_to_dest_nm.unwrap_or(999.0);
            let proc = telem.active_procedure_kind.unwrap_or(' ');

            if agl < 1500.0
                && telem.vertical_speed_fpm > 300.0
                && (self.current_phase == FlightPhase::Takeoff
                    || self.current_phase == FlightPhase::InitialClimb
                    || !self.has_been_airborne)
            {
                (
                    FlightPhase::InitialClimb,
                    format!(
                        "Airborne, climbing rapidly (VS {:.0} fpm, AGL {:.0} ft)",
                        telem.vertical_speed_fpm, agl
                    ),
                )
            } else if proc == 'D'
                || (telem.distance_from_dep_nm.unwrap_or(999.0) < 30.0
                    && telem.vertical_speed_fpm > 200.0)
            {
                (
                    FlightPhase::Departure,
                    "Flying SID / Terminal Departure phase".to_string(),
                )
            } else if proc == 'F'
                || (dist_dest < 15.0 && agl < 4000.0 && telem.vertical_speed_fpm < -100.0)
            {
                if dist_dest < 5.0 && agl < 1500.0 {
                    (
                        FlightPhase::Final,
                        format!(
                            "On final approach segment (Dist {:.1} NM, AGL {:.0} ft)",
                            dist_dest, agl
                        ),
                    )
                } else {
                    (
                        FlightPhase::Approach,
                        format!(
                            "On instrument approach procedure (Dist {:.1} NM)",
                            dist_dest
                        ),
                    )
                }
            } else if proc == 'E' || (dist_dest < 60.0 && telem.vertical_speed_fpm < -200.0) {
                (
                    FlightPhase::Arrival,
                    format!(
                        "Terminal Arrival (STAR) / Descent towards destination (Dist {:.1} NM)",
                        dist_dest
                    ),
                )
            } else if telem.vertical_speed_fpm < -300.0 && dist_dest < 150.0 {
                (
                    FlightPhase::Descent,
                    format!(
                        "Enroute descent (VS {:.0} fpm, Dist {:.0} NM)",
                        telem.vertical_speed_fpm, dist_dest
                    ),
                )
            } else if telem.vertical_speed_fpm > 300.0 {
                (
                    FlightPhase::Climb,
                    format!(
                        "Enroute climb (VS {:.0} fpm, Alt {:.0} ft)",
                        telem.vertical_speed_fpm, telem.altitude_msl_ft
                    ),
                )
            } else {
                (
                    FlightPhase::Cruise,
                    format!(
                        "Enroute cruise (Alt {:.0} ft, GS {:.0} kt)",
                        telem.altitude_msl_ft, telem.groundspeed_kt
                    ),
                )
            }
        }
    }
}

/// Calculate runway wind components: headwind (+)/tailwind (-) and crosswind (right + / left -).
pub fn calculate_runway_wind_components(
    runway_heading_deg: f64,
    wind_dir_deg: f64,
    wind_speed_kt: f64,
) -> (f64, f64) {
    let diff_rad = (wind_dir_deg - runway_heading_deg).to_radians();
    let headwind = wind_speed_kt * diff_rad.cos();
    let crosswind = wind_speed_kt * diff_rad.sin();
    (headwind, crosswind)
}

/// Calculate planning Top-of-Descent (TOD) distance in NM from destination.
///
/// Uses standard 3.0° descent slope (~3 NM per 1000 ft altitude to lose) plus deceleration buffer.
pub fn calculate_planning_tod_nm(
    current_altitude_ft: f64,
    destination_elevation_ft: f64,
    decel_buffer_nm: f64,
) -> f64 {
    let alt_to_lose = (current_altitude_ft - destination_elevation_ft).max(0.0);
    (alt_to_lose / 1000.0) * 3.0 + decel_buffer_nm
}

/// Calculate cross-track distance (XTK) in NM from aircraft position to geodesic route segment.
pub fn calculate_cross_track_nm(
    aircraft_pos: (f64, f64), // (lon, lat)
    seg_start: (f64, f64),
    seg_end: (f64, f64),
) -> (f64, &'static str) {
    let seg_len = gc_distance_nm(seg_start, seg_end);
    if seg_len < 1e-4 {
        return (0.0, "ON");
    }

    let px = aircraft_pos.0;
    let py = aircraft_pos.1;
    let x1 = seg_start.0;
    let y1 = seg_start.1;
    let x2 = seg_end.0;
    let y2 = seg_end.1;

    let dx = x2 - x1;
    let dy = y2 - y1;

    let t = (((px - x1) * dx + (py - y1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    let proj_x = x1 + t * dx;
    let proj_y = y1 + t * dy;

    let dist = gc_distance_nm(aircraft_pos, (proj_x, proj_y));

    // Determine Left or Right via 2D cross product of segment vector and point vector
    let cross = (x2 - x1) * (py - y1) - (y2 - y1) * (px - x1);
    let side = if cross > 1e-6 {
        "L"
    } else if cross < -1e-6 {
        "R"
    } else {
        "ON"
    };

    (dist, side)
}

fn gc_distance_nm(p1: (f64, f64), p2: (f64, f64)) -> f64 {
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

/// Chart suggestion recommendation based on current flight phase and active procedure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChartSuggestion {
    pub suggested_category: NormalizedChartType,
    pub airport_target: &'static str, // "departure" or "destination"
    pub reason: &'static str,
}

impl ChartSuggestion {
    pub fn for_phase(phase: FlightPhase) -> Self {
        match phase {
            FlightPhase::Preflight | FlightPhase::TaxiOut => Self {
                suggested_category: NormalizedChartType::AirportDiagram,
                airport_target: "departure",
                reason: "Departure airport diagram for taxi-out",
            },
            FlightPhase::Takeoff | FlightPhase::InitialClimb | FlightPhase::Departure => Self {
                suggested_category: NormalizedChartType::Sid,
                airport_target: "departure",
                reason: "Standard Instrument Departure (SID) chart",
            },
            FlightPhase::Climb | FlightPhase::Cruise => Self {
                suggested_category: NormalizedChartType::GeneralInfo,
                airport_target: "departure",
                reason: "Enroute navigation & general reference",
            },
            FlightPhase::Descent | FlightPhase::Arrival => Self {
                suggested_category: NormalizedChartType::Star,
                airport_target: "destination",
                reason: "Standard Terminal Arrival Route (STAR) chart",
            },
            FlightPhase::Approach | FlightPhase::Final => Self {
                suggested_category: NormalizedChartType::Approach,
                airport_target: "destination",
                reason: "Instrument Approach Procedure (IAP) plate",
            },
            FlightPhase::Landing | FlightPhase::TaxiIn | FlightPhase::Parked => Self {
                suggested_category: NormalizedChartType::AirportDiagram,
                airport_target: "destination",
                reason: "Destination airport diagram for taxi-in & parking",
            },
            FlightPhase::Unknown => Self {
                suggested_category: NormalizedChartType::GeneralInfo,
                airport_target: "destination",
                reason: "General reference chart",
            },
        }
    }
}
