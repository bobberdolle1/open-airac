//! Flightdeck Snapshot v2 and Compact AI Snapshot models.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::execution::{FlightExecutionSession, FlightPhase, FlightProgress};
use crate::flightdeck::advisory::CrewAdvisory;

pub const FLIGHTDECK_SNAPSHOT_SCHEMA_V2: &str = "flightdeck_snapshot_v2";
pub const COMPACT_AI_SNAPSHOT_SCHEMA_V1: &str = "compact_ai_snapshot_v1";

/// Simulator connection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FlightdeckConnectionState {
    Connected,
    Stale,
    Disconnected,
    Reconnecting,
}

impl FlightdeckConnectionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Connected => "CONNECTED",
            Self::Stale => "STALE",
            Self::Disconnected => "DISCONNECTED",
            Self::Reconnecting => "RECONNECTING",
        }
    }
}

/// Aircraft operational profile for AI crew awareness.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightdeckAircraftProfile {
    pub icao_type: String,
    pub model_name: Option<String>,
    pub cruise_altitude_ft: u32,
    pub cruise_speed_kts: Option<u32>,
}

/// Structured airport brief in snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightdeckAirportBrief {
    pub ident: String,
    pub iata_code: Option<String>,
    pub name: String,
    pub municipality: Option<String>,
    pub elevation_ft: Option<f64>,
    pub selected_runway: Option<String>,
    pub procedure_name: Option<String>,
    pub transition_name: Option<String>,
    pub initial_or_final_restrictions: Vec<String>,
    pub provider_name: Option<String>,
    pub is_source_required: bool,
    pub source_required_note: Option<String>,
}

/// Precise WGS84 aircraft position in snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightdeckPosition {
    pub latitude_deg: f64,
    pub longitude_deg: f64,
    pub altitude_msl_ft: f64,
    pub altitude_agl_ft: Option<f64>,
    pub groundspeed_kts: f64,
    pub vertical_speed_fpm: f64,
    pub track_true_deg: f64,
    pub on_ground: bool,
}

/// Current active navigation leg.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightdeckActiveLeg {
    pub leg_index: usize,
    pub leg_name: String,
    pub prev_fix: Option<String>,
    pub next_fix: Option<String>,
    pub leg_type: String,
    pub route_or_procedure: Option<String>,
    pub desired_track_deg: f64,
    pub distance_nm: f64,
    pub altitude_constraint: Option<String>,
    pub speed_constraint_kts: Option<u32>,
    pub provider_name: Option<String>,
}

/// Upcoming flight constraint (altitude/speed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightdeckConstraint {
    pub fix_ident: String,
    pub constraint_type: String,
    pub altitude_ft: Option<u32>,
    pub speed_kts: Option<u32>,
    pub distance_to_constraint_nm: f64,
    pub is_active: bool,
}

/// Real-time navigation geometry and distance/time estimates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightdeckNavGeometry {
    pub xtk_nm: f64,
    pub xtk_side: String,
    pub is_off_route: bool,
    pub distance_to_next_fix_nm: f64,
    pub remaining_route_distance_nm: f64,
    pub direct_destination_distance_nm: f64,
    pub ete_next_fix_sec: Option<u32>,
    pub eta_next_fix_utc: Option<DateTime<Utc>>,
    pub ete_destination_sec: Option<u32>,
    pub eta_destination_utc: Option<DateTime<Utc>>,
}

/// Descent profile and Top-of-Descent monitoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightdeckDescentProfile {
    pub tod_distance_nm: Option<f64>,
    pub tod_eta_utc: Option<DateTime<Utc>>,
    pub required_descent_rate_fpm: Option<f64>,
    pub profile_deviation_ft: Option<f64>,
    pub profile_status: String,
}

/// Runway wind components.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightdeckRunwayWind {
    pub runway_ident: String,
    pub headwind_kts: f64,
    pub crosswind_kts: f64,
    pub is_tailwind: bool,
    pub is_recommended: bool,
}

/// Real-time weather summary for flight crew.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FlightdeckWeatherSummary {
    pub origin_metar: Option<String>,
    pub origin_category: Option<String>,
    pub destination_metar: Option<String>,
    pub destination_taf: Option<String>,
    pub destination_category: Option<String>,
    pub destination_runway_wind: Option<FlightdeckRunwayWind>,
    pub weather_age_sec: Option<u64>,
    pub weather_stale: bool,
}

/// Relevant online ATC controller (VATSIM/IVAO).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightdeckOnlineAtc {
    pub network: String,
    pub callsign: String,
    pub frequency_mhz: String,
    pub facility_type: String,
    pub role_context: String,
    pub distance_nm: Option<f64>,
}

/// Provenance and data source transparency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightdeckDataProvenance {
    pub active_provider_datasets: Vec<String>,
    pub airac_cycle: Option<String>,
    pub source_required_items: Vec<String>,
    pub confidence: String,
}

/// Granular data freshness report across all independent subsystems.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightdeckFreshnessReport {
    pub telemetry: TelemetryFreshness,
    pub weather: WeatherFreshness,
    pub online_atc: OnlineAtcFreshness,
    pub navdata: NavdataFreshness,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryFreshness {
    pub source_timestamp: Option<DateTime<Utc>>,
    pub received_at: Option<DateTime<Utc>>,
    pub age_ms: u64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherFreshness {
    pub source: String,
    pub observation_time: Option<DateTime<Utc>>,
    pub fetched_at: Option<DateTime<Utc>>,
    pub age_sec: Option<u64>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineAtcFreshness {
    pub network: String,
    pub fetched_at: Option<DateTime<Utc>>,
    pub age_sec: Option<u64>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavdataFreshness {
    pub primary_provider: String,
    pub airac_cycle: String,
    pub effective_from: Option<DateTime<Utc>>,
    pub effective_to: Option<DateTime<Utc>>,
    pub status: String,
}

/// Structured freshness summary in Compact AI Snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactAiFreshness {
    pub telemetry: String,
    pub weather: String,
    pub online: String,
    pub navdata: String,
    pub telemetry_age_ms: u64,
    pub weather_age_sec: Option<u64>,
}

/// Data freshness flags for AI reasoning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightdeckStaleFlags {
    pub telemetry_stale: bool,
    pub telemetry_age_ms: u64,
    pub weather_stale: bool,
    pub navdata_stale: bool,
}
/// Comprehensive Flightdeck Snapshot v2.
///
/// Designed as the deterministic, authoritative machine interface for
/// AI flight crew and FlightdeckOS consumers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightdeckSnapshotV2 {
    pub schema_version: String,
    pub session_id: String,
    pub timestamp: DateTime<Utc>,
    pub simulator: String,
    pub connection_state: FlightdeckConnectionState,
    pub flight_phase: FlightPhase,
    pub phase_evidence: String,
    pub aircraft: FlightdeckAircraftProfile,
    pub origin: FlightdeckAirportBrief,
    pub destination: FlightdeckAirportBrief,
    pub alternate: Option<FlightdeckAirportBrief>,
    pub position: Option<FlightdeckPosition>,
    pub active_leg: Option<FlightdeckActiveLeg>,
    pub next_constraint: Option<FlightdeckConstraint>,
    pub navigation_geometry: FlightdeckNavGeometry,
    pub descent_profile: FlightdeckDescentProfile,
    pub weather_summary: FlightdeckWeatherSummary,
    pub online_atc: Vec<FlightdeckOnlineAtc>,
    pub advisories: Vec<CrewAdvisory>,
    pub data_provenance: FlightdeckDataProvenance,
    pub stale_flags: FlightdeckStaleFlags,
    pub freshness_report: FlightdeckFreshnessReport,
    pub navigation_warnings: Vec<String>,
}

impl FlightdeckSnapshotV2 {
    /// Construct a full snapshot from a live execution session and supplemental operational context.
    pub fn build(
        session: &FlightExecutionSession,
        progress: Option<&FlightProgress>,
        weather: Option<FlightdeckWeatherSummary>,
        online_atc: Vec<FlightdeckOnlineAtc>,
        advisories: Vec<CrewAdvisory>,
        simulator_name: Option<&str>,
    ) -> Self {
        let now = Utc::now();
        let plan = &session.flight_plan;

        let telem_age_ms = session
            .last_telemetry_time
            .map(|t| (now.signed_duration_since(t).num_milliseconds().max(0)) as u64)
            .unwrap_or(u64::MAX);

        let telem_stale = telem_age_ms > 5000;
        let connection_state = if !session.is_connected {
            FlightdeckConnectionState::Disconnected
        } else if telem_stale {
            FlightdeckConnectionState::Stale
        } else {
            FlightdeckConnectionState::Connected
        };

        let aircraft = FlightdeckAircraftProfile {
            icao_type: plan
                .aircraft_profile
                .icao_type
                .clone()
                .unwrap_or_else(|| "GENERIC".to_string()),
            model_name: Some(plan.aircraft_profile.name.clone()),
            cruise_altitude_ft: plan.cruise_altitude_ft,
            cruise_speed_kts: plan.aircraft_profile.cruise_speed_kts,
        };

        // Origin Brief
        let mut dep_restrictions = Vec::new();
        if let Some(sid) = &plan.sid {
            for leg in &sid.legs {
                if let Some(c) = &leg.altitude_constraint {
                    dep_restrictions.push(format!("{}: {:?}", leg.fix_ident, c));
                }
            }
        }
        let origin = FlightdeckAirportBrief {
            ident: plan.origin.ident.clone(),
            iata_code: crate::flightdeck::tools::FlightdeckToolRegistry::resolve_airport_identity(
                &plan.origin.ident,
            )
            .iata_code,
            name: plan.origin.name.clone(),
            municipality: plan.origin.municipality.clone(),
            elevation_ft: plan.origin.elevation_ft,
            selected_runway: plan.departure_runway.clone(),
            procedure_name: plan.sid.as_ref().map(|s| s.procedure.name.clone()),
            transition_name: plan.sid_transition.clone(),
            initial_or_final_restrictions: dep_restrictions,
            provider_name: plan.active_provider_datasets.first().cloned(),
            is_source_required: false,
            source_required_note: None,
        };
        // Destination Brief
        let is_dest_source_req = plan
            .diagnostics
            .iter()
            .any(|d| d.contains("SOURCE_REQUIRED"))
            || plan.destination.ident == "URAS";
        let dest_note = if is_dest_source_req {
            Some("Terminal procedures unavailable in open source dataset; official AIP source required".to_string())
        } else {
            None
        };

        let mut arr_restrictions = Vec::new();
        if let Some(star) = &plan.star {
            for leg in &star.legs {
                if let Some(c) = &leg.altitude_constraint {
                    arr_restrictions.push(format!("{}: {:?}", leg.fix_ident, c));
                }
            }
        }
        if let Some(app) = &plan.approach {
            for leg in &app.legs {
                if let Some(c) = &leg.altitude_constraint {
                    arr_restrictions.push(format!("{}: {:?}", leg.fix_ident, c));
                }
            }
        }
        let destination = FlightdeckAirportBrief {
            ident: plan.destination.ident.clone(),
            iata_code: crate::flightdeck::tools::FlightdeckToolRegistry::resolve_airport_identity(
                &plan.destination.ident,
            )
            .iata_code,
            name: plan.destination.name.clone(),
            municipality: plan.destination.municipality.clone(),
            elevation_ft: plan.destination.elevation_ft,
            selected_runway: plan.arrival_runway.clone(),
            procedure_name: plan.star.as_ref().map(|s| s.procedure.name.clone()),
            transition_name: plan.star_transition.clone(),
            initial_or_final_restrictions: arr_restrictions,
            provider_name: plan.active_provider_datasets.first().cloned(),
            is_source_required: is_dest_source_req,
            source_required_note: dest_note,
        };
        let alternate = plan.alternates.first().map(|alt| FlightdeckAirportBrief {
            ident: alt.ident.clone(),
            iata_code: crate::flightdeck::tools::FlightdeckToolRegistry::resolve_airport_identity(
                &alt.ident,
            )
            .iata_code,
            name: alt.name.clone(),
            municipality: alt.municipality.clone(),
            elevation_ft: alt.elevation_ft,
            selected_runway: None,
            procedure_name: None,
            transition_name: None,
            initial_or_final_restrictions: Vec::new(),
            provider_name: plan.active_provider_datasets.first().cloned(),
            is_source_required: false,
            source_required_note: None,
        });
        let position = session.last_telemetry.as_ref().map(|t| FlightdeckPosition {
            latitude_deg: t.latitude_deg,
            longitude_deg: t.longitude_deg,
            altitude_msl_ft: t.altitude_msl_ft,
            altitude_agl_ft: t.altitude_agl_ft,
            groundspeed_kts: t.groundspeed_kts,
            vertical_speed_fpm: t.vertical_speed_fpm,
            track_true_deg: t.track_true_deg,
            on_ground: t.on_ground,
        });

        // Active Leg & Next Constraint
        let active_leg = if session.active_leg_index < plan.all_legs.len() {
            let leg = &plan.all_legs[session.active_leg_index];
            Some(FlightdeckActiveLeg {
                leg_index: session.active_leg_index,
                leg_name: format!("{} -> {}", leg.from_fix, leg.to_fix),
                prev_fix: Some(leg.from_fix.clone()),
                next_fix: Some(leg.to_fix.clone()),
                leg_type: leg.kind.as_str().to_string(),
                route_or_procedure: leg
                    .route_ident
                    .clone()
                    .or_else(|| leg.procedure_ident.clone()),
                desired_track_deg: leg.course_true_deg.unwrap_or(0.0),
                distance_nm: leg.distance_nm,
                altitude_constraint: leg.altitude_constraint_str.clone(),
                speed_constraint_kts: leg.speed_constraint_kts,
                provider_name: leg.provider_name.clone(),
            })
        } else {
            None
        };

        // Find next constraint
        let mut next_constraint = None;
        for i in session.active_leg_index..plan.all_legs.len() {
            let leg = &plan.all_legs[i];
            if leg.altitude_constraint_str.is_some() || leg.speed_constraint_kts.is_some() {
                let dist: f64 = plan.all_legs[session.active_leg_index..=i]
                    .iter()
                    .map(|l| l.distance_nm)
                    .sum();
                next_constraint = Some(FlightdeckConstraint {
                    fix_ident: leg.to_fix.clone(),
                    constraint_type: leg
                        .altitude_constraint_str
                        .clone()
                        .unwrap_or_else(|| "SPEED_ONLY".to_string()),
                    altitude_ft: None,
                    speed_kts: leg.speed_constraint_kts,
                    distance_to_constraint_nm: dist,
                    is_active: i == session.active_leg_index,
                });
                break;
            }
        }

        // Nav geometry
        let xtk_nm = progress.map(|p| p.xtk_nm).unwrap_or(0.0);
        let xtk_side = if xtk_nm.abs() < 0.05 {
            "ON_TRACK".to_string()
        } else if xtk_nm < 0.0 {
            "LEFT".to_string()
        } else {
            "RIGHT".to_string()
        };

        let navigation_geometry = FlightdeckNavGeometry {
            xtk_nm: xtk_nm.abs(),
            xtk_side,
            is_off_route: progress.map(|p| p.is_off_route).unwrap_or(false),
            distance_to_next_fix_nm: progress.map(|p| p.distance_to_next_fix_nm).unwrap_or(0.0),
            remaining_route_distance_nm: progress
                .map(|p| p.remaining_route_distance_nm)
                .unwrap_or(plan.total_distance_nm),
            direct_destination_distance_nm: progress
                .map(|p| p.direct_distance_to_destination_nm)
                .unwrap_or(plan.total_distance_nm),
            ete_next_fix_sec: progress.and_then(|p| p.ete_next_fix_sec),
            eta_next_fix_utc: progress.and_then(|p| p.eta_next_fix),
            ete_destination_sec: progress.and_then(|p| p.ete_destination_sec),
            eta_destination_utc: progress.and_then(|p| p.eta_destination),
        };

        // Descent profile
        let profile_dev = progress.and_then(|p| p.descent_profile_deviation_ft);
        let profile_status = match profile_dev {
            Some(dev) if dev > 200.0 => "ABOVE_PROFILE".to_string(),
            Some(dev) if dev < -200.0 => "BELOW_PROFILE".to_string(),
            Some(_) => "ON_PROFILE".to_string(),
            None => {
                if session.phase_engine.current_phase() == FlightPhase::Cruise {
                    "CRUISE_LEVEL".to_string()
                } else {
                    "UNAVAILABLE".to_string()
                }
            }
        };

        let descent_profile = FlightdeckDescentProfile {
            tod_distance_nm: progress.and_then(|p| p.tod_distance_nm),
            tod_eta_utc: None,
            required_descent_rate_fpm: progress.and_then(|p| p.required_descent_rate_fpm),
            profile_deviation_ft: profile_dev,
            profile_status,
        };

        let weather_summary = weather.unwrap_or_default();

        let mut source_req_items = Vec::new();
        if is_dest_source_req {
            source_req_items.push(format!("{}: TERMINAL PROCEDURES", plan.destination.ident));
        }

        let data_provenance = FlightdeckDataProvenance {
            active_provider_datasets: plan.active_provider_datasets.clone(),
            airac_cycle: Some("2608".to_string()),
            source_required_items: source_req_items,
            confidence: "AUTHORITATIVE_FEDERATED".to_string(),
        };
        let stale_flags = FlightdeckStaleFlags {
            telemetry_stale: telem_stale,
            telemetry_age_ms: telem_age_ms,
            weather_stale: weather_summary.weather_stale,
            navdata_stale: false,
        };

        let telem_freshness = TelemetryFreshness {
            source_timestamp: session.last_telemetry_time,
            received_at: session.last_telemetry_time,
            age_ms: telem_age_ms,
            status: match connection_state {
                FlightdeckConnectionState::Connected => "CURRENT".to_string(),
                FlightdeckConnectionState::Stale => "STALE".to_string(),
                _ => "DISCONNECTED".to_string(),
            },
        };

        let weather_freshness = WeatherFreshness {
            source: if weather_summary.destination_metar.is_some() {
                "NOAA/AviationWeather.gov".to_string()
            } else {
                "UNAVAILABLE".to_string()
            },
            observation_time: None,
            fetched_at: Some(now),
            age_sec: weather_summary.weather_age_sec,
            status: if weather_summary.destination_metar.is_none() {
                "UNAVAILABLE".to_string()
            } else if weather_summary.weather_stale {
                "STALE".to_string()
            } else {
                "CURRENT".to_string()
            },
        };

        let online_freshness = OnlineAtcFreshness {
            network: if online_atc.is_empty() {
                "NONE".to_string()
            } else {
                online_atc[0].network.clone()
            },
            fetched_at: Some(now),
            age_sec: Some(60),
            status: if online_atc.is_empty() {
                "UNAVAILABLE".to_string()
            } else {
                "CURRENT".to_string()
            },
        };

        let navdata_freshness = NavdataFreshness {
            primary_provider: plan
                .active_provider_datasets
                .first()
                .cloned()
                .unwrap_or_else(|| "GLOBAL_OPEN".to_string()),
            airac_cycle: "2608".to_string(),
            effective_from: None,
            effective_to: None,
            status: if is_dest_source_req {
                "SOURCE_REQUIRED".to_string()
            } else {
                "CURRENT".to_string()
            },
        };

        let freshness_report = FlightdeckFreshnessReport {
            telemetry: telem_freshness,
            weather: weather_freshness,
            online_atc: online_freshness,
            navdata: navdata_freshness,
        };

        Self {
            schema_version: FLIGHTDECK_SNAPSHOT_SCHEMA_V2.to_string(),
            session_id: session.session_id.clone(),
            timestamp: now,
            simulator: simulator_name
                .unwrap_or("X-Plane 11/12 Protocol")
                .to_string(),
            connection_state,
            flight_phase: session.phase_engine.current_phase(),
            phase_evidence: session.phase_engine.evidence().to_string(),
            aircraft,
            origin,
            destination,
            alternate,
            position,
            active_leg,
            next_constraint,
            navigation_geometry,
            descent_profile,
            weather_summary,
            online_atc,
            advisories,
            data_provenance,
            stale_flags,
            freshness_report,
            navigation_warnings: plan.diagnostics.clone(),
        }
    }

    /// Convert full snapshot into context-budget Compact AI Snapshot.
    pub fn to_compact(&self) -> CompactAiSnapshot {
        let flight = format!("{} -> {}", self.origin.ident, self.destination.ident);
        let phase = self.flight_phase.display_name().to_string();
        let aircraft = self.aircraft.icao_type.clone();

        let position = if let Some(pos) = &self.position {
            format!(
                "LAT {:.4}° LON {:.4}° | {:.0} ft MSL | GS {:.0} kt | TRK {:.0}°",
                pos.latitude_deg,
                pos.longitude_deg,
                pos.altitude_msl_ft,
                pos.groundspeed_kts,
                pos.track_true_deg
            )
        } else {
            "NO POSITION DATA".to_string()
        };

        let active_leg = self
            .active_leg
            .as_ref()
            .map(|l| {
                format!(
                    "{} ({} | Desired TRK: {:.0}°)",
                    l.leg_name, l.leg_type, l.desired_track_deg
                )
            })
            .unwrap_or_else(|| "NONE".to_string());

        let next_fix = if let Some(leg) = &self.active_leg {
            let fix = leg.next_fix.as_deref().unwrap_or("DEST");
            let ete_str = self
                .navigation_geometry
                .ete_next_fix_sec
                .map(|s| format!("{}m {}s", s / 60, s % 60))
                .unwrap_or_else(|| "--:--".to_string());
            format!(
                "{fix} ({:.1} NM, ETE: {ete_str})",
                self.navigation_geometry.distance_to_next_fix_nm
            )
        } else {
            "NONE".to_string()
        };

        let next_constraint = self
            .next_constraint
            .as_ref()
            .map(|c| format!("{}: {}", c.fix_ident, c.constraint_type))
            .unwrap_or_else(|| "NONE".to_string());

        let xtk = format!(
            "{:.2} NM {} ({})",
            self.navigation_geometry.xtk_nm,
            self.navigation_geometry.xtk_side,
            if self.navigation_geometry.is_off_route {
                "OFF ROUTE"
            } else {
                "ON ROUTE"
            }
        );

        let rem_ete = self
            .navigation_geometry
            .ete_destination_sec
            .map(|s| format!("{}m {}s", s / 60, s % 60))
            .unwrap_or_else(|| "--:--".to_string());
        let route_remaining = format!(
            "{:.1} NM (ETE: {rem_ete})",
            self.navigation_geometry.remaining_route_distance_nm
        );

        let tod = if let Some(tod_nm) = self.descent_profile.tod_distance_nm {
            format!("{:.1} NM", tod_nm)
        } else if self.flight_phase == FlightPhase::Descent
            || self.flight_phase.is_terminal_arrival()
        {
            "PASSED".to_string()
        } else {
            "N/A".to_string()
        };

        let descent_profile = format!(
            "{} (Req VS: {} fpm | Dev: {} ft)",
            self.descent_profile.profile_status,
            self.descent_profile
                .required_descent_rate_fpm
                .map(|v| format!("{:.0}", v))
                .unwrap_or_else(|| "--".to_string()),
            self.descent_profile
                .profile_deviation_ft
                .map(|v| format!("{:.0}", v))
                .unwrap_or_else(|| "--".to_string())
        );

        let arrival = if self.destination.is_source_required {
            format!(
                "{} (NO STAR / NO APPROACH - SOURCE REQUIRED)",
                self.destination.ident
            )
        } else {
            format!(
                "{} / {} / RWY {}",
                self.destination
                    .procedure_name
                    .as_deref()
                    .unwrap_or("DIRECT"),
                self.destination
                    .procedure_name
                    .as_deref()
                    .unwrap_or("VISUAL"),
                self.destination.selected_runway.as_deref().unwrap_or("--")
            )
        };

        let destination_weather = self
            .weather_summary
            .destination_metar
            .clone()
            .unwrap_or_else(|| "NO METAR AVAILABLE".to_string());

        let online_atc = self
            .online_atc
            .iter()
            .map(|a| format!("{} [{}] ({})", a.callsign, a.facility_type, a.frequency_mhz))
            .collect();

        let advisories = self
            .advisories
            .iter()
            .map(|a| format!("[{:?}] {}: {}", a.level, a.code, a.message))
            .collect();

        let provenance = format!(
            "{} | AIRAC {}",
            self.data_provenance.active_provider_datasets.join(", "),
            self.data_provenance
                .airac_cycle
                .as_deref()
                .unwrap_or("CURRENT")
        );

        let freshness = CompactAiFreshness {
            telemetry: self.freshness_report.telemetry.status.clone(),
            weather: self.freshness_report.weather.status.clone(),
            online: self.freshness_report.online_atc.status.clone(),
            navdata: self.freshness_report.navdata.status.clone(),
            telemetry_age_ms: self.stale_flags.telemetry_age_ms,
            weather_age_sec: self.weather_summary.weather_age_sec,
        };
        CompactAiSnapshot {
            schema_version: COMPACT_AI_SNAPSHOT_SCHEMA_V1.to_string(),
            flight,
            phase,
            aircraft,
            position,
            active_leg,
            next_fix,
            next_constraint,
            xtk,
            route_remaining,
            tod,
            descent_profile,
            arrival,
            destination_weather,
            online_atc,
            advisories,
            provenance,
            freshness,
            warnings: self.navigation_warnings.clone(),
        }
    }
}

/// Low-token context-budget Compact AI Snapshot for AI flight crew narration and reasoning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactAiSnapshot {
    pub schema_version: String,
    pub flight: String,
    pub phase: String,
    pub aircraft: String,
    pub position: String,
    pub active_leg: String,
    pub next_fix: String,
    pub next_constraint: String,
    pub xtk: String,
    pub route_remaining: String,
    pub tod: String,
    pub descent_profile: String,
    pub arrival: String,
    pub destination_weather: String,
    pub online_atc: Vec<String>,
    pub advisories: Vec<String>,
    pub provenance: String,
    pub freshness: CompactAiFreshness,
    pub warnings: Vec<String>,
}
