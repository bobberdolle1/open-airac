//! Structured Departure, Arrival, and In-Flight Briefing models.
//!
//! Provides deterministic briefing packages for flight crew and AI narration
//! without synthesizing unbacked procedures or falsifying operational context.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::flightdeck::snapshot::{FlightdeckRunwayWind, FlightdeckSnapshotV2};

/// Structured Departure Briefing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightdeckDepartureBriefing {
    pub origin_icao: String,
    pub origin_name: String,
    pub elevation_ft: f64,
    pub departure_runway: Option<String>,
    pub sid_procedure: Option<String>,
    pub sid_transition: Option<String>,
    pub initial_altitude_constraints: Vec<String>,
    pub weather_metar: Option<String>,
    pub runway_wind: Option<FlightdeckRunwayWind>,
    pub provider_name: Option<String>,
    pub warnings: Vec<String>,
    pub briefing_text: String,
}

/// Structured Arrival Briefing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightdeckArrivalBriefing {
    pub destination_icao: String,
    pub destination_name: String,
    pub elevation_ft: f64,
    pub arrival_runway: Option<String>,
    pub star_procedure: Option<String>,
    pub star_transition: Option<String>,
    pub approach_procedure: Option<String>,
    pub approach_type: Option<String>,
    pub final_approach_restrictions: Vec<String>,
    pub weather_metar: Option<String>,
    pub weather_taf: Option<String>,
    pub runway_wind: Option<FlightdeckRunwayWind>,
    pub is_source_required: bool,
    pub source_required_note: Option<String>,
    pub provider_name: Option<String>,
    pub warnings: Vec<String>,
    pub briefing_text: String,
}

/// In-Flight Operational Briefing Summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InFlightBriefSummary {
    pub timestamp: DateTime<Utc>,
    pub flight_id: String,
    pub flight_phase: String,
    pub current_status: String,
    pub active_leg: String,
    pub next_fix_summary: String,
    pub tod_summary: String,
    pub arrival_summary: String,
    pub weather_summary: String,
    pub advisories_count: usize,
    pub text_narration: String,
}

impl InFlightBriefSummary {
    /// Generate a structured in-flight brief from a snapshot.
    pub fn from_snapshot(snap: &FlightdeckSnapshotV2) -> Self {
        let next_fix_summary = snap
            .active_leg
            .as_ref()
            .map(|l| {
                format!(
                    "Next fix {} in {:.1} NM (ETE: {})",
                    l.next_fix.as_deref().unwrap_or("DEST"),
                    snap.navigation_geometry.distance_to_next_fix_nm,
                    snap.navigation_geometry
                        .ete_next_fix_sec
                        .map(|s| format!("{}m {}s", s / 60, s % 60))
                        .unwrap_or_else(|| "--:--".to_string())
                )
            })
            .unwrap_or_else(|| "No active navigation leg".to_string());

        let tod_summary = if let Some(tod_nm) = snap.descent_profile.tod_distance_nm {
            format!("Top of Descent in {:.1} NM", tod_nm)
        } else {
            snap.descent_profile.profile_status.clone()
        };

        let arrival_summary = if snap.destination.is_source_required {
            format!(
                "Destination {} requires official source dataset for terminal procedures",
                snap.destination.ident
            )
        } else {
            format!(
                "Expect runway {} via {} / {}",
                snap.destination.selected_runway.as_deref().unwrap_or("TBD"),
                snap.destination
                    .procedure_name
                    .as_deref()
                    .unwrap_or("DIRECT"),
                snap.destination
                    .transition_name
                    .as_deref()
                    .unwrap_or("DEFAULT")
            )
        };

        let weather_summary = snap
            .weather_summary
            .destination_metar
            .clone()
            .unwrap_or_else(|| "Destination weather unavailable".to_string());

        let text_narration = format!(
            "Flight {} to {}. Currently in {} phase. {}. {}. {}. Active advisories: {}.",
            snap.origin.ident,
            snap.destination.ident,
            snap.flight_phase.display_name(),
            next_fix_summary,
            tod_summary,
            arrival_summary,
            snap.advisories.len()
        );

        Self {
            timestamp: Utc::now(),
            flight_id: snap.session_id.clone(),
            flight_phase: snap.flight_phase.display_name().to_string(),
            current_status: snap.connection_state.as_str().to_string(),
            active_leg: snap
                .active_leg
                .as_ref()
                .map(|l| l.leg_name.clone())
                .unwrap_or_else(|| "NONE".to_string()),
            next_fix_summary,
            tod_summary,
            arrival_summary,
            weather_summary,
            advisories_count: snap.advisories.len(),
            text_narration,
        }
    }
}
