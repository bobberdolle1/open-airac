//! OpenAIRAC Online Network Subsystem.
//!
//! Provides structured models, ingest providers, and route/airport awareness engines
//! for real-time online flight simulation networks (e.g. VATSIM).

pub mod cache;
pub mod model;
pub mod provider;
pub mod providers;
pub mod route;
pub mod sanitize;

pub use cache::OnlineCache;
pub use model::{
    FacilityType, NetworkFreshness, NetworkSnapshot, OnlineAtis, OnlineController, OnlineEvent,
    OnlinePilot, OnlinePrefile, OnlineServer,
};
pub use provider::OnlineNetworkProvider;
pub use providers::{VatsimProvider, parse_vatsim_events_json};
pub use route::{
    AirportOnlineSummary, AtcConfidence, RouteAtcPhase, RouteController, RouteOnlineAwareness,
    distance_nm, summarize_airport_online,
};
pub use sanitize::{escape_html, sanitize_callsign, sanitize_icao, sanitize_text};
