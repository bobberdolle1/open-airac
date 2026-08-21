//! Deterministic AI Crew Tool Registry and Query Interfaces.
//!
//! Provides typed, deterministic query endpoints and tools for AI flight crew
//! reasoning without internal LLM dependencies.

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::flightdeck::briefing::{FlightdeckArrivalBriefing, FlightdeckDepartureBriefing};
use crate::flightdeck::snapshot::{CompactAiSnapshot, FlightdeckSnapshotV2};

/// Typed machine errors returned by AI Crew query tools.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FlightdeckError {
    NoActiveFlight,
    SimNotConnected,
    TelemetryStale,
    WeatherUnavailable(String),
    ProviderSourceRequired(String),
    FixNotFound(String),
    ConstraintNotFound,
    InvalidParameter(String),
}

impl fmt::Display for FlightdeckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoActiveFlight => write!(f, "No active flight execution session"),
            Self::SimNotConnected => write!(f, "Simulator is not connected"),
            Self::TelemetryStale => write!(f, "Simulator telemetry is stale"),
            Self::WeatherUnavailable(icao) => write!(f, "Weather data is unavailable for {icao}"),
            Self::ProviderSourceRequired(s) => write!(
                f,
                "Airport or procedure requires official provider source dataset ({s})"
            ),
            Self::FixNotFound(fix) => write!(
                f,
                "Fix {fix} not found in current flight plan or active navigation database"
            ),
            Self::ConstraintNotFound => write!(f, "No altitude or speed constraint found"),
            Self::InvalidParameter(p) => write!(f, "Invalid query parameter: {p}"),
        }
    }
}

impl std::error::Error for FlightdeckError {}

/// Resolved airport identity information across providers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedAirportIdentity {
    pub query_ident: String,
    pub authoritative_ident: String,
    pub iata_code: Option<String>,
    pub airport_name: String,
    pub country_code: String,
    pub primary_provider: String,
    pub alternate_identities: Vec<ProviderScopedIdentity>,
    pub terminal_procedures_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderScopedIdentity {
    pub provider: String,
    pub ident: String,
    pub name: String,
    pub note: Option<String>,
}

/// Deterministic Tool Registry for AI Crew query execution.
pub struct FlightdeckToolRegistry;

impl FlightdeckToolRegistry {
    /// Get the complete Flightdeck Snapshot v2.
    pub fn get_snapshot(
        snapshot: &FlightdeckSnapshotV2,
    ) -> Result<FlightdeckSnapshotV2, FlightdeckError> {
        Ok(snapshot.clone())
    }

    /// Get the context-budget Compact AI Snapshot.
    pub fn get_compact_snapshot(
        snapshot: &FlightdeckSnapshotV2,
    ) -> Result<CompactAiSnapshot, FlightdeckError> {
        Ok(snapshot.to_compact())
    }

    /// Query current active flight state and position.
    pub fn get_active_flight(
        snapshot: &FlightdeckSnapshotV2,
    ) -> Result<serde_json::Value, FlightdeckError> {
        Ok(serde_json::json!({
            "flight": format!("{} -> {}", snapshot.origin.ident, snapshot.destination.ident),
            "phase": snapshot.flight_phase.display_name(),
            "aircraft": snapshot.aircraft.icao_type,
            "position": snapshot.position,
            "connection_state": snapshot.connection_state,
        }))
    }

    /// Query current active navigation leg.
    pub fn get_active_leg(
        snapshot: &FlightdeckSnapshotV2,
    ) -> Result<serde_json::Value, FlightdeckError> {
        if let Some(leg) = &snapshot.active_leg {
            Ok(serde_json::json!({
                "leg_name": leg.leg_name,
                "prev_fix": leg.prev_fix,
                "next_fix": leg.next_fix,
                "leg_type": leg.leg_type,
                "desired_track_deg": leg.desired_track_deg,
                "distance_to_next_nm": snapshot.navigation_geometry.distance_to_next_fix_nm,
                "xtk_nm": snapshot.navigation_geometry.xtk_nm,
                "xtk_side": snapshot.navigation_geometry.xtk_side,
                "is_off_route": snapshot.navigation_geometry.is_off_route,
            }))
        } else {
            Err(FlightdeckError::NoActiveFlight)
        }
    }

    /// Query upcoming altitude/speed constraint.
    pub fn get_next_constraint(
        snapshot: &FlightdeckSnapshotV2,
    ) -> Result<serde_json::Value, FlightdeckError> {
        if let Some(constraint) = &snapshot.next_constraint {
            Ok(serde_json::json!({
                "fix_ident": constraint.fix_ident,
                "constraint": constraint.constraint_type,
                "distance_nm": constraint.distance_to_constraint_nm,
                "is_active": constraint.is_active,
            }))
        } else {
            Err(FlightdeckError::ConstraintNotFound)
        }
    }

    /// Query structured departure briefing.
    pub fn get_departure_brief(
        snapshot: &FlightdeckSnapshotV2,
    ) -> Result<FlightdeckDepartureBriefing, FlightdeckError> {
        let text = format!(
            "Departure from {} (Elev {:.0} ft), Runway {}, SID {} (Trans: {}).",
            snapshot.origin.ident,
            snapshot.origin.elevation_ft.unwrap_or(0.0),
            snapshot
                .origin
                .selected_runway
                .as_deref()
                .unwrap_or("DEFAULT"),
            snapshot.origin.procedure_name.as_deref().unwrap_or("NONE"),
            snapshot.origin.transition_name.as_deref().unwrap_or("NONE")
        );

        Ok(FlightdeckDepartureBriefing {
            origin_icao: snapshot.origin.ident.clone(),
            origin_name: snapshot.origin.name.clone(),
            elevation_ft: snapshot.origin.elevation_ft.unwrap_or(0.0),
            departure_runway: snapshot.origin.selected_runway.clone(),
            sid_procedure: snapshot.origin.procedure_name.clone(),
            sid_transition: snapshot.origin.transition_name.clone(),
            initial_altitude_constraints: snapshot.origin.initial_or_final_restrictions.clone(),
            weather_metar: snapshot.weather_summary.origin_metar.clone(),
            runway_wind: None,
            provider_name: snapshot.origin.provider_name.clone(),
            warnings: snapshot.navigation_warnings.clone(),
            briefing_text: text,
        })
    }

    /// Query structured arrival briefing with strict SOURCE_REQUIRED verification.
    pub fn get_arrival_brief(
        snapshot: &FlightdeckSnapshotV2,
    ) -> Result<FlightdeckArrivalBriefing, FlightdeckError> {
        let is_source_req = snapshot.destination.is_source_required;
        let text = if is_source_req {
            format!(
                "Arrival at {} (Elev {:.0} ft): SOURCE_REQUIRED. Terminal procedures (STAR/APP) are not available in the open source dataset.",
                snapshot.destination.ident,
                snapshot.destination.elevation_ft.unwrap_or(0.0)
            )
        } else {
            format!(
                "Arrival at {} (Elev {:.0} ft), Runway {}, STAR {} (Trans: {}), Approach {}.",
                snapshot.destination.ident,
                snapshot.destination.elevation_ft.unwrap_or(0.0),
                snapshot
                    .destination
                    .selected_runway
                    .as_deref()
                    .unwrap_or("DEFAULT"),
                snapshot
                    .destination
                    .procedure_name
                    .as_deref()
                    .unwrap_or("DIRECT"),
                snapshot
                    .destination
                    .transition_name
                    .as_deref()
                    .unwrap_or("DEFAULT"),
                snapshot
                    .destination
                    .procedure_name
                    .as_deref()
                    .unwrap_or("VISUAL")
            )
        };

        Ok(FlightdeckArrivalBriefing {
            destination_icao: snapshot.destination.ident.clone(),
            destination_name: snapshot.destination.name.clone(),
            elevation_ft: snapshot.destination.elevation_ft.unwrap_or(0.0),
            arrival_runway: snapshot.destination.selected_runway.clone(),
            star_procedure: if is_source_req {
                None
            } else {
                snapshot.destination.procedure_name.clone()
            },
            star_transition: if is_source_req {
                None
            } else {
                snapshot.destination.transition_name.clone()
            },
            approach_procedure: if is_source_req {
                None
            } else {
                snapshot.destination.procedure_name.clone()
            },
            approach_type: if is_source_req {
                None
            } else {
                Some("ILS/RNP".to_string())
            },
            final_approach_restrictions: snapshot.destination.initial_or_final_restrictions.clone(),
            weather_metar: snapshot.weather_summary.destination_metar.clone(),
            weather_taf: snapshot.weather_summary.destination_taf.clone(),
            runway_wind: snapshot.weather_summary.destination_runway_wind.clone(),
            is_source_required: is_source_req,
            source_required_note: snapshot.destination.source_required_note.clone(),
            provider_name: snapshot.destination.provider_name.clone(),
            warnings: snapshot.navigation_warnings.clone(),
            briefing_text: text,
        })
    }

    /// Resolve airport multi-identity without collapsing provider semantics.
    pub fn resolve_airport_identity(ident: &str) -> ResolvedAirportIdentity {
        let upper = ident.trim().to_uppercase();
        if upper == "URAS" || upper == "UGSS" || upper == "SUI" {
            ResolvedAirportIdentity {
                query_ident: ident.to_string(),
                authoritative_ident: "URAS".to_string(),
                iata_code: Some("SUI".to_string()),
                airport_name: "Sukhumi Babushara".to_string(),
                country_code: "GE/AB".to_string(),
                primary_provider: "CAICA".to_string(),
                alternate_identities: vec![
                    ProviderScopedIdentity {
                        provider: "CAICA".to_string(),
                        ident: "URAS".to_string(),
                        name: "Сухум (Бабушара)".to_string(),
                        note: Some("Authoritative Russian AIS AIP designation".to_string()),
                    },
                    ProviderScopedIdentity {
                        provider: "OurAirports/ICAO".to_string(),
                        ident: "UGSS".to_string(),
                        name: "Sukhumi Babushara Airport".to_string(),
                        note: Some("International / Georgian ICAO designation".to_string()),
                    },
                ],
                terminal_procedures_status: "SOURCE_REQUIRED".to_string(),
            }
        } else if upper == "URFF" || upper == "UKFF" || upper == "SIP" {
            ResolvedAirportIdentity {
                query_ident: ident.to_string(),
                authoritative_ident: "URFF".to_string(),
                iata_code: Some("SIP".to_string()),
                airport_name: "Simferopol".to_string(),
                country_code: "RU/UA".to_string(),
                primary_provider: "CAICA".to_string(),
                alternate_identities: vec![
                    ProviderScopedIdentity {
                        provider: "CAICA".to_string(),
                        ident: "URFF".to_string(),
                        name: "Симферополь".to_string(),
                        note: Some("Authoritative CAICA domestic AIP designation".to_string()),
                    },
                    ProviderScopedIdentity {
                        provider: "OurAirports/ICAO".to_string(),
                        ident: "UKFF".to_string(),
                        name: "Simferopol International".to_string(),
                        note: Some("Ukrainian / ICAO international designation".to_string()),
                    },
                ],
                terminal_procedures_status: "AUTHORITATIVE_AVAILABLE".to_string(),
            }
        } else {
            ResolvedAirportIdentity {
                query_ident: ident.to_string(),
                authoritative_ident: upper.clone(),
                iata_code: None,
                airport_name: format!("Airport {}", upper),
                country_code: "GLOBAL".to_string(),
                primary_provider: "GLOBAL_OPEN".to_string(),
                alternate_identities: Vec::new(),
                terminal_procedures_status: "AVAILABLE".to_string(),
            }
        }
    }
}
