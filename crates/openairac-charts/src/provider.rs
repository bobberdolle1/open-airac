//! Chart Provider Abstraction and Registry.
//!
//! Decouples chart metadata synchronization and asset retrieval from
//! authority-specific protocols and storage formats.

use crate::cache::ChartCache;
use crate::catalog::ChartCatalog;
use crate::model::ChartDocument;
use anyhow::Result;
use openairac_model::RedistributionPermission;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncReport {
    pub provider_id: String,
    pub airac_cycle: String,
    pub airports_indexed: usize,
    pub charts_indexed: usize,
}

pub trait ChartProvider: Send + Sync {
    fn provider_id(&self) -> &'static str;
    fn provider_name(&self) -> &'static str;
    fn authority(&self) -> &'static str;
    fn jurisdiction(&self) -> &'static str;
    fn license_policy(&self) -> RedistributionPermission;

    /// Synchronize chart metadata for a given cycle into the catalog database.
    fn sync_catalog(&self, catalog: &ChartCatalog, cycle: Option<&str>) -> Result<SyncReport>;

    /// Fetch and verify the chart asset, storing it into the content-addressed cache.
    fn fetch_asset(&self, chart: &ChartDocument, cache: &ChartCache) -> Result<PathBuf>;
}
