//! Provider and Source Policy, Licensing, and Provenance Layer.
//!
//! Provides first-class concepts for:
//! - Source jurisdiction and aviation authority
//! - Dataset formats (ARINC 424, AIXM 5.x, AIXM 4.x, OpenAirports CSV, etc.)
//! - Licensing identification (SPDX / official open data licenses)
//! - Redistribution permissions (`PublicRedistribution`, `LocalOnly`, `MetadataOnly`, `Forbidden`)
//! - Derivative work and attribution terms
//! - Entity coverage capabilities (airports, runways, navaids, fixes, airways, SIDs, STARs, approaches, etc.)
//! - Machine-readable registry of official worldwide providers (`providers.yaml`)

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// Legal redistribution permission level for an aeronautical dataset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RedistributionPermission {
    /// Officially open / public domain data permissible for worldwide public redistribution in OpenAIRAC bundles.
    PublicRedistribution,
    /// Authorized for local/personal use by the end-user (BYOD, EAD account, local AIP download),
    /// but strictly FORBIDDEN from public redistribution in official OpenAIRAC release bundles.
    LocalOnly,
    /// Only high-level metadata (frequencies, identifier cross-references) may be published;
    /// geometric and procedure payloads must remain local.
    MetadataOnly,
    /// Proprietary, encrypted, or contractually protected data (e.g. Navigraph, Jeppesen, NavDataPro).
    /// OpenAIRAC MUST NOT ingest, store, redistribute, or use these datasets.
    Forbidden,
}

impl RedistributionPermission {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PublicRedistribution => "public_redistribution",
            Self::LocalOnly => "local_only",
            Self::MetadataOnly => "metadata_only",
            Self::Forbidden => "forbidden",
        }
    }

    pub fn is_publicly_redistributable(&self) -> bool {
        matches!(self, Self::PublicRedistribution)
    }

    pub fn is_allowed_for_local_use(&self) -> bool {
        matches!(
            self,
            Self::PublicRedistribution | Self::LocalOnly | Self::MetadataOnly
        )
    }

    pub fn is_forbidden(&self) -> bool {
        matches!(self, Self::Forbidden)
    }
}

/// Aeronautical data source format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatasetFormat {
    /// ARINC 424 fixed-width record format (e.g., ARINC 424-18/19/20/21).
    Arinc424,
    /// Aeronautical Information Exchange Model version 5.x (XML/GML).
    Aixm5,
    /// Aeronautical Information Exchange Model version 4.x / 4.5.
    Aixm4,
    /// OpenAirports CSV tables (airports.csv, runways.csv, navaids.csv).
    OpenAirportsCsv,
    /// Custom or provider-specific CSV.
    CustomCsv,
    /// Custom XML format.
    CustomXml,
    /// Custom JSON format.
    CustomJson,
    /// Official procedure coding publication table format (e.g. SIA DATA, ENAIRE tabular).
    ProcedurePublicationPdf,
    /// Official Russian CAICA HTML procedure coding collection.
    CaicaHtml,
}

impl DatasetFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Arinc424 => "arinc424",
            Self::Aixm5 => "aixm5",
            Self::Aixm4 => "aixm4",
            Self::OpenAirportsCsv => "openairports_csv",
            Self::CustomCsv => "custom_csv",
            Self::CustomXml => "custom_xml",
            Self::CustomJson => "custom_json",
            Self::ProcedurePublicationPdf => "procedure_pub_pdf",
            Self::CaicaHtml => "caica_html",
        }
    }
}

/// Declared entity coverage capabilities of a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProviderEntityCoverage {
    pub airports: bool,
    pub runways: bool,
    pub navaids: bool,
    pub fixes: bool,
    pub airways: bool,
    pub sids: bool,
    pub stars: bool,
    pub approaches: bool,
    pub lpv_fas: bool,
    pub msa: bool,
    pub mora: bool,
}

impl ProviderEntityCoverage {
    pub fn has_procedures(&self) -> bool {
        self.sids || self.stars || self.approaches
    }

    pub fn has_terminal_nav(&self) -> bool {
        self.airports || self.runways || self.navaids || self.fixes
    }

    pub fn summary_string(&self) -> String {
        let mut parts = Vec::new();
        if self.airports {
            parts.push("APT");
        }
        if self.runways {
            parts.push("RWY");
        }
        if self.navaids {
            parts.push("NAV");
        }
        if self.fixes {
            parts.push("FIX");
        }
        if self.airways {
            parts.push("AWY");
        }
        if self.sids {
            parts.push("SID");
        }
        if self.stars {
            parts.push("STAR");
        }
        if self.approaches {
            parts.push("APP");
        }
        if self.lpv_fas {
            parts.push("FAS");
        }
        if self.msa {
            parts.push("MSA");
        }
        if self.mora {
            parts.push("MORA");
        }
        if parts.is_empty() {
            "none".to_string()
        } else {
            parts.join(",")
        }
    }
}

/// Machine-readable provider policy and licensing contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderPolicy {
    /// Unique provider identifier (matches `source_snapshots.provider`).
    pub name: String,
    /// Object-id namespace prefix (e.g. `faa`, `ourairports`, `ead`, `byod`).
    pub namespace: String,
    /// Geographic jurisdiction (e.g., "US", "GLOBAL", "EU", "DE", "FR", "BYOD").
    pub jurisdiction: String,
    /// Publishing authority (e.g., "FAA", "OurAirports Community", "EUROCONTROL", "DFS", "BYOD User").
    pub authority: String,
    /// Data serialization format.
    pub format: DatasetFormat,
    /// Publication cadence (e.g. "28-day AIRAC", "continuous", "ad-hoc").
    pub airac_cadence: Option<String>,
    /// SPDX license identifier or legal license description.
    pub license_id: String,
    /// Redistribution permission tier.
    pub redistribution: RedistributionPermission,
    /// Permission to create derivative formats and export simulator databases.
    pub derivative_work_allowed: bool,
    /// Whether attribution notice is legally required in releases/manifests.
    pub attribution_required: bool,
    /// Canonical attribution notice if required.
    pub attribution_notice: Option<String>,
    /// Flag explicitly marking local-use-only datasets.
    pub is_local_only: bool,
    /// Entity capabilities published by this provider.
    pub coverage: ProviderEntityCoverage,
    /// Expected source URI or download URL template.
    pub source_uri_pattern: Option<String>,
    /// Additional legal or operational notes.
    pub notes: Option<String>,
}

impl ProviderPolicy {
    /// Check whether this provider is safe and legal to include in public OpenAIRAC release bundles.
    pub fn is_safe_for_public_bundle(&self) -> bool {
        self.redistribution.is_publicly_redistributable()
            && !self.is_local_only
            && !self.redistribution.is_forbidden()
    }

    /// Check whether this provider is allowed for local BYOD bundle generation.
    pub fn is_allowed_for_local_bundle(&self) -> bool {
        self.redistribution.is_allowed_for_local_use() && !self.redistribution.is_forbidden()
    }
}

/// Provider registry container for worldwide aeronautical authorities and BYOD sources.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderRegistry {
    pub providers: BTreeMap<String, ProviderPolicy>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: BTreeMap::new(),
        }
    }

    /// Insert or update a provider policy.
    pub fn register(&mut self, policy: ProviderPolicy) {
        self.providers.insert(policy.name.clone(), policy);
    }

    /// Lookup a provider policy by exact name or namespace.
    pub fn get(&self, name_or_namespace: &str) -> Option<&ProviderPolicy> {
        if let Some(p) = self.providers.get(name_or_namespace) {
            return Some(p);
        }
        self.providers
            .values()
            .find(|p| p.namespace.eq_ignore_ascii_case(name_or_namespace))
    }

    /// Check if a given provider name is allowed for public redistribution.
    pub fn is_redistributable(&self, provider_name: &str) -> bool {
        self.get(provider_name)
            .map(|p| p.is_safe_for_public_bundle())
            .unwrap_or(false)
    }

    /// Check if a given provider name is marked local-only.
    pub fn is_local_only(&self, provider_name: &str) -> bool {
        self.get(provider_name)
            .map(|p| p.is_local_only || p.redistribution == RedistributionPermission::LocalOnly)
            .unwrap_or(true) // Unknown providers fail closed as local-only
    }

    /// Check if a provider is explicitly forbidden.
    pub fn is_forbidden(&self, provider_name: &str) -> bool {
        self.get(provider_name)
            .map(|p| p.redistribution.is_forbidden())
            .unwrap_or(false)
    }

    /// Parse a registry from YAML string.
    pub fn from_yaml(yaml: &str) -> Result<Self> {
        let registry: ProviderRegistry =
            serde_yaml::from_str(yaml).context("deserializing provider registry from YAML")?;
        Ok(registry)
    }

    /// Serialize registry to YAML string.
    pub fn to_yaml(&self) -> Result<String> {
        serde_yaml::to_string(self).context("serializing provider registry to YAML")
    }

    /// Built-in canonical registry containing worldwide official authorities, open data providers, and BYOD templates.
    pub fn default_registry() -> Self {
        let mut reg = Self::new();

        // 1. FAA CIFP (United States Federal Aviation Administration)
        reg.register(ProviderPolicy {
            name: "FAA_CIFP".to_string(),
            namespace: "faa".to_string(),
            jurisdiction: "US".to_string(),
            authority: "Federal Aviation Administration".to_string(),
            format: DatasetFormat::Arinc424,
            airac_cadence: Some("28-day AIRAC".to_string()),
            license_id: "PublicDomain-US-Gov".to_string(),
            redistribution: RedistributionPermission::PublicRedistribution,
            derivative_work_allowed: true,
            attribution_required: false,
            attribution_notice: Some(
                "FAA Aeronautical Information Services CIFP (Public Domain)".to_string(),
            ),
            is_local_only: false,
            coverage: ProviderEntityCoverage {
                airports: true,
                runways: true,
                navaids: true,
                fixes: true,
                airways: true,
                sids: true,
                stars: true,
                approaches: true,
                lpv_fas: true,
                msa: true,
                mora: true,
            },
            source_uri_pattern: Some(
                "https://nfdc.faa.gov/webContent/28DaySub/{cycle}_CSV.zip".to_string(),
            ),
            notes: Some(
                "US nationwide coverage with comprehensive ARINC 424-18 procedure semantics."
                    .to_string(),
            ),
        });

        // 2. FAA AIXM (FAA NASR 28-Day Subscription in AIXM 5.1)
        reg.register(ProviderPolicy {
            name: "FAA_AIXM".to_string(),
            namespace: "faa_aixm".to_string(),
            jurisdiction: "US".to_string(),
            authority: "Federal Aviation Administration".to_string(),
            format: DatasetFormat::Aixm5,
            airac_cadence: Some("28-day AIRAC".to_string()),
            license_id: "PublicDomain-US-Gov".to_string(),
            redistribution: RedistributionPermission::PublicRedistribution,
            derivative_work_allowed: true,
            attribution_required: false,
            attribution_notice: Some("FAA NASR AIXM 5.1 (Public Domain)".to_string()),
            is_local_only: false,
            coverage: ProviderEntityCoverage {
                airports: true,
                runways: true,
                navaids: true,
                fixes: true,
                airways: true,
                sids: true,
                stars: true,
                approaches: true,
                lpv_fas: true,
                msa: true,
                mora: false,
            },
            source_uri_pattern: Some(
                "https://nfdc.faa.gov/webContent/28DaySub/aixm5.1_{cycle}.zip".to_string(),
            ),
            notes: Some("Generic AIXM 5.1 ingestion for US NASR datasets.".to_string()),
        });

        // 3. OurAirports (Global Community Open Data)
        reg.register(ProviderPolicy {
            name: "OurAirports".to_string(),
            namespace: "ourairports".to_string(),
            jurisdiction: "GLOBAL".to_string(),
            authority: "OurAirports Community".to_string(),
            format: DatasetFormat::OpenAirportsCsv,
            airac_cadence: Some("continuous daily".to_string()),
            license_id: "CC0-1.0".to_string(),
            redistribution: RedistributionPermission::PublicRedistribution,
            derivative_work_allowed: true,
            attribution_required: false,
            attribution_notice: Some(
                "OurAirports.com (Dedicated to Public Domain via CC0 1.0)".to_string(),
            ),
            is_local_only: false,
            coverage: ProviderEntityCoverage {
                airports: true,
                runways: true,
                navaids: true,
                fixes: false,
                airways: false,
                sids: false,
                stars: false,
                approaches: false,
                lpv_fas: false,
                msa: false,
                mora: false,
            },
            source_uri_pattern: Some(
                "https://davidmegginson.github.io/ourairports-data/{dataset}.csv".to_string(),
            ),
            notes: Some("Worldwide airport, runway, and radio navaid basic metadata.".to_string()),
        });

        // 4. openflightmaps (OFMA Open Aeronautical Data)
        reg.register(ProviderPolicy {
            name: "OpenFlightmaps".to_string(),
            namespace: "ofm".to_string(),
            jurisdiction: "EU".to_string(),
            authority: "open flightmaps association".to_string(),
            format: DatasetFormat::Aixm5,
            airac_cadence: Some("28-day AIRAC".to_string()),
            license_id: "OFMA-Open-Data".to_string(),
            redistribution: RedistributionPermission::PublicRedistribution,
            derivative_work_allowed: true,
            attribution_required: true,
            attribution_notice: Some("open flightmaps (OFMA)".to_string()),
            is_local_only: false,
            coverage: ProviderEntityCoverage {
                airports: true,
                runways: true,
                navaids: true,
                fixes: true,
                airways: true,
                sids: false,
                stars: false,
                approaches: false,
                lpv_fas: false,
                msa: false,
                mora: false,
            },
            source_uri_pattern: Some("https://openflightmaps.org/".to_string()),
            notes: Some(
                "Open VFR and enroute aeronautical data for European countries.".to_string(),
            ),
        });

        // 5a. DFS Germany INSPIRE Open Geodata (Deutsche Flugsicherung)
        reg.register(ProviderPolicy {
            name: "DFS_INSPIRE".to_string(),
            namespace: "dfs".to_string(),
            jurisdiction: "DE".to_string(),
            authority: "DFS Deutsche Flugsicherung GmbH (INSPIRE Open Data)".to_string(),
            format: DatasetFormat::Aixm5,
            airac_cadence: Some("28-day AIRAC".to_string()),
            license_id: "GeoNutzV-OpenData".to_string(),
            redistribution: RedistributionPermission::PublicRedistribution,
            derivative_work_allowed: true,
            attribution_required: true,
            attribution_notice: Some("DFS Deutsche Flugsicherung GmbH (INSPIRE GeoNutzV)".to_string()),
            is_local_only: false,
            coverage: ProviderEntityCoverage {
                airports: true,
                runways: true,
                navaids: true,
                fixes: true,
                airways: true,
                sids: false,
                stars: false,
                approaches: false,
                lpv_fas: false,
                msa: false,
                mora: false,
            },
            source_uri_pattern: Some("https://aip.dfs.de/".to_string()),
            notes: Some("German INSPIRE open aeronautical geodata: aerodromes, runways, navaids, waypoints, airways.".to_string()),
        });

        // 5b. DFS Germany AIS Portal (Local-Only / Authenticated)
        reg.register(ProviderPolicy {
            name: "DFS_AIS".to_string(),
            namespace: "dfs_ais".to_string(),
            jurisdiction: "DE".to_string(),
            authority: "DFS Deutsche Flugsicherung GmbH (AIS Portal)".to_string(),
            format: DatasetFormat::Aixm5,
            airac_cadence: Some("28-day AIRAC".to_string()),
            license_id: "DFS-AIS-TermsOfUse".to_string(),
            redistribution: RedistributionPermission::LocalOnly,
            derivative_work_allowed: true,
            attribution_required: true,
            attribution_notice: Some("DFS Deutsche Flugsicherung GmbH AIS Portal".to_string()),
            is_local_only: true,
            coverage: ProviderEntityCoverage {
                airports: true,
                runways: true,
                navaids: true,
                fixes: true,
                airways: true,
                sids: true,
                stars: true,
                approaches: true,
                lpv_fas: true,
                msa: true,
                mora: false,
            },
            source_uri_pattern: Some("https://ais.dfs.de/".to_string()),
            notes: Some("German AIP eAIP/AIS terminal flight procedures; user account required, local use only.".to_string()),
        });
        // 5b. French SIA (Service de l'Information Aeronautique - DGAC France)
        reg.register(ProviderPolicy {
            name: "FR_SIA".to_string(),
            namespace: "sia".to_string(),
            jurisdiction: "FR".to_string(),
            authority: "Service de l'Information Aeronautique (DGAC France)".to_string(),
            format: DatasetFormat::Aixm4,
            airac_cadence: Some("28-day AIRAC".to_string()),
            license_id: "Licence-Ouverte-v2.0".to_string(),
            redistribution: RedistributionPermission::PublicRedistribution,
            derivative_work_allowed: true,
            attribution_required: true,
            attribution_notice: Some(
                "Service de l'Information Aeronautique - DGAC France (Licence Ouverte v2.0)"
                    .to_string(),
            ),
            is_local_only: false,
            coverage: ProviderEntityCoverage {
                airports: true,
                runways: true,
                navaids: true,
                fixes: true,
                airways: true,
                sids: true,
                stars: true,
                approaches: true,
                lpv_fas: false,
                msa: false,
                mora: false,
            },
            source_uri_pattern: Some("http://data.cquest.org/dgac/aip/".to_string()),
            notes: Some(
                "French AIP / AIXM 4.5 aeronautical data under Etalab Licence Ouverte v2.0."
                    .to_string(),
            ),
        });

        // 5c. French SIA Structured Procedure Publications (DATA SID, DATA STAR, DATA RNP Approach)
        reg.register(ProviderPolicy {
            name: "FR_SIA_PROCEDURES".to_string(),
            namespace: "sia_proc".to_string(),
            jurisdiction: "FR".to_string(),
            authority: "Service de l'Information Aeronautique (DGAC France)".to_string(),
            format: DatasetFormat::ProcedurePublicationPdf,
            airac_cadence: Some("28-day AIRAC".to_string()),
            license_id: "Licence-Ouverte-v2.0".to_string(),
            redistribution: RedistributionPermission::PublicRedistribution,
            derivative_work_allowed: true,
            attribution_required: true,
            attribution_notice: Some(
                "Service de l'Information Aeronautique - DGAC France (Licence Ouverte v2.0)"
                    .to_string(),
            ),
            is_local_only: false,
            coverage: ProviderEntityCoverage {
                airports: false,
                runways: false,
                navaids: false,
                fixes: true,
                airways: false,
                sids: true,
                stars: true,
                approaches: true,
                lpv_fas: false,
                msa: false,
                mora: false,
            },
            source_uri_pattern: Some("https://www.sia.aviation-civile.gouv.fr/".to_string()),
            notes: Some(
                "French SIA official structured procedure database tables (DATA SID/STAR/RNP) under Licence Ouverte v2.0."
                    .to_string(),
            ),
        });

        // 5d. Spain ENAIRE AIP Tabular Procedures (Local-Only / BYOD)
        reg.register(ProviderPolicy {
            name: "ES_ENAIRE_PROCEDURES".to_string(),
            namespace: "enaire_proc".to_string(),
            jurisdiction: "ES".to_string(),
            authority: "ENAIRE (AIP Espana)".to_string(),
            format: DatasetFormat::ProcedurePublicationPdf,
            airac_cadence: Some("28-day AIRAC".to_string()),
            license_id: "ENAIRE-AIP-TermsOfUse".to_string(),
            redistribution: RedistributionPermission::LocalOnly,
            derivative_work_allowed: true,
            attribution_required: true,
            attribution_notice: Some("ENAIRE AIP Espana".to_string()),
            is_local_only: true,
            coverage: ProviderEntityCoverage {
                airports: false,
                runways: false,
                navaids: false,
                fixes: true,
                airways: false,
                sids: true,
                stars: true,
                approaches: true,
                lpv_fas: false,
                msa: false,
                mora: false,
            },
            source_uri_pattern: Some("https://aip.enaire.es/".to_string()),
            notes: Some(
                "Spanish ENAIRE AIP tabular procedure descriptions; redistribution restricted without prior written authorization, local use only."
                    .to_string(),
            ),
        });

        // 5e. Russian Federation CAICA Official Procedure Coding (Local-Only / BYOD)
        reg.register(ProviderPolicy {
            name: "RU_CAICA_PROCEDURES".to_string(),
            namespace: "caica_proc".to_string(),
            jurisdiction: "RU".to_string(),
            authority: "ФАВТ / Росавиация / ЦАИ (Центр Аэронавигационной Информации)".to_string(),
            format: DatasetFormat::CaicaHtml,
            airac_cadence: Some("28-day AIRAC".to_string()),
            license_id: "CAICA-TermsOfUse".to_string(),
            redistribution: RedistributionPermission::LocalOnly,
            derivative_work_allowed: true,
            attribution_required: true,
            attribution_notice: Some("ФГУП «Госкорпорация по ОрВД» / Центр Аэронавигационной Информации (ЦАИ)".to_string()),
            is_local_only: true,
            coverage: ProviderEntityCoverage {
                airports: false,
                runways: false,
                navaids: false,
                fixes: true,
                airways: false,
                sids: true,
                stars: true,
                approaches: true,
                lpv_fas: false,
                msa: false,
                mora: false,
            },
            source_uri_pattern: Some("http://www.caica.ru/".to_string()),
            notes: Some(
                "Official Russian Federation RNAV procedure coding collection; open access publication, strictly Local-Only for derived navigation data unless explicit redistribution permission granted."
                    .to_string(),
            ),
        });

        // 5f. Russian Federation CAICA Full Navigation Provider (Local-Only / BYOD)
        reg.register(ProviderPolicy {
            name: "RU_CAICA".to_string(),
            namespace: "caica".to_string(),
            jurisdiction: "RU".to_string(),
            authority: "ФГУП «Госкорпорация по ОрВД» / Центр Аэронавигационной Информации (ЦАИ)"
                .to_string(),
            format: DatasetFormat::CaicaHtml,
            airac_cadence: Some("28-day AIRAC".to_string()),
            license_id: "CAICA-TermsOfUse".to_string(),
            redistribution: RedistributionPermission::LocalOnly,
            derivative_work_allowed: true,
            attribution_required: true,
            attribution_notice: Some("ФГУП «Госкорпорация по ОрВД» / ЦАИ".to_string()),
            is_local_only: true,
            coverage: ProviderEntityCoverage {
                airports: true,
                runways: true,
                navaids: true,
                fixes: true,
                airways: true,
                sids: true,
                stars: true,
                approaches: true,
                lpv_fas: true,
                msa: true,
                mora: false,
            },
            source_uri_pattern: Some("http://www.caica.ru/".to_string()),
            notes: Some(
                "Comprehensive Russian Federation aeronautical data provider in Local AIP Vault."
                    .to_string(),
            ),
        });

        // 5g. Russian Federation ARNAD Commercial Database (Local-Only / BYOD)
        reg.register(ProviderPolicy {
            name: "RU_ARNAD_LOCAL".to_string(),
            namespace: "arnad_local".to_string(),
            jurisdiction: "RU".to_string(),
            authority: "ЦАИ / ARNAD".to_string(),
            format: DatasetFormat::Arinc424,
            airac_cadence: Some("28-day AIRAC".to_string()),
            license_id: "ARNAD-Commercial-License".to_string(),
            redistribution: RedistributionPermission::LocalOnly,
            derivative_work_allowed: true,
            attribution_required: true,
            attribution_notice: Some("ЦАИ ARNAD (Local Use Only)".to_string()),
            is_local_only: true,
            coverage: ProviderEntityCoverage {
                airports: true,
                runways: true,
                navaids: true,
                fixes: true,
                airways: true,
                sids: true,
                stars: true,
                approaches: true,
                lpv_fas: true,
                msa: true,
                mora: true,
            },
            source_uri_pattern: None,
            notes: Some("User-supplied commercial ARNAD ARINC 424 navigation database for local aircraft interoperability.".to_string()),
        });
        // 6. Eurocontrol EAD (European AIS Database) - Local-Only / BYOD
        reg.register(ProviderPolicy {
            name: "Eurocontrol_EAD".to_string(),
            namespace: "ead".to_string(),
            jurisdiction: "EU".to_string(),
            authority: "EUROCONTROL European AIS Database".to_string(),
            format: DatasetFormat::Aixm5,
            airac_cadence: Some("28-day AIRAC".to_string()),
            license_id: "Eurocontrol-EAD-TermsOfUse".to_string(),
            redistribution: RedistributionPermission::LocalOnly,
            derivative_work_allowed: true,
            attribution_required: true,
            attribution_notice: Some("EUROCONTROL EAD (Local Use Only; User Authenticated)".to_string()),
            is_local_only: true,
            coverage: ProviderEntityCoverage {
                airports: true,
                runways: true,
                navaids: true,
                fixes: true,
                airways: true,
                sids: true,
                stars: true,
                approaches: true,
                lpv_fas: true,
                msa: true,
                mora: false,
            },
            source_uri_pattern: Some("https://www.ead.eurocontrol.int/".to_string()),
            notes: Some("Requires user personal EAD account. Permitted for local personal compiler use, forbidden from public redistribution.".to_string()),
        });

        // 7. BYOD Generic AIXM Importer (User Bring-Your-Own-Data)
        reg.register(ProviderPolicy {
            name: "BYOD_AIXM".to_string(),
            namespace: "byod".to_string(),
            jurisdiction: "BYOD".to_string(),
            authority: "User Bring-Your-Own-Data".to_string(),
            format: DatasetFormat::Aixm5,
            airac_cadence: Some("ad-hoc".to_string()),
            license_id: "BYOD-Local-License".to_string(),
            redistribution: RedistributionPermission::LocalOnly,
            derivative_work_allowed: true,
            attribution_required: false,
            attribution_notice: None,
            is_local_only: true,
            coverage: ProviderEntityCoverage {
                airports: true,
                runways: true,
                navaids: true,
                fixes: true,
                airways: true,
                sids: true,
                stars: true,
                approaches: true,
                lpv_fas: true,
                msa: true,
                mora: true,
            },
            source_uri_pattern: None,
            notes: Some("User-imported AIXM dataset. Strictly local-only, excluded from public bundle publication.".to_string()),
        });

        // 8. BYOD Generic ARINC 424 Importer
        reg.register(ProviderPolicy {
            name: "BYOD_ARINC424".to_string(),
            namespace: "byod_424".to_string(),
            jurisdiction: "BYOD".to_string(),
            authority: "User Bring-Your-Own-Data".to_string(),
            format: DatasetFormat::Arinc424,
            airac_cadence: Some("ad-hoc".to_string()),
            license_id: "BYOD-Local-License".to_string(),
            redistribution: RedistributionPermission::LocalOnly,
            derivative_work_allowed: true,
            attribution_required: false,
            attribution_notice: None,
            is_local_only: true,
            coverage: ProviderEntityCoverage {
                airports: true,
                runways: true,
                navaids: true,
                fixes: true,
                airways: true,
                sids: true,
                stars: true,
                approaches: true,
                lpv_fas: true,
                msa: true,
                mora: true,
            },
            source_uri_pattern: None,
            notes: Some("User-imported ARINC 424 dataset for local compiler use.".to_string()),
        });

        // 9. Proprietary / Restricted Providers (Forbidden)
        reg.register(ProviderPolicy {
            name: "Navigraph_Forbidden".to_string(),
            namespace: "navigraph_forbidden".to_string(),
            jurisdiction: "COMMERCIAL".to_string(),
            authority: "Navigraph / Jeppesen".to_string(),
            format: DatasetFormat::Arinc424,
            airac_cadence: Some("28-day AIRAC".to_string()),
            license_id: "Proprietary-Restricted".to_string(),
            redistribution: RedistributionPermission::Forbidden,
            derivative_work_allowed: false,
            attribution_required: false,
            attribution_notice: None,
            is_local_only: true,
            coverage: ProviderEntityCoverage::default(),
            source_uri_pattern: None,
            notes: Some("Proprietary commercial navdata. Ingestion, distribution, and repository inclusion are strictly FORBIDDEN.".to_string()),
        });

        reg
    }
}

/// Embedded global provider registry instance.
pub static GLOBAL_PROVIDER_REGISTRY: LazyLock<ProviderRegistry> =
    LazyLock::new(ProviderRegistry::default_registry);

/// Helper to get a provider policy from the global registry.
pub fn get_provider_policy(name_or_namespace: &str) -> Option<&'static ProviderPolicy> {
    GLOBAL_PROVIDER_REGISTRY.get(name_or_namespace)
}

/// Helper to check if a provider is allowed for public release bundles.
pub fn is_provider_publicly_redistributable(provider_name: &str) -> bool {
    GLOBAL_PROVIDER_REGISTRY.is_redistributable(provider_name)
}

/// Helper to check if a provider is local-only.
pub fn is_provider_local_only(provider_name: &str) -> bool {
    GLOBAL_PROVIDER_REGISTRY.is_local_only(provider_name)
}

/// Validate that a set of provider names can be legally included in a public release bundle.
/// Returns Ok(()) if all providers are publicly redistributable, or an error detailing the violators.
pub fn validate_bundle_distribution_policy(provider_names: &[String]) -> Result<()> {
    let mut violations = Vec::new();
    for p in provider_names {
        if let Some(policy) = get_provider_policy(p) {
            if !policy.is_safe_for_public_bundle() {
                violations.push(format!(
                    "Provider '{}' (license: '{}', redistribution: '{:?}', is_local_only: {}) cannot be redistributed in public bundles",
                    p, policy.license_id, policy.redistribution, policy.is_local_only
                ));
            }
        } else {
            // Fail closed on unknown providers in public release bundles!
            violations.push(format!(
                "Provider '{}' is unknown in the provider registry; fail-closed policy forbids inclusion in public release bundles",
                p
            ));
        }
    }

    if !violations.is_empty() {
        bail!(
            "Public release bundle policy violation ({} forbidden provider(s)):\n- {}",
            violations.len(),
            violations.join("\n- ")
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_registry_providers() {
        let reg = ProviderRegistry::default_registry();
        assert!(reg.is_redistributable("FAA_CIFP"));
        assert!(reg.is_redistributable("OurAirports"));
        assert!(!reg.is_redistributable("Eurocontrol_EAD"));
        assert!(!reg.is_redistributable("BYOD_AIXM"));
        assert!(!reg.is_redistributable("Navigraph_Forbidden"));
        assert!(reg.is_forbidden("Navigraph_Forbidden"));
    }

    #[test]
    fn test_yaml_roundtrip() {
        let reg = ProviderRegistry::default_registry();
        let yaml = reg.to_yaml().expect("serialize yaml");
        let parsed = ProviderRegistry::from_yaml(&yaml).expect("deserialize yaml");
        assert_eq!(reg.providers.len(), parsed.providers.len());
        assert_eq!(
            reg.get("FAA_CIFP").unwrap().license_id,
            parsed.get("FAA_CIFP").unwrap().license_id
        );
    }

    #[test]
    fn test_validate_bundle_distribution_policy() {
        // Safe providers pass
        assert!(
            validate_bundle_distribution_policy(&[
                "FAA_CIFP".to_string(),
                "OurAirports".to_string()
            ])
            .is_ok()
        );

        // Local-only provider fails
        assert!(
            validate_bundle_distribution_policy(&[
                "FAA_CIFP".to_string(),
                "Eurocontrol_EAD".to_string()
            ])
            .is_err()
        );

        // BYOD provider fails public bundle
        assert!(validate_bundle_distribution_policy(&["BYOD_AIXM".to_string()]).is_err());

        // Forbidden provider fails
        assert!(validate_bundle_distribution_policy(&["Navigraph_Forbidden".to_string()]).is_err());

        // Unknown provider fails closed
        assert!(
            validate_bundle_distribution_policy(&["UnknownPiratedSource".to_string()]).is_err()
        );
    }
}
