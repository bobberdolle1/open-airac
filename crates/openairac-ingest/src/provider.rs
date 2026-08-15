use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use openairac_store::{EntityWrite, WorldStore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// SHA-256 of raw content, hex encoded.
pub fn sha256_hex(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    format!("{:x}", hasher.finalize())
}

/// Metadata and payload of a fetched raw dataset.
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

/// Deterministic statistics report generated during data ingestion.
///
/// Classification (fail-closed policy):
/// * `records_rejected` — malformed / invalid: dropped, counted, warning.
/// * `records_quarantined` — questionable (e.g. composite or unrepresentable):
///   dropped from the store, counted, warning.
/// * `records_created` / `records_updated` / `records_unchanged` — accepted.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IngestReport {
    pub provider_name: String,
    pub dataset_name: String,
    pub records_seen: usize,
    pub records_parsed: usize,
    pub records_created: usize,
    pub records_updated: usize,
    pub records_unchanged: usize,
    pub records_quarantined: usize,
    pub records_rejected: usize,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    pub duration_ms: u64,
    pub source_checksum: String,
}

impl IngestReport {
    pub fn new(provider: &str, dataset: &str, checksum: &str) -> Self {
        Self {
            provider_name: provider.to_string(),
            dataset_name: dataset.to_string(),
            source_checksum: checksum.to_string(),
            ..Default::default()
        }
    }

    /// Number of records written to the store.
    pub fn records_accepted(&self) -> usize {
        self.records_created + self.records_updated
    }

    /// A raw record (row/line) was encountered.
    pub fn record_seen(&mut self) {
        self.records_seen += 1;
    }

    /// A raw record deserialized into the provider schema.
    pub fn record_parsed(&mut self) {
        self.records_parsed += 1;
    }

    /// The store outcome for one accepted record.
    pub fn record_write(&mut self, write: EntityWrite) {
        match write {
            EntityWrite::Created => self.records_created += 1,
            EntityWrite::Updated => self.records_updated += 1,
            EntityWrite::Unchanged => self.records_unchanged += 1,
        }
    }

    /// Questionable record: dropped from the store with a diagnostic.
    pub fn record_quarantined(&mut self, reason: String) {
        self.records_quarantined += 1;
        self.warnings.push(reason);
    }

    /// Invalid record: dropped from the store with a diagnostic.
    pub fn record_rejected(&mut self, reason: String) {
        self.records_rejected += 1;
        self.warnings.push(reason);
    }

    /// Fatal problem (usually I/O or store level, not a single record).
    pub fn record_error(&mut self, reason: String) {
        self.errors.push(reason);
    }
}

/// Abstract data provider: fetch a named dataset, then parse and ingest it.
///
/// Implementations must be fail-closed: malformed records are rejected or
/// quarantined, never reinterpreted into plausible values.
pub trait DataProvider: Send + Sync {
    fn name(&self) -> &'static str;

    /// Fetch one raw dataset over the network (or a documented local mirror).
    fn fetch(&self, dataset: &str) -> Result<FetchedDataset>;

    /// Parse the fetched content and write it into the temporal store as one
    /// transaction.
    fn parse_and_ingest(
        &self,
        dataset: &FetchedDataset,
        store: &mut WorldStore,
    ) -> Result<IngestReport>;
}

/// Decimal year (e.g. 2026.61) of a UTC instant, used for WMM secular
/// variation evaluation during ingestion.
pub fn decimal_year(dt: DateTime<Utc>) -> f64 {
    use chrono::Datelike;
    let date = dt.date_naive();
    let days_in_year = if chrono::NaiveDate::from_ymd_opt(date.year(), 12, 31)
        .map(|d| d.ordinal())
        .unwrap_or(365)
        == 366
    {
        366.0
    } else {
        365.0
    };
    date.year() as f64 + (date.ordinal() as f64 - 1.0) / days_in_year
}
/// Last-Modified, and hash the body.
pub fn fetch_url(
    provider: &str,
    dataset: &str,
    url: &str,
    retrieved_at: DateTime<Utc>,
) -> Result<FetchedDataset> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("openairac/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .context("building HTTP client")?;

    let response = client
        .get(url)
        .send()
        .with_context(|| format!("GET {url}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(anyhow::anyhow!("GET {url} returned HTTP {status}"));
    }
    let provider_revision = response
        .headers()
        .get(reqwest::header::LAST_MODIFIED)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let body = response
        .text()
        .with_context(|| format!("reading body of {url}"))?;
    let checksum = sha256_hex(body.as_bytes());
    Ok(FetchedDataset {
        provider_name: provider.to_string(),
        dataset_name: dataset.to_string(),
        source_uri: url.to_string(),
        content_sha256: checksum,
        retrieved_at,
        provider_revision,
        raw_content: body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_hex() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn test_decimal_year() {
        let mid = DateTime::parse_from_rfc3339("2026-07-02T00:00:00Z").unwrap();
        let dec = decimal_year(mid.with_timezone(&Utc));
        // 2026 is not a leap year; day 183 of 365 -> 2026.4986...
        assert!((dec - 2026.4986).abs() < 0.01);
    }
}
