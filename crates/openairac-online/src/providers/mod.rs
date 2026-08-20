//! Online Network Providers (VATSIM and future extensions).

pub mod vatsim;
pub mod vatsim_events;

pub use vatsim::VatsimProvider;
pub use vatsim_events::parse_vatsim_events_json;
