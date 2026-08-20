//! OpenAIRAC Weather Subsystem.
//!
//! Provides structured METAR, TAF, international SIGMET polygons, PIREPs,
//! route corridor hazard intersections, and integrated preflight briefing.

pub mod briefing;
pub mod cache;
pub mod corridor;
pub mod model;
pub mod providers;

pub use briefing::{AirportWeatherBriefing, FlightBriefing};
pub use cache::{WeatherCache, WeatherCacheStatus};
pub use corridor::RouteCorridor;
pub use model::{
    CloudLayer, FlightCategory, MetarReport, PirepReport, Sigmet, SigmetHazard, TafForecastPeriod,
    TafReport, WeatherStaleness, WeatherStation,
};
pub use providers::AviationWeatherProvider;
