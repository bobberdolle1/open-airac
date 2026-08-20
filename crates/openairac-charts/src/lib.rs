//! OpenAIRAC Charts Subsystem.
//!
//! Provides canonical chart metadata models, procedure-to-chart association,
//! content-addressed cache storage, and government chart providers (FAA d-TPP, France SIA eAIP).

pub mod association;
pub mod cache;
pub mod catalog;
pub mod model;
pub mod provider;
pub mod providers;

pub use association::AssociationEngine;
pub use cache::{CacheStatus, ChartCache, DEFAULT_MAX_CHART_SIZE_BYTES};
pub use catalog::ChartCatalog;
pub use model::{
    AssociationConfidence, ChartAssociation, ChartDocument, ChartDocumentId, ChartMimeType,
    GeoreferenceStatus, NormalizedChartType,
};
pub use provider::{ChartProvider, SyncReport};
pub use providers::{FaaDtppProvider, FranceSiaChartProvider};
