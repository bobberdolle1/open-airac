//! Deterministic rule-based Crew Advisory Engine.
//!
//! Evaluates operational navigation, descent profile, weather, runway winds,
//! telemetry freshness, and dataset source constraints without external LLM dependency.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::execution::{FlightExecutionSession, FlightPhase, FlightProgress};
use crate::flightdeck::snapshot::{FlightdeckOnlineAtc, FlightdeckWeatherSummary};

/// Severity levels for crew advisories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AdvisoryLevel {
    Info,
    Caution,
    Warning,
}

/// A structured deterministic advisory for the flight crew.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrewAdvisory {
    pub level: AdvisoryLevel,
    pub code: String,
    pub message: String,
    pub evidence: String,
    pub timestamp: DateTime<Utc>,
}

/// Rule-based deterministic advisory generator.
pub struct CrewAdvisoryEngine;

impl CrewAdvisoryEngine {
    /// Evaluate all deterministic advisory rules against the current session state.
    pub fn evaluate(
        session: &FlightExecutionSession,
        progress: Option<&FlightProgress>,
        weather: Option<&FlightdeckWeatherSummary>,
        _atc: &[FlightdeckOnlineAtc],
    ) -> Vec<CrewAdvisory> {
        let mut advisories = Vec::new();
        let now = Utc::now();
        let current_phase = progress
            .map(|p| p.current_phase)
            .unwrap_or_else(|| session.phase_engine.current_phase());
        // 1. Rule: TELEMETRY_STALE
        if let Some(last_time) = session.last_telemetry_time {
            let age_sec = now.signed_duration_since(last_time).num_seconds().max(0);
            if age_sec > 5 {
                advisories.push(CrewAdvisory {
                    level: AdvisoryLevel::Warning,
                    code: "TELEMETRY_STALE".to_string(),
                    message: format!("Simulator telemetry is stale ({age_sec}s since last packet)"),
                    evidence: format!(
                        "Last packet at {}, current time {}",
                        last_time.to_rfc3339(),
                        now.to_rfc3339()
                    ),
                    timestamp: now,
                });
            }
        } else if session.is_connected {
            advisories.push(CrewAdvisory {
                level: AdvisoryLevel::Warning,
                code: "NO_TELEMETRY_DATA".to_string(),
                message: "Simulator is connected but no telemetry packets received".to_string(),
                evidence: "last_telemetry_time is None".to_string(),
                timestamp: now,
            });
        }

        // 2. Rule: OFF_ROUTE
        if let Some(prog) = progress
            && prog.is_off_route
        {
            let side = if prog.xtk_nm < 0.0 { "LEFT" } else { "RIGHT" };
            advisories.push(CrewAdvisory {
                level: AdvisoryLevel::Warning,
                code: "OFF_ROUTE".to_string(),
                message: format!(
                    "Aircraft is off route ({:.1} NM {})",
                    prog.xtk_nm.abs(),
                    side
                ),
                evidence: format!(
                    "XTK {:.2} NM exceeds allowed corridor tolerance",
                    prog.xtk_nm
                ),
                timestamp: now,
            });
        }

        // 3. Rule: TOD_APPROACHING
        // 3. Rule: TOD_APPROACHING
        if current_phase == FlightPhase::Cruise
            && let Some(prog) = progress
            && let Some(tod_nm) = prog.tod_distance_nm
            && tod_nm <= 15.0
            && tod_nm > 0.0
        {
            advisories.push(CrewAdvisory {
                level: AdvisoryLevel::Caution,
                code: "TOD_APPROACHING".to_string(),
                message: format!("Approaching Top of Descent in {:.1} NM", tod_nm),
                evidence: format!(
                    "Cruise FL{:.0} -> Dest elev {:.0} ft, 3.0° descent profile",
                    session.flight_plan.cruise_altitude_ft,
                    session.flight_plan.destination.elevation_ft.unwrap_or(0.0)
                ),
                timestamp: now,
            });
        }

        // 4. Rule: DESCENT_REQUIRED (Past TOD, high above profile)
        // 4. Rule: DESCENT_REQUIRED (Past TOD, high above profile)
        if current_phase == FlightPhase::Cruise
            && let Some(prog) = progress
            && let Some(dev_ft) = prog.descent_profile_deviation_ft
            && dev_ft > 1500.0
            && prog.tod_distance_nm.map(|d| d <= 0.0).unwrap_or(true)
        {
            advisories.push(CrewAdvisory {
                level: AdvisoryLevel::Warning,
                code: "DESCENT_REQUIRED".to_string(),
                message: format!(
                    "Aircraft is {:.0} ft above 3.0° descent profile past TOD",
                    dev_ft
                ),
                evidence: format!(
                    "Current alt {:.0} ft, target profile {:.0} ft, required VS {:.0} fpm",
                    session
                        .last_telemetry
                        .as_ref()
                        .map(|t| t.altitude_msl_ft)
                        .unwrap_or(0.0),
                    session
                        .last_telemetry
                        .as_ref()
                        .map(|t| t.altitude_msl_ft)
                        .unwrap_or(0.0)
                        - dev_ft,
                    prog.required_descent_rate_fpm.unwrap_or(0.0)
                ),
                timestamp: now,
            });
        }

        // 5. Rule: SOURCE_REQUIRED_PROCEDURE
        if session.flight_plan.destination.ident == "URAS"
            || session
                .flight_plan
                .diagnostics
                .iter()
                .any(|d| d.contains("SOURCE_REQUIRED"))
        {
            advisories.push(CrewAdvisory {
                level: AdvisoryLevel::Caution,
                code: "SOURCE_REQUIRED_PROCEDURE".to_string(),
                message: format!("Terminal procedures for {} require official source dataset", session.flight_plan.destination.ident),
                evidence: "CAICA source required for Abkhazia/Crimea aerodromes where open AIP data is withheld".to_string(),
                timestamp: now,
            });
        }

        // 6. Rule: SIGNIFICANT_TAILWIND
        // 6. Rule: SIGNIFICANT_TAILWIND
        if let Some(wx) = weather
            && let Some(rw) = &wx.destination_runway_wind
            && rw.is_tailwind
            && rw.headwind_kts.abs() > 5.0
        {
            advisories.push(CrewAdvisory {
                level: AdvisoryLevel::Caution,
                code: "SIGNIFICANT_TAILWIND".to_string(),
                message: format!(
                    "Selected arrival runway {} has {:.0} kt tailwind",
                    rw.runway_ident,
                    rw.headwind_kts.abs()
                ),
                evidence: format!(
                    "Runway wind: {:.0} kt tailwind, {:.0} kt crosswind",
                    rw.headwind_kts.abs(),
                    rw.crosswind_kts
                ),
                timestamp: now,
            });
        }

        // 7. Rule: ALTITUDE_CONSTRAINT_APPROACHING
        // 7. Rule: ALTITUDE_CONSTRAINT_APPROACHING
        if session.active_leg_index < session.flight_plan.all_legs.len() {
            let leg = &session.flight_plan.all_legs[session.active_leg_index];
            if let Some(constraint) = &leg.altitude_constraint_str
                && let Some(prog) = progress
                && prog.distance_to_next_fix_nm <= 10.0
            {
                advisories.push(CrewAdvisory {
                    level: AdvisoryLevel::Info,
                    code: "ALTITUDE_CONSTRAINT_APPROACHING".to_string(),
                    message: format!(
                        "Approaching altitude constraint at {}: {}",
                        leg.to_fix, constraint
                    ),
                    evidence: format!(
                        "Distance to {}: {:.1} NM",
                        leg.to_fix, prog.distance_to_next_fix_nm
                    ),
                    timestamp: now,
                });
            }
        }

        advisories
    }
}
