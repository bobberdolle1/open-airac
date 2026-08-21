//! FlightdeckOS Integration Adapter.
//!
//! Translates OpenAIRAC canonical snapshots and events into standard FlightdeckOS
//! machine-consumable flight context payloads while keeping the systems decoupled.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::flightdeck::snapshot::FlightdeckSnapshotV2;

/// Standard FlightdeckOS flight context model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightdeckOsContext {
    pub flightdeck_os_version: String,
    pub session_id: String,
    pub timestamp_utc: DateTime<Utc>,
    pub flight_id: String,
    pub flight_phase: String,
    pub connection_status: String,
    pub telemetry_age_ms: u64,
    pub position: Option<FlightdeckOsPosition>,
    pub active_leg: Option<FlightdeckOsLeg>,
    pub next_waypoint: Option<FlightdeckOsWaypoint>,
    pub descent_guidance: FlightdeckOsDescentGuidance,
    pub destination_brief: FlightdeckOsDestinationBrief,
    pub active_advisories: Vec<FlightdeckOsAdvisory>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightdeckOsPosition {
    pub lat: f64,
    pub lon: f64,
    pub altitude_ft: f64,
    pub groundspeed_kts: f64,
    pub vertical_speed_fpm: f64,
    pub true_track_deg: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightdeckOsLeg {
    pub name: String,
    pub leg_type: String,
    pub desired_track_deg: f64,
    pub xtk_nm: f64,
    pub xtk_side: String,
    pub is_off_route: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightdeckOsWaypoint {
    pub ident: String,
    pub distance_nm: f64,
    pub ete_sec: Option<u32>,
    pub eta_utc: Option<DateTime<Utc>>,
    pub altitude_constraint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightdeckOsDescentGuidance {
    pub tod_distance_nm: Option<f64>,
    pub profile_status: String,
    pub required_vs_fpm: Option<f64>,
    pub profile_deviation_ft: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightdeckOsDestinationBrief {
    pub icao: String,
    pub runway: Option<String>,
    pub star: Option<String>,
    pub approach: Option<String>,
    pub metar: Option<String>,
    pub is_source_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightdeckOsAdvisory {
    pub severity: String,
    pub code: String,
    pub message: String,
}

/// FlightdeckOS integration adapter.
pub struct FlightdeckOsAdapter;

impl FlightdeckOsAdapter {
    /// Translate an OpenAIRAC FlightdeckSnapshotV2 into a FlightdeckOsContext.
    pub fn translate(snapshot: &FlightdeckSnapshotV2) -> FlightdeckOsContext {
        let position = snapshot.position.as_ref().map(|p| FlightdeckOsPosition {
            lat: p.latitude_deg,
            lon: p.longitude_deg,
            altitude_ft: p.altitude_msl_ft,
            groundspeed_kts: p.groundspeed_kts,
            vertical_speed_fpm: p.vertical_speed_fpm,
            true_track_deg: p.track_true_deg,
        });

        let active_leg = snapshot.active_leg.as_ref().map(|l| FlightdeckOsLeg {
            name: l.leg_name.clone(),
            leg_type: l.leg_type.clone(),
            desired_track_deg: l.desired_track_deg,
            xtk_nm: snapshot.navigation_geometry.xtk_nm,
            xtk_side: snapshot.navigation_geometry.xtk_side.clone(),
            is_off_route: snapshot.navigation_geometry.is_off_route,
        });

        let next_waypoint = snapshot.active_leg.as_ref().and_then(|l| {
            l.next_fix.as_ref().map(|fix| FlightdeckOsWaypoint {
                ident: fix.clone(),
                distance_nm: snapshot.navigation_geometry.distance_to_next_fix_nm,
                ete_sec: snapshot.navigation_geometry.ete_next_fix_sec,
                eta_utc: snapshot.navigation_geometry.eta_next_fix_utc,
                altitude_constraint: l.altitude_constraint.clone(),
            })
        });

        let descent_guidance = FlightdeckOsDescentGuidance {
            tod_distance_nm: snapshot.descent_profile.tod_distance_nm,
            profile_status: snapshot.descent_profile.profile_status.clone(),
            required_vs_fpm: snapshot.descent_profile.required_descent_rate_fpm,
            profile_deviation_ft: snapshot.descent_profile.profile_deviation_ft,
        };

        let destination_brief = FlightdeckOsDestinationBrief {
            icao: snapshot.destination.ident.clone(),
            runway: snapshot.destination.selected_runway.clone(),
            star: snapshot.destination.procedure_name.clone(),
            approach: snapshot.destination.procedure_name.clone(),
            metar: snapshot.weather_summary.destination_metar.clone(),
            is_source_required: snapshot.destination.is_source_required,
        };

        let active_advisories = snapshot
            .advisories
            .iter()
            .map(|a| FlightdeckOsAdvisory {
                severity: format!("{:?}", a.level),
                code: a.code.clone(),
                message: a.message.clone(),
            })
            .collect();

        FlightdeckOsContext {
            flightdeck_os_version: "1.0".to_string(),
            session_id: snapshot.session_id.clone(),
            timestamp_utc: snapshot.timestamp,
            flight_id: format!("{}-{}", snapshot.origin.ident, snapshot.destination.ident),
            flight_phase: snapshot.flight_phase.display_name().to_string(),
            connection_status: snapshot.connection_state.as_str().to_string(),
            telemetry_age_ms: snapshot.stale_flags.telemetry_age_ms,
            position,
            active_leg,
            next_waypoint,
            descent_guidance,
            destination_brief,
            active_advisories,
            warnings: snapshot.navigation_warnings.clone(),
        }
    }
}
