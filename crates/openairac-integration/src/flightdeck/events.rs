//! Flightdeck Event Stream and State Delta / Change Detector.
//!
//! Provides bounded semantic event streaming (ring buffer) and lightweight
//! state diffing so AI crew consumers can efficiently track state transitions.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

use crate::flightdeck::snapshot::FlightdeckSnapshotV2;

/// Semantic event types emitted during flight execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FlightEventType {
    SimConnected,
    SimDisconnected,
    TelemetryStale,
    TelemetryRestored,
    FlightStarted,
    TaxiStarted,
    Takeoff,
    PhaseChanged,
    FixSequenced,
    SidCompleted,
    TopOfClimb,
    TodReached,
    DescentStarted,
    StarEntered,
    ApproachEntered,
    RunwayChanged,
    GoAround,
    Landing,
    FlightCompleted,
    OffRoute,
    RouteRejoined,
    WeatherChanged,
    AtcChanged,
    AdvisoryIssued,
}

impl FlightEventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SimConnected => "SIM_CONNECTED",
            Self::SimDisconnected => "SIM_DISCONNECTED",
            Self::TelemetryStale => "TELEMETRY_STALE",
            Self::TelemetryRestored => "TELEMETRY_RESTORED",
            Self::FlightStarted => "FLIGHT_STARTED",
            Self::TaxiStarted => "TAXI_STARTED",
            Self::Takeoff => "TAKEOFF",
            Self::PhaseChanged => "PHASE_CHANGED",
            Self::FixSequenced => "FIX_SEQUENCED",
            Self::SidCompleted => "SID_COMPLETED",
            Self::TopOfClimb => "TOP_OF_CLIMB",
            Self::TodReached => "TOD_REACHED",
            Self::DescentStarted => "DESCENT_STARTED",
            Self::StarEntered => "STAR_ENTERED",
            Self::ApproachEntered => "APPROACH_ENTERED",
            Self::RunwayChanged => "RUNWAY_CHANGED",
            Self::GoAround => "GO_AROUND",
            Self::Landing => "LANDING",
            Self::FlightCompleted => "FLIGHT_COMPLETED",
            Self::OffRoute => "OFF_ROUTE",
            Self::RouteRejoined => "ROUTE_REJOINED",
            Self::WeatherChanged => "WEATHER_CHANGED",
            Self::AtcChanged => "ATC_CHANGED",
            Self::AdvisoryIssued => "ADVISORY_ISSUED",
        }
    }
}

/// A structured monotonic flight event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightdeckEvent {
    pub id: u64,
    pub timestamp: DateTime<Utc>,
    pub event_type: String,
    pub description: String,
    pub metadata: serde_json::Value,
}

/// Bounded ring buffer for flightdeck events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightEventStream {
    capacity: usize,
    next_id: u64,
    events: VecDeque<FlightdeckEvent>,
}

impl Default for FlightEventStream {
    fn default() -> Self {
        Self::new(256)
    }
}

impl FlightEventStream {
    pub fn new(capacity: usize) -> Self {
        let cap = capacity.max(1);
        Self {
            capacity: cap,
            next_id: 1,
            events: VecDeque::with_capacity(cap),
        }
    }

    /// Record a new semantic event into the ring buffer.
    pub fn push(
        &mut self,
        event_type: FlightEventType,
        description: &str,
        metadata: serde_json::Value,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        let event = FlightdeckEvent {
            id,
            timestamp: Utc::now(),
            event_type: event_type.as_str().to_string(),
            description: description.to_string(),
            metadata,
        };

        if self.events.len() >= self.capacity {
            self.events.pop_front();
        }
        self.events.push_back(event);
        id
    }

    /// Retrieve events filtered by minimum event ID and maximum limit.
    pub fn get_events(&self, since_id: Option<u64>, limit: usize) -> Vec<FlightdeckEvent> {
        let lim = limit.clamp(1, 100);
        self.events
            .iter()
            .filter(|e| since_id.map(|s| e.id > s).unwrap_or(true))
            .take(lim)
            .cloned()
            .collect()
    }

    pub fn latest_id(&self) -> u64 {
        self.next_id.saturating_sub(1)
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// Semantic state changes between snapshots.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FlightStateDelta {
    pub timestamp: DateTime<Utc>,
    pub phase_changed: Option<String>,
    pub active_leg_changed: Option<String>,
    pub fix_sequenced: Option<String>,
    pub off_route_changed: Option<bool>,
    pub connection_changed: Option<String>,
    pub new_advisories: Vec<String>,
    pub weather_updated: bool,
    pub atc_updated: bool,
}

/// Helper to detect semantic changes between consecutive snapshots.
pub struct FlightStateDeltaDetector;

impl FlightStateDeltaDetector {
    pub fn compute_delta(
        prev: &FlightdeckSnapshotV2,
        current: &FlightdeckSnapshotV2,
    ) -> FlightStateDelta {
        let mut delta = FlightStateDelta {
            timestamp: Utc::now(),
            ..Default::default()
        };

        if prev.flight_phase != current.flight_phase {
            delta.phase_changed = Some(format!(
                "{} -> {}",
                prev.flight_phase.display_name(),
                current.flight_phase.display_name()
            ));
        }

        let prev_leg_name = prev.active_leg.as_ref().map(|l| l.leg_name.as_str());
        let cur_leg_name = current.active_leg.as_ref().map(|l| l.leg_name.as_str());
        if prev_leg_name != cur_leg_name {
            delta.active_leg_changed = cur_leg_name.map(|s| s.to_string());
        }

        let prev_fix = prev.active_leg.as_ref().and_then(|l| l.next_fix.as_deref());
        let cur_fix = current
            .active_leg
            .as_ref()
            .and_then(|l| l.next_fix.as_deref());
        if prev_fix != cur_fix && cur_fix.is_some() {
            delta.fix_sequenced = cur_fix.map(|s| s.to_string());
        }

        if prev.navigation_geometry.is_off_route != current.navigation_geometry.is_off_route {
            delta.off_route_changed = Some(current.navigation_geometry.is_off_route);
        }

        if prev.connection_state != current.connection_state {
            delta.connection_changed = Some(current.connection_state.as_str().to_string());
        }

        let prev_codes: std::collections::HashSet<_> =
            prev.advisories.iter().map(|a| a.code.as_str()).collect();
        for adv in &current.advisories {
            if !prev_codes.contains(adv.code.as_str()) {
                delta
                    .new_advisories
                    .push(format!("{}: {}", adv.code, adv.message));
            }
        }

        if prev.weather_summary.destination_metar != current.weather_summary.destination_metar {
            delta.weather_updated = true;
        }

        if prev.online_atc.len() != current.online_atc.len() {
            delta.atc_updated = true;
        }

        delta
    }
}
