use anyhow::Result;
use chrono::{DateTime, Utc};
use openairac_store::WorldStore;
use serde::{Deserialize, Serialize};

/// Metadata and payload of a fetched raw dataset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchedDataset {
    pub provider_name: String,
    pub dataset_name: String,
    pub source_uri: String,
    pub content_sha256: String,
    pub retrieved_at: DateTime<Utc>,
    pub provider_revision: Option<String>,
    pub raw_content: String,
}

/// Statistics report generated during data ingestion
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IngestReport {
    pub provider_name: String,
    pub dataset_name: String,
    pub records_seen: usize,
    pub records_accepted: usize,
    pub records_rejected: usize,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

impl IngestReport {
    pub fn new(provider: &str, dataset: &str) -> Self {
        Self {
            provider_name: provider.to_string(),
            dataset_name: dataset.to_string(),
            ..Default::default()
        }
    }

    pub fn record_accepted(&mut self) {
        self.records_seen += 1;
        self.records_accepted += 1;
    }

    pub fn record_rejected(&mut self, reason: String) {
        self.records_seen += 1;
        self.records_rejected += 1;
        self.warnings.push(reason);
    }
}

/// Abstract Data Provider Trait
pub trait DataProvider {
    fn name(&self) -> &'static str;
    fn fetch(&self) -> Result<FetchedDataset>;
    fn parse_and_ingest(
        &self,
        dataset: &FetchedDataset,
        store: &WorldStore,
    ) -> Result<IngestReport>;
}
