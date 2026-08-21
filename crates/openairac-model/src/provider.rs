//! Generic Aviation Data Provider Model & Registry.
//!
//! Provides first-class abstractions for:
//! - Strongly typed `ProviderId` and `ProviderDescriptor`
//! - Provider capabilities, update mechanisms, and runtime priorities
//! - Provider dataset versioning and temporal validity
//! - Provider health and lifecycle state transitions
//! - Fine-grained entity provenance and source traceability
//! - Machine-readable coverage metrics
use crate::policy::{DatasetFormat, RedistributionPermission};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

/// Strongly-typed provider identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProviderId(pub String);

impl ProviderId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into().to_ascii_lowercase())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    // Well-known built-in provider identifiers
    pub fn ourairports() -> Self {
        Self("ourairports".to_string())
    }

    pub fn faa() -> Self {
        Self("faa_cifp".to_string())
    }

    pub fn sia_france() -> Self {
        Self("sia_france".to_string())
    }

    pub fn caica_russia() -> Self {
        Self("ru_caica_local".to_string())
    }

    pub fn dfs_germany() -> Self {
        Self("dfs_germany".to_string())
    }

    pub fn openflightmaps() -> Self {
        Self("openflightmaps".to_string())
    }

    pub fn synthetic_test() -> Self {
        Self("synthetic_test".to_string())
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for ProviderId {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for ProviderId {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl FromStr for ProviderId {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(s))
    }
}

/// Provider capabilities bitset / flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    pub airports: bool,
    pub runways: bool,
    pub navaids: bool,
    pub fixes: bool,
    pub procedures: bool,
    pub ats_routes: bool,
    pub airspace: bool,
    pub charts: bool,
    pub weather: bool,
    pub online_traffic: bool,
}

impl ProviderCapabilities {
    pub fn all() -> Self {
        Self {
            airports: true,
            runways: true,
            navaids: true,
            fixes: true,
            procedures: true,
            ats_routes: true,
            airspace: true,
            charts: true,
            weather: true,
            online_traffic: true,
        }
    }

    pub fn summary_list(&self) -> Vec<&'static str> {
        let mut caps = Vec::new();
        if self.airports {
            caps.push("AIRPORTS");
        }
        if self.runways {
            caps.push("RUNWAYS");
        }
        if self.navaids {
            caps.push("NAVAIDS");
        }
        if self.fixes {
            caps.push("FIXES");
        }
        if self.procedures {
            caps.push("PROCEDURES");
        }
        if self.ats_routes {
            caps.push("ATS_ROUTES");
        }
        if self.airspace {
            caps.push("AIRSPACE");
        }
        if self.charts {
            caps.push("CHARTS");
        }
        if self.weather {
            caps.push("WEATHER");
        }
        if self.online_traffic {
            caps.push("ONLINE_TRAFFIC");
        }
        caps
    }
}

/// How a provider dataset is acquired / updated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderUpdateMechanism {
    /// Remotely downloadable from an official open / public URL.
    AutoDownload,
    /// Manually supplied by user (local file / directory).
    ManualImport,
    /// Packaged in the Local AIP Vault directory (`data/vault/<provider>`).
    AipVaultPackage,
    /// Built-in synthetic fixture for test / validation.
    SyntheticFixture,
    /// Disabled or deprecated.
    Disabled,
}

/// Health and lifecycle status of a provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "detail", rename_all = "snake_case")]
pub enum ProviderHealth {
    /// Active dataset is installed, validated, and matches current temporal requirements.
    Current,
    /// Active dataset is valid but past its effective AIRAC cycle.
    Stale { active_cycle: String },
    /// Partially available / imported (some capabilities missing or limited).
    Partial { reason: String },
    /// Ingestion or validation failed / broken.
    Broken { error: String },
    /// Not installed / no active dataset available.
    NotInstalled,
    /// User action required to provide lawful source material (e.g. Local AIP Vault source files).
    SourceRequired { instructions: String },
}

impl ProviderHealth {
    pub fn is_operational(&self) -> bool {
        matches!(
            self,
            Self::Current | Self::Stale { .. } | Self::Partial { .. }
        )
    }

    pub fn display_badge(&self) -> &'static str {
        match self {
            Self::Current => "CURRENT",
            Self::Stale { .. } => "STALE",
            Self::Partial { .. } => "PARTIAL",
            Self::Broken { .. } => "BROKEN",
            Self::NotInstalled => "NOT_INSTALLED",
            Self::SourceRequired { .. } => "SOURCE_REQUIRED",
        }
    }
}

/// First-class descriptor of an aeronautical provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderDescriptor {
    pub id: ProviderId,
    pub name: String,
    pub authority: String,
    pub country: String,
    pub policy: RedistributionPermission,
    pub homepage: Option<String>,
    pub source_description: String,
    pub capabilities: ProviderCapabilities,
    pub dataset_format: DatasetFormat,
    pub update_mechanism: ProviderUpdateMechanism,
    pub effective_cycle: Option<String>,
    pub license_metadata: String,
    pub runtime_priority: u32,
    pub is_enabled: bool,
}

impl ProviderDescriptor {
    pub fn is_safe_for_public_bundle(&self) -> bool {
        self.policy == RedistributionPermission::PublicRedistribution
    }

    pub fn is_local_only(&self) -> bool {
        self.policy == RedistributionPermission::LocalOnly
    }

    pub fn is_forbidden(&self) -> bool {
        self.policy == RedistributionPermission::Forbidden
    }
}

/// Tracked dataset version for an installed / staged provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderDatasetVersion {
    pub provider_id: ProviderId,
    pub version_tag: String,
    pub airac_cycle: Option<String>,
    pub effective_from: Option<DateTime<Utc>>,
    pub effective_until: Option<DateTime<Utc>>,
    pub source_uri: Option<String>,
    pub is_active: bool,
    pub metrics: Option<ProviderCoverageMetrics>,
}

/// Fine-grained entity provenance.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProviderProvenance {
    pub provider_id: ProviderId,
    pub dataset_version: String,
    pub source_file: Option<String>,
    pub physical_page: Option<u32>,
    pub printed_page: Option<String>,
    pub table_name: Option<String>,
    pub row_index: Option<usize>,
    pub retrieved_at: Option<DateTime<Utc>>,
}

impl ProviderProvenance {
    pub fn new(provider_id: ProviderId, dataset_version: impl Into<String>) -> Self {
        Self {
            provider_id,
            dataset_version: dataset_version.into(),
            source_file: None,
            physical_page: None,
            printed_page: None,
            table_name: None,
            row_index: None,
            retrieved_at: Some(Utc::now()),
        }
    }

    pub fn with_source_file(mut self, file: impl Into<String>) -> Self {
        self.source_file = Some(file.into());
        self
    }

    pub fn with_page(mut self, physical: u32, printed: impl Into<String>) -> Self {
        self.physical_page = Some(physical);
        self.printed_page = Some(printed.into());
        self
    }

    pub fn with_table_row(mut self, table: impl Into<String>, row: usize) -> Self {
        self.table_name = Some(table.into());
        self.row_index = Some(row);
        self
    }
}

/// Terminal states for generic provider ingestion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "detail", rename_all = "snake_case")]
pub enum ProviderIngestionStatus {
    Success,
    Partial { warning: String },
    Failed { error: String },
    NotAvailable { reason: String },
    Disabled,
    PolicyBlocked { violation: String },
}

/// Machine-readable coverage metrics produced by a provider parse/import.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProviderCoverageMetrics {
    pub airports: usize,
    pub runways: usize,
    pub navaids: usize,
    pub fixes: usize,
    pub sids: usize,
    pub stars: usize,
    pub approaches: usize,
    pub total_procedures: usize,
    pub ats_routes: usize,
    pub ats_segments: usize,
    pub airspace_objects: usize,
    pub parsed_count: usize,
    pub partial_count: usize,
    pub unsupported_count: usize,
    pub rejected_count: usize,
    pub source_provenance_count: usize,
    pub validation_errors: usize,
}

impl ProviderCoverageMetrics {
    pub fn accounting_equation_holds(&self, expected_total: usize) -> bool {
        self.parsed_count + self.partial_count + self.unsupported_count + self.rejected_count
            == expected_total
    }
}

/// Central Provider Registry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderRegistryV2 {
    pub providers: BTreeMap<ProviderId, ProviderDescriptor>,
}

impl ProviderRegistryV2 {
    pub fn new() -> Self {
        Self {
            providers: BTreeMap::new(),
        }
    }

    pub fn register(&mut self, descriptor: ProviderDescriptor) {
        self.providers.insert(descriptor.id.clone(), descriptor);
    }

    pub fn get(&self, id: &ProviderId) -> Option<&ProviderDescriptor> {
        self.providers.get(id)
    }

    pub fn list(&self) -> Vec<&ProviderDescriptor> {
        self.providers.values().collect()
    }

    pub fn list_enabled(&self) -> Vec<&ProviderDescriptor> {
        self.providers.values().filter(|p| p.is_enabled).collect()
    }

    pub fn default_registry() -> Self {
        let mut reg = Self::new();

        // 1. OurAirports Community Public Domain Baseline
        reg.register(ProviderDescriptor {
            id: ProviderId::ourairports(),
            name: "OurAirports".to_string(),
            authority: "OurAirports Community".to_string(),
            country: "GLOBAL".to_string(),
            policy: RedistributionPermission::PublicRedistribution,
            homepage: Some("https://ourairports.com/data/".to_string()),
            source_description: "Worldwide open aerodrome, runway, navaid, and frequency baseline"
                .to_string(),
            capabilities: ProviderCapabilities {
                airports: true,
                runways: true,
                navaids: true,
                fixes: false,
                procedures: false,
                ats_routes: false,
                airspace: false,
                charts: false,
                weather: false,
                online_traffic: false,
            },
            dataset_format: DatasetFormat::OpenAirportsCsv,
            update_mechanism: ProviderUpdateMechanism::AutoDownload,
            effective_cycle: None,
            license_metadata: "CC0 1.0 Universal / Public Domain".to_string(),
            runtime_priority: 10,
            is_enabled: true,
        });

        // 2. FAA CIFP (United States Federal Aviation Administration)
        reg.register(ProviderDescriptor {
            id: ProviderId::faa(),
            name: "FAA CIFP".to_string(),
            authority: "Federal Aviation Administration (US)".to_string(),
            country: "US".to_string(),
            policy: RedistributionPermission::PublicRedistribution,
            homepage: Some("https://www.faa.gov/air_traffic/flight_info/aeronav/digital_products/cifp/".to_string()),
            source_description: "Complete US instrument flight procedures, airways, fixes, and navigation database in ARINC 424".to_string(),
            capabilities: ProviderCapabilities {
                airports: true,
                runways: true,
                navaids: true,
                fixes: true,
                procedures: true,
                ats_routes: true,
                airspace: true,
                charts: false,
                weather: false,
                online_traffic: false,
            },
            dataset_format: DatasetFormat::Arinc424,
            update_mechanism: ProviderUpdateMechanism::AutoDownload,
            effective_cycle: Some("2608".to_string()),
            license_metadata: "Public Domain (U.S. Government Work)".to_string(),
            runtime_priority: 50,
            is_enabled: true,
        });

        // 3. SIA France (Service de l'Information Aeronautique)
        reg.register(ProviderDescriptor {
            id: ProviderId::sia_france(),
            name: "SIA France".to_string(),
            authority: "Service de l'Information Aeronautique (France)".to_string(),
            country: "FR".to_string(),
            policy: RedistributionPermission::PublicRedistribution,
            homepage: Some("https://www.sia.aviation-civile.gouv.fr/".to_string()),
            source_description: "French national aeronautical dataset in AIXM 4.5 and official procedure publications".to_string(),
            capabilities: ProviderCapabilities {
                airports: true,
                runways: true,
                navaids: true,
                fixes: true,
                procedures: true,
                ats_routes: true,
                airspace: true,
                charts: false,
                weather: false,
                online_traffic: false,
            },
            dataset_format: DatasetFormat::Aixm4,
            update_mechanism: ProviderUpdateMechanism::AutoDownload,
            effective_cycle: Some("2608".to_string()),
            license_metadata: "Licence Ouverte / Open Licence 2.0".to_string(),
            runtime_priority: 60,
            is_enabled: true,
        });

        // 4. CAICA Russia Local AIP Vault
        reg.register(ProviderDescriptor {
            id: ProviderId::caica_russia(),
            name: "CAICA Russia Local AIP Vault".to_string(),
            authority: "Federal Air Transport Agency (Rosaviatsiya) / CAICA".to_string(),
            country: "RU".to_string(),
            policy: RedistributionPermission::LocalOnly,
            homepage: Some("https://www.caica.ru".to_string()),
            source_description: "Official Russian procedure coding tables and ATS Route Manual for local BYOD ingestion".to_string(),
            capabilities: ProviderCapabilities {
                airports: true,
                runways: true,
                navaids: true,
                fixes: true,
                procedures: true,
                ats_routes: true,
                airspace: true,
                charts: false,
                weather: false,
                online_traffic: false,
            },
            dataset_format: DatasetFormat::CaicaHtml,
            update_mechanism: ProviderUpdateMechanism::AipVaultPackage,
            effective_cycle: Some("2608".to_string()),
            license_metadata: "National Aeronautical Publication (Local Personal BYOD Use Only)".to_string(),
            runtime_priority: 100,
            is_enabled: true,
        });

        // 5. DFS Germany
        reg.register(ProviderDescriptor {
            id: ProviderId::dfs_germany(),
            name: "DFS Germany".to_string(),
            authority: "DFS Deutsche Flugsicherung GmbH".to_string(),
            country: "DE".to_string(),
            policy: RedistributionPermission::LocalOnly,
            homepage: Some("https://aip.dfs.de".to_string()),
            source_description: "German official AIP aeronautical data (AIP IFR/VFR)".to_string(),
            capabilities: ProviderCapabilities {
                airports: true,
                runways: true,
                navaids: true,
                fixes: true,
                procedures: true,
                ats_routes: true,
                airspace: true,
                charts: false,
                weather: false,
                online_traffic: false,
            },
            dataset_format: DatasetFormat::Aixm5,
            update_mechanism: ProviderUpdateMechanism::ManualImport,
            effective_cycle: Some("2608".to_string()),
            license_metadata: "DFS Copyright / Restricted Redistribution (Local Use Only)"
                .to_string(),
            runtime_priority: 90,
            is_enabled: true,
        });

        // 6. OpenFlightMaps
        reg.register(ProviderDescriptor {
            id: ProviderId::openflightmaps(),
            name: "open flightmaps".to_string(),
            authority: "open flightmaps Association".to_string(),
            country: "EU".to_string(),
            policy: RedistributionPermission::PublicRedistribution,
            homepage: Some("https://openflightmaps.org".to_string()),
            source_description:
                "Open VFR aeronautical chart and airspace data for European regions in AIXM 4.5"
                    .to_string(),
            capabilities: ProviderCapabilities {
                airports: true,
                runways: true,
                navaids: true,
                fixes: true,
                procedures: false,
                ats_routes: false,
                airspace: true,
                charts: true,
                weather: false,
                online_traffic: false,
            },
            dataset_format: DatasetFormat::Aixm4,
            update_mechanism: ProviderUpdateMechanism::AutoDownload,
            effective_cycle: Some("2608".to_string()),
            license_metadata: "ODbL (Open Database License)".to_string(),
            runtime_priority: 40,
            is_enabled: true,
        });

        // 7. Synthetic Test Provider (For SDK testing and isolated E2E tests)
        reg.register(ProviderDescriptor {
            id: ProviderId::synthetic_test(),
            name: "Synthetic Test Provider".to_string(),
            authority: "OpenAIRAC Test Fixture Authority".to_string(),
            country: "ZZ".to_string(),
            policy: RedistributionPermission::PublicRedistribution,
            homepage: None,
            source_description:
                "Deterministic synthetic aviation dataset used strictly for integration testing"
                    .to_string(),
            capabilities: ProviderCapabilities {
                airports: true,
                runways: true,
                navaids: true,
                fixes: true,
                procedures: true,
                ats_routes: true,
                airspace: false,
                charts: false,
                weather: false,
                online_traffic: false,
            },
            dataset_format: DatasetFormat::CustomJson,
            update_mechanism: ProviderUpdateMechanism::SyntheticFixture,
            effective_cycle: Some("2608".to_string()),
            license_metadata: "CC0-1.0 (Public Domain Test Fixture)".to_string(),
            runtime_priority: 5,
            is_enabled: false, // Disabled by default in production
        });

        reg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_id_and_registry() {
        let reg = ProviderRegistryV2::default_registry();
        assert_eq!(reg.list().len(), 7);

        let ourairports = reg.get(&ProviderId::ourairports()).unwrap();
        assert!(ourairports.is_safe_for_public_bundle());
        assert_eq!(
            ourairports.policy,
            RedistributionPermission::PublicRedistribution
        );

        let caica = reg.get(&ProviderId::caica_russia()).unwrap();
        assert!(caica.is_local_only());
        assert!(!caica.is_safe_for_public_bundle());

        let synthetic = reg.get(&ProviderId::synthetic_test()).unwrap();
        assert!(!synthetic.is_enabled);
    }

    #[test]
    fn test_provenance_chain() {
        let prov = ProviderProvenance::new(ProviderId::caica_russia(), "AIRAC 2608")
            .with_source_file("ATS_Routes_Manual_06.08.2026.pdf")
            .with_page(22, "1.1-11")
            .with_table_row("A300", 0);

        assert_eq!(prov.provider_id.as_str(), "ru_caica_local");
        assert_eq!(prov.dataset_version, "AIRAC 2608");
        assert_eq!(prov.physical_page, Some(22));
        assert_eq!(prov.table_name.as_deref(), Some("A300"));
    }
}
