//! Online Network Provider Trait and Capabilities.

use crate::model::{NetworkSnapshot, OnlineEvent};
use anyhow::Result;

/// Abstract interface for online flight networks (e.g. VATSIM, IVAO).
pub trait OnlineNetworkProvider: Send + Sync {
    /// Identifier for this provider (e.g. "VATSIM").
    fn name(&self) -> &'static str;

    /// Official human-readable authority/network title.
    fn title(&self) -> &'static str;

    /// Recommended polling cadence in seconds.
    fn polling_interval_secs(&self) -> u32;

    /// Fetch latest live network snapshot.
    fn fetch_snapshot(&self) -> Result<NetworkSnapshot>;

    /// Fetch active and upcoming online events.
    fn fetch_events(&self) -> Result<Vec<OnlineEvent>>;
}
