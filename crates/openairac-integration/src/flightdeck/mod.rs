//! OpenAIRAC FlightdeckOS & AI Crew Integration Engine.
//!
//! Provides deterministic machine-readable aviation state, Flightdeck Snapshot v2,
//! Compact AI Snapshots, deterministic Crew Advisories, structured Event Streams,
//! Delta/Change Detection, Airport Multi-Identity Resolution, and AI Crew Query Tools.

pub mod adapter;
pub mod advisory;
pub mod briefing;
pub mod events;
pub mod snapshot;
pub mod tools;

pub use adapter::{FlightdeckOsAdapter, FlightdeckOsContext};
pub use advisory::{AdvisoryLevel, CrewAdvisory, CrewAdvisoryEngine};
pub use briefing::{FlightdeckArrivalBriefing, FlightdeckDepartureBriefing, InFlightBriefSummary};
pub use events::{
    FlightEventStream, FlightEventType, FlightStateDelta, FlightStateDeltaDetector, FlightdeckEvent,
};
pub use snapshot::{
    COMPACT_AI_SNAPSHOT_SCHEMA_V1, CompactAiFreshness, CompactAiSnapshot,
    FLIGHTDECK_SNAPSHOT_SCHEMA_V2, FlightdeckActiveLeg, FlightdeckAircraftProfile,
    FlightdeckAirportBrief, FlightdeckConnectionState, FlightdeckConstraint,
    FlightdeckDataProvenance, FlightdeckDescentProfile, FlightdeckFreshnessReport,
    FlightdeckNavGeometry, FlightdeckOnlineAtc, FlightdeckPosition, FlightdeckRunwayWind,
    FlightdeckSnapshotV2, FlightdeckStaleFlags, FlightdeckWeatherSummary, NavdataFreshness,
    OnlineAtcFreshness, TelemetryFreshness, WeatherFreshness,
};
pub use tools::{
    FlightdeckError, FlightdeckToolRegistry, ProviderScopedIdentity, ResolvedAirportIdentity,
};
