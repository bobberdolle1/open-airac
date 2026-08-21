use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
pub use openairac_model::{Coverage, RevisionKind};
use openairac_store::{EntityWrite, WorldStore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// SHA-256 of raw content, hex encoded.
pub fn sha256_hex(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    format!("{:x}", hasher.finalize())
}
use crate::validation::ProviderValidationReport;
use openairac_model::{
    ProviderCoverageMetrics, ProviderDescriptor, ProviderId, ProviderProvenance,
};

/// A snapshot of raw source material acquired from an official authority, open URL, or Local AIP Vault.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawSourceSnapshot {
    pub provider_id: ProviderId,
    pub source_uri: String,
    pub content_sha256: String,
    pub retrieved_at: DateTime<Utc>,
    pub airac_cycle: Option<String>,
    pub effective_from: Option<DateTime<Utc>>,
    pub effective_until: Option<DateTime<Utc>>,
    pub raw_bytes: Vec<u8>,
    pub files: BTreeMap<String, Vec<u8>>,
}

impl RawSourceSnapshot {
    pub fn new(provider_id: ProviderId, source_uri: impl Into<String>, raw_bytes: Vec<u8>) -> Self {
        let checksum = sha256_hex(&raw_bytes);
        Self {
            provider_id,
            source_uri: source_uri.into(),
            content_sha256: checksum,
            retrieved_at: Utc::now(),
            airac_cycle: None,
            effective_from: None,
            effective_until: None,
            raw_bytes,
            files: BTreeMap::new(),
        }
    }

    pub fn with_file(mut self, relative_path: impl Into<String>, bytes: Vec<u8>) -> Self {
        self.files.insert(relative_path.into(), bytes);
        self
    }

    pub fn with_cycle(
        mut self,
        cycle: impl Into<String>,
        effective_from: Option<DateTime<Utc>>,
        effective_until: Option<DateTime<Utc>>,
    ) -> Self {
        self.airac_cycle = Some(cycle.into());
        self.effective_from = effective_from;
        self.effective_until = effective_until;
        self
    }
}

/// Generic container for canonical aeronautical entities produced by a provider parser.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CanonicalProviderDataset {
    pub provider_id: ProviderId,
    pub version_tag: String,
    pub airac_cycle: Option<String>,
    pub effective_from: Option<DateTime<Utc>>,
    pub effective_until: Option<DateTime<Utc>>,
    pub metrics: ProviderCoverageMetrics,
    pub provenance_records: Vec<ProviderProvenance>,
    pub raw_entities_json: BTreeMap<String, String>,
}

impl CanonicalProviderDataset {
    pub fn new(provider_id: ProviderId, version_tag: impl Into<String>) -> Self {
        Self {
            provider_id,
            version_tag: version_tag.into(),
            airac_cycle: None,
            effective_from: None,
            effective_until: None,
            metrics: ProviderCoverageMetrics::default(),
            provenance_records: Vec::new(),
            raw_entities_json: BTreeMap::new(),
        }
    }
}

/// Generic Provider Adapter SDK interface.
pub trait ProviderAdapter: Send + Sync {
    /// Return the immutable provider descriptor.
    fn descriptor(&self) -> &ProviderDescriptor;

    /// Discover available source publications / URLs / cycles.
    fn discover(&self) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    /// Acquire raw source material (from URL, local file, or Local AIP Vault).
    fn acquire(&self, source_hint: Option<&str>) -> Result<RawSourceSnapshot>;

    /// Parse the raw source snapshot into canonical aviation entities.
    fn parse(&self, snapshot: &RawSourceSnapshot) -> Result<CanonicalProviderDataset>;

    /// Run generic and provider-specific semantic/geometric validation.
    fn validate(&self, dataset: &CanonicalProviderDataset) -> Result<ProviderValidationReport> {
        let report = ProviderValidationReport::new(
            self.descriptor().name.clone(),
            dataset.version_tag.clone(),
        );
        Ok(report)
    }
}

/// Explicit, unambiguous fetch target for cycle-aware providers.
///
/// Cycle identity, download location, and the confirmed effective date are
/// three separate facts and MUST NOT be confused: `cycle_ident` is the
/// AIRAC ident, `source_uri` is where the file comes from, and
/// `effective_from` is the confirmed validity start.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CycleSelector {
    /// AIRAC cycle ident (e.g. `2608`).
    pub cycle_ident: String,
    /// Download location: file stem (`CIFP_260806`) or full URL.
    pub source_uri: String,
    /// Confirmed effective date. `None` = unconfirmed: cycle-aware
    /// providers MUST reject the fetch (fail-closed; preloading an
    /// unconfirmed cycle would make its data effective at an unknown
    /// instant).
    pub effective_from: Option<DateTime<Utc>>,
}

/// Metadata and payload of a fetched raw dataset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchedDataset {
    pub provider_name: String,
    pub dataset_name: String,
    pub source_uri: String,
    pub content_sha256: String,
    pub retrieved_at: DateTime<Utc>,
    /// Raw fetched bytes, encoded as a byte-preserving latin-1 string
    /// (byte i <-> char i). Fixed-width decoders (CIFP) rely on
    /// 1-byte-per-char column math. Hash/parse must use this, not a
    /// UTF-8 re-decode.
    pub raw_bytes: Vec<u8>,
    pub provider_revision: Option<String>,
    /// AIRAC cycle this publication belongs to (None for cycle-less
    /// providers like OurAirports).
    pub airac_cycle: Option<String>,
    /// Baseline (first publication of the cycle) or Correction.
    pub revision_kind: RevisionKind,
    /// Whether the publication covers the whole dataset (full-snapshot
    /// removal semantics apply) or only changed records.
    pub coverage: Coverage,
    /// The temporal validity start for this publication's entities
    /// (usually the AIRAC cycle's effective_from). `None` = now.
    pub valid_from: Option<DateTime<Utc>>,
    /// Publication identity for replay/conflict detection. `None`:
    /// derived per provider/dataset/cycle/kind.
    pub publication_id: Option<String>,
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
    /// Per-entity-class accepted counts (e.g. "waypoints", "procedure_legs").
    pub kind_counts: BTreeMap<String, usize>,
    /// Rejected records whose entity id could NOT be identified. For a
    /// full-snapshot publication these block close_absent semantics: a
    /// parser failure must never silently become a source deletion.
    pub unidentifiable_rejections: usize,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    pub duration_ms: u64,
    pub source_checksum: String,
}

const MAX_WARNINGS: usize = 1000;

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
        if self.warnings.len() < MAX_WARNINGS {
            self.warnings.push(reason);
        }
    }

    /// Invalid record: dropped from the store with a diagnostic.
    pub fn record_rejected(&mut self, reason: String) {
        self.records_rejected += 1;
        if self.warnings.len() < MAX_WARNINGS {
            self.warnings.push(reason);
        }
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

    /// The provider's declared manifest (ownership contract + v0.6
    /// capabilities: coverage, temporal model, update model).
    fn manifest(&self) -> &'static openairac_model::ProviderManifest {
        openairac_model::manifest_for_provider(self.name()).expect("registered provider")
    }

    /// The datasets this provider publishes.
    fn datasets(&self) -> &'static [&'static str];

    /// Fetch one raw dataset over the network (or a documented local
    /// mirror). `cycle` is `Some` for cycle-aware providers and MUST be
    /// `None` for cycle-less ones (implementations reject mismatches).
    fn fetch(&self, dataset: &str, cycle: Option<&CycleSelector>) -> Result<FetchedDataset>;

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
        .bytes()
        .with_context(|| format!("reading body of {url}"))?;
    let checksum = sha256_hex(&body);
    let raw_content = match String::from_utf8(body.to_vec()) {
        Ok(text) => text,
        Err(_) => {
            // Binary payload (e.g. a zip): byte-preserving latin-1
            // decode; providers that need the archive must extract it
            // themselves from raw_bytes.
            body.iter().map(|&b| b as char).collect::<String>()
        }
    };
    Ok(FetchedDataset {
        provider_name: provider.to_string(),
        dataset_name: dataset.to_string(),
        source_uri: url.to_string(),
        content_sha256: checksum,
        retrieved_at,
        raw_bytes: body.to_vec(),
        provider_revision,
        airac_cycle: None,
        revision_kind: RevisionKind::Baseline,
        coverage: Coverage::FullSnapshot,
        valid_from: None,
        publication_id: None,
        raw_content,
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
