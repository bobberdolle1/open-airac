//! Multi-Identity Aerodrome Model.
//!
//! Decouples physical aerodrome identity from provider-scoped ICAO / IATA / local identifiers.
//!
//! Principles:
//! - Physical reality comes first: one physical runway/terminal complex = one `AerodromeEntityId`.
//! - Geopolitical designations are provider assertions, NOT global primary keys.
//! - Resolves multiple conflicting or historical codes (e.g. URFF / UKFF for Simferopol, URAS / UGSS for Sukhumi)
//!   to the exact same physical facility without duplicate markers or lost navigation data.
//! - Exposes verified simulator aliases for sceneries without corrupting canonical source truth.

use crate::provider::{ProviderId, ProviderProvenance};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// Stable physical aerodrome entity identifier (independent of political / ICAO coding changes).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AerodromeEntityId(pub String);

impl AerodromeEntityId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into().to_ascii_lowercase())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    // Well-known Crimean & Abkhazian physical aerodrome entity identifiers
    pub fn simferopol_intl() -> Self {
        Self("aerodrome_simferopol_intl".to_string())
    }

    pub fn simferopol_zavodskoye() -> Self {
        Self("aerodrome_simferopol_zavodskoye".to_string())
    }

    pub fn sevastopol_belbek() -> Self {
        Self("aerodrome_sevastopol_belbek".to_string())
    }

    pub fn sukhumi_babushara() -> Self {
        Self("aerodrome_sukhumi_babushara".to_string())
    }

    pub fn gudauta() -> Self {
        Self("aerodrome_gudauta_bamboura".to_string())
    }

    pub fn kerch_voykovo() -> Self {
        Self("aerodrome_kerch_voykovo".to_string())
    }
}

impl fmt::Display for AerodromeEntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for AerodromeEntityId {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for AerodromeEntityId {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

/// Type of aerodrome identifier asserted by a provider or source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AerodromeIdentifierType {
    /// Official ICAO location indicator asserted by the active controlling authority.
    IcaoOfficial,
    /// Historical or legacy ICAO indicator previously assigned or asserted by another authority.
    IcaoLegacy,
    /// IATA 3-letter commercial airport code (e.g. SIP, SVO, AER, CDG).
    Iata,
    /// GPS location code in aeronautical GPS databases (e.g. UGSG).
    GpsCode,
    /// National domestic location code (e.g. Russian Cyrillic index УРФФ / УРАС).
    NationalCode,
    /// Flight simulator scenery alias (e.g. legacy add-on scenery bgl/apt.dat code).
    SimulatorAlias,
    /// Community / historical search alias keyword (e.g. UG29 for Sukhumi, UG23 for Gudauta).
    CommunityAlias,
    /// Local AIP / regional civil aviation authority index.
    LocalAipCode,
}
impl AerodromeIdentifierType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::IcaoOfficial => "icao_official",
            Self::IcaoLegacy => "icao_legacy",
            Self::Iata => "iata",
            Self::GpsCode => "gps_code",
            Self::NationalCode => "national_code",
            Self::SimulatorAlias => "simulator_alias",
            Self::CommunityAlias => "community_alias",
            Self::LocalAipCode => "local_aip_code",
        }
    }
}

/// Status of an asserted aerodrome identity within a provider or historical context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AerodromeIdentityStatus {
    /// Currently active and asserted in the provider's active publication.
    CurrentInProvider,
    /// Explicitly not listed in this provider's active location indicator directory.
    NotListedInProvider,
    /// Legacy code supported for backwards compatibility.
    Legacy,
    /// Historical code no longer published by the primary authority.
    Historical,
    /// Simulator scenery compatibility mapping only.
    SimulatorAlias,
    /// Conflicting assertions exist between multiple authoritative providers.
    SourceConflict,
    /// Unverified / research baseline only.
    Unverified,
}

impl AerodromeIdentityStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CurrentInProvider => "current_in_provider",
            Self::NotListedInProvider => "not_listed_in_provider",
            Self::Legacy => "legacy",
            Self::Historical => "historical",
            Self::SimulatorAlias => "simulator_alias",
            Self::SourceConflict => "source_conflict",
            Self::Unverified => "unverified",
        }
    }
}

/// A provider-scoped or historical identifier mapping for a physical aerodrome.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AerodromeIdentity {
    pub entity_id: AerodromeEntityId,
    pub provider_id: ProviderId,
    pub identifier: String,
    pub identifier_type: AerodromeIdentifierType,
    pub status: AerodromeIdentityStatus,
    pub name: String,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_to: Option<DateTime<Utc>>,
    pub source: String,
    pub provenance: Option<ProviderProvenance>,
}

/// Canonical Physical Aerodrome representation with multi-identity support.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhysicalAerodrome {
    pub entity_id: AerodromeEntityId,
    pub canonical_name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub elevation_ft: Option<f64>,
    pub primary_icao: String,
    pub identities: Vec<AerodromeIdentity>,
}

impl PhysicalAerodrome {
    pub fn new(
        entity_id: AerodromeEntityId,
        canonical_name: impl Into<String>,
        primary_icao: impl Into<String>,
        latitude: f64,
        longitude: f64,
    ) -> Self {
        Self {
            entity_id,
            canonical_name: canonical_name.into(),
            primary_icao: primary_icao.into().to_ascii_uppercase(),
            latitude,
            longitude,
            elevation_ft: None,
            identities: Vec::new(),
        }
    }

    pub fn with_elevation(mut self, elevation_ft: f64) -> Self {
        self.elevation_ft = Some(elevation_ft);
        self
    }

    pub fn add_identity(mut self, identity: AerodromeIdentity) -> Self {
        self.identities.push(identity);
        self
    }

    /// Returns the active ICAO indicator for a specific provider, or defaults to primary_icao.
    pub fn get_active_icao(&self, provider_id: Option<&ProviderId>) -> &str {
        if let Some(pid) = provider_id {
            for id in &self.identities {
                if id.provider_id == *pid && id.status == AerodromeIdentityStatus::CurrentInProvider
                {
                    return &id.identifier;
                }
            }
        }
        &self.primary_icao
    }

    /// Check if this physical aerodrome matches any alias or identifier.
    pub fn matches_query(&self, query: &str) -> bool {
        let q = query.trim().to_ascii_uppercase();
        if self.primary_icao.eq_ignore_ascii_case(&q) {
            return true;
        }
        if self.entity_id.as_str().eq_ignore_ascii_case(&q) {
            return true;
        }
        if self.canonical_name.to_ascii_uppercase().contains(&q) {
            return true;
        }
        self.identities.iter().any(|id| {
            id.identifier.eq_ignore_ascii_case(&q) || id.name.to_ascii_uppercase().contains(&q)
        })
    }
}

/// Central Multi-Identity Registry for Physical Aerodromes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MultiIdentityRegistry {
    pub aerodromes: BTreeMap<AerodromeEntityId, PhysicalAerodrome>,
}

impl MultiIdentityRegistry {
    pub fn new() -> Self {
        Self {
            aerodromes: BTreeMap::new(),
        }
    }

    pub fn register(&mut self, aerodrome: PhysicalAerodrome) {
        self.aerodromes
            .insert(aerodrome.entity_id.clone(), aerodrome);
    }

    pub fn resolve(&self, ident: &str) -> Option<&PhysicalAerodrome> {
        let q = ident.trim().to_ascii_uppercase();
        self.aerodromes.values().find(|a| a.matches_query(&q))
    }

    /// Default authoritative multi-identity registry containing Crimea & Abkhazia physical aerodromes.
    pub fn default_registry() -> Self {
        let mut reg = Self::new();

        // 1. Simferopol International Airport (URFF / UKFF / SIP)
        let simferopol = PhysicalAerodrome::new(
            AerodromeEntityId::simferopol_intl(),
            "Simferopol International Airport",
            "URFF",
            45.0522,
            33.9750,
        )
        .with_elevation(639.0)
        .add_identity(AerodromeIdentity {
            entity_id: AerodromeEntityId::simferopol_intl(),
            provider_id: ProviderId::caica_russia(),
            identifier: "URFF".to_string(),
            identifier_type: AerodromeIdentifierType::IcaoOfficial,
            status: AerodromeIdentityStatus::CurrentInProvider,
            name: "Симферополь (Simferopol)".to_string(),
            valid_from: None,
            valid_to: None,
            source: "CAICA AIP / Aeronautical Information Manual (AIRAC 2608)".to_string(),
            provenance: Some(
                ProviderProvenance::new(ProviderId::caica_russia(), "AIRAC 2608")
                    .with_source_file("CAICA_AIP_URFF.html"),
            ),
        })
        .add_identity(AerodromeIdentity {
            entity_id: AerodromeEntityId::simferopol_intl(),
            provider_id: ProviderId::ourairports(),
            identifier: "UKFF".to_string(),
            identifier_type: AerodromeIdentifierType::IcaoLegacy,
            status: AerodromeIdentityStatus::Legacy,
            name: "Simferopol International Airport".to_string(),
            valid_from: None,
            valid_to: None,
            source: "OurAirports / Historical International Indicator".to_string(),
            provenance: Some(
                ProviderProvenance::new(ProviderId::ourairports(), "2026-08-20")
                    .with_source_file("airports.csv"),
            ),
        })
        .add_identity(AerodromeIdentity {
            entity_id: AerodromeEntityId::simferopol_intl(),
            provider_id: ProviderId::new("iata"),
            identifier: "SIP".to_string(),
            identifier_type: AerodromeIdentifierType::Iata,
            status: AerodromeIdentityStatus::CurrentInProvider,
            name: "Simferopol".to_string(),
            valid_from: None,
            valid_to: None,
            source: "IATA Airline Coding Directory".to_string(),
            provenance: None,
        });
        reg.register(simferopol);

        // 2. Simferopol Zavodskoye Airfield (URFW)
        let zavodskoye = PhysicalAerodrome::new(
            AerodromeEntityId::simferopol_zavodskoye(),
            "Simferopol Zavodskoye Airfield",
            "URFW",
            44.9272,
            34.0650,
        )
        .with_elevation(950.0)
        .add_identity(AerodromeIdentity {
            entity_id: AerodromeEntityId::simferopol_zavodskoye(),
            provider_id: ProviderId::caica_russia(),
            identifier: "URFW".to_string(),
            identifier_type: AerodromeIdentifierType::IcaoOfficial,
            status: AerodromeIdentityStatus::CurrentInProvider,
            name: "Заводское (Zavodskoye)".to_string(),
            valid_from: None,
            valid_to: None,
            source: "CAICA AIP / General Aviation Airfield Index".to_string(),
            provenance: None,
        });
        reg.register(zavodskoye);

        // 3. Sevastopol Belbek (URFB / UKFB)
        let belbek = PhysicalAerodrome::new(
            AerodromeEntityId::sevastopol_belbek(),
            "Sevastopol Belbek Airport",
            "URFB",
            44.6914,
            33.5744,
        )
        .with_elevation(344.0)
        .add_identity(AerodromeIdentity {
            entity_id: AerodromeEntityId::sevastopol_belbek(),
            provider_id: ProviderId::caica_russia(),
            identifier: "URFB".to_string(),
            identifier_type: AerodromeIdentifierType::IcaoOfficial,
            status: AerodromeIdentityStatus::CurrentInProvider,
            name: "Севастополь / Бельбек (Belbek)".to_string(),
            valid_from: None,
            valid_to: None,
            source: "CAICA AIP Joint Civil/Military Index".to_string(),
            provenance: None,
        })
        .add_identity(AerodromeIdentity {
            entity_id: AerodromeEntityId::sevastopol_belbek(),
            provider_id: ProviderId::ourairports(),
            identifier: "UKFB".to_string(),
            identifier_type: AerodromeIdentifierType::IcaoLegacy,
            status: AerodromeIdentityStatus::Legacy,
            name: "Sevastopol Belbek".to_string(),
            valid_from: None,
            valid_to: None,
            source: "OurAirports".to_string(),
            provenance: None,
        });
        reg.register(belbek);

        // 4. Sukhumi Babushara Airport (URAS / UGSS / SUI)
        let sukhumi = PhysicalAerodrome::new(
            AerodromeEntityId::sukhumi_babushara(),
            "Sukhumi Babushara Airport (Vladislav Ardzinba)",
            "URAS",
            42.8581,
            41.1281,
        )
        .with_elevation(52.0)
        .add_identity(AerodromeIdentity {
            entity_id: AerodromeEntityId::sukhumi_babushara(),
            provider_id: ProviderId::caica_russia(),
            identifier: "URAS".to_string(),
            identifier_type: AerodromeIdentifierType::IcaoOfficial,
            status: AerodromeIdentityStatus::CurrentInProvider,
            name: "Сухум / Бабушара (Sukhum Babushara)".to_string(),
            valid_from: None,
            valid_to: None,
            source: "CAICA Aeronautical Information Manual No.12 / Rosaviatsiya".to_string(),
            provenance: Some(
                ProviderProvenance::new(ProviderId::caica_russia(), "AIRAC 2608")
                    .with_source_file("CAICA_AIM_12_URAS.pdf"),
            ),
        })
        .add_identity(AerodromeIdentity {
            entity_id: AerodromeEntityId::sukhumi_babushara(),
            provider_id: ProviderId::ourairports(),
            identifier: "UGSS".to_string(),
            identifier_type: AerodromeIdentifierType::IcaoLegacy,
            status: AerodromeIdentityStatus::CurrentInProvider,
            name: "Sukhumi Babushara Airport".to_string(),
            valid_from: None,
            valid_to: None,
            source: "OurAirports Baseline Dataset (Current in OurAirports)".to_string(),
            provenance: None,
        })
        .add_identity(AerodromeIdentity {
            entity_id: AerodromeEntityId::sukhumi_babushara(),
            provider_id: ProviderId::new("georgia_ais"),
            identifier: "UGSS".to_string(),
            identifier_type: AerodromeIdentifierType::IcaoLegacy,
            status: AerodromeIdentityStatus::NotListedInProvider,
            name: "Sukhumi (Not listed in active 2026 Georgian AIP GEN 2.4)".to_string(),
            valid_from: None,
            valid_to: None,
            source: "Georgian AIP GEN 2.4 Location Indicators".to_string(),
            provenance: None,
        })
        .add_identity(AerodromeIdentity {
            entity_id: AerodromeEntityId::sukhumi_babushara(),
            provider_id: ProviderId::new("iata"),
            identifier: "SUI".to_string(),
            identifier_type: AerodromeIdentifierType::Iata,
            status: AerodromeIdentityStatus::CurrentInProvider,
            name: "Sukhumi".to_string(),
            valid_from: None,
            valid_to: None,
            source: "IATA".to_string(),
            provenance: None,
        })
        .add_identity(AerodromeIdentity {
            entity_id: AerodromeEntityId::sukhumi_babushara(),
            provider_id: ProviderId::ourairports(),
            identifier: "UG29".to_string(),
            identifier_type: AerodromeIdentifierType::CommunityAlias,
            status: AerodromeIdentityStatus::Historical,
            name: "Sukhumi Historical Keyword / Secondary Code".to_string(),
            valid_from: None,
            valid_to: None,
            source: "OurAirports Secondary Identifier Mapping".to_string(),
            provenance: None,
        });
        reg.register(sukhumi);
        // 5. Gudauta (Bombora Air Base / UGSG / UG23)
        let gudauta = PhysicalAerodrome::new(
            AerodromeEntityId::gudauta(),
            "Gudauta Bombora Air Base",
            "UGSG",
            43.1033,
            40.5800,
        )
        .with_elevation(82.0)
        .add_identity(AerodromeIdentity {
            entity_id: AerodromeEntityId::gudauta(),
            provider_id: ProviderId::ourairports(),
            identifier: "UGSG".to_string(),
            identifier_type: AerodromeIdentifierType::GpsCode,
            status: AerodromeIdentityStatus::CurrentInProvider,
            name: "Gudauta Bombora Airfield".to_string(),
            valid_from: None,
            valid_to: None,
            source: "OurAirports GPS Identifier".to_string(),
            provenance: None,
        })
        .add_identity(AerodromeIdentity {
            entity_id: AerodromeEntityId::gudauta(),
            provider_id: ProviderId::ourairports(),
            identifier: "UG23".to_string(),
            identifier_type: AerodromeIdentifierType::CommunityAlias,
            status: AerodromeIdentityStatus::Historical,
            name: "Gudauta Historical Keyword".to_string(),
            valid_from: None,
            valid_to: None,
            source: "OurAirports Historical Keyword".to_string(),
            provenance: None,
        });
        reg.register(gudauta);

        // 6. Kerch Voykovo Airfield
        let kerch = PhysicalAerodrome::new(
            AerodromeEntityId::kerch_voykovo(),
            "Kerch Voykovo Airfield",
            "URFK",
            45.3711,
            36.4014,
        )
        .with_elevation(170.0)
        .add_identity(AerodromeIdentity {
            entity_id: AerodromeEntityId::kerch_voykovo(),
            provider_id: ProviderId::caica_russia(),
            identifier: "URFK".to_string(),
            identifier_type: AerodromeIdentifierType::IcaoOfficial,
            status: AerodromeIdentityStatus::CurrentInProvider,
            name: "Керчь (Kerch)".to_string(),
            valid_from: None,
            valid_to: None,
            source: "CAICA Regional Airfield Directory".to_string(),
            provenance: None,
        });
        reg.register(kerch);

        // 7. Bolshiye Shiraki (Kakheti, Georgia / UG28)
        let shiraki = PhysicalAerodrome::new(
            AerodromeEntityId::new("aerodrome_bolshiye_shiraki"),
            "Bolshiye Shiraki Air Base",
            "UG28",
            41.3800,
            46.3600,
        )
        .with_elevation(1640.0)
        .add_identity(AerodromeIdentity {
            entity_id: AerodromeEntityId::new("aerodrome_bolshiye_shiraki"),
            provider_id: ProviderId::ourairports(),
            identifier: "UG28".to_string(),
            identifier_type: AerodromeIdentifierType::IcaoLegacy,
            status: AerodromeIdentityStatus::Historical,
            name: "Bolshiye Shiraki Airfield (Kakheti)".to_string(),
            valid_from: None,
            valid_to: None,
            source: "OurAirports / Historical Military Airfield Record".to_string(),
            provenance: None,
        });
        reg.register(shiraki);

        // 8. Pskhu Mountain Airfield (GE-0015)
        let pskhu = PhysicalAerodrome::new(
            AerodromeEntityId::new("aerodrome_pskhu"),
            "Pskhu Mountain Airfield",
            "GE-0015",
            43.3764,
            40.8019,
        )
        .with_elevation(2040.0)
        .add_identity(AerodromeIdentity {
            entity_id: AerodromeEntityId::new("aerodrome_pskhu"),
            provider_id: ProviderId::ourairports(),
            identifier: "GE-0015".to_string(),
            identifier_type: AerodromeIdentifierType::LocalAipCode,
            status: AerodromeIdentityStatus::CurrentInProvider,
            name: "Pskhu Mountain Landing Site".to_string(),
            valid_from: None,
            valid_to: None,
            source: "OurAirports Baseline Identifier GE-0015".to_string(),
            provenance: None,
        });
        reg.register(pskhu);
        reg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multi_identity_resolution() {
        let reg = MultiIdentityRegistry::default_registry();

        // 1. Query Simferopol by URFF or UKFF -> Resolves to the same physical entity
        let simf1 = reg.resolve("URFF").expect("resolve URFF");
        let simf2 = reg.resolve("UKFF").expect("resolve UKFF");
        let simf3 = reg.resolve("SIP").expect("resolve SIP");
        assert_eq!(simf1.entity_id, simf2.entity_id);
        assert_eq!(simf1.entity_id, simf3.entity_id);
        assert_eq!(simf1.entity_id, AerodromeEntityId::simferopol_intl());

        // 2. Query Sukhumi by URAS, UGSS, SUI, or historical keyword UG29 -> Resolves to the same physical entity
        let sukhum1 = reg.resolve("URAS").expect("resolve URAS");
        let sukhum2 = reg.resolve("UGSS").expect("resolve UGSS");
        let sukhum3 = reg.resolve("SUI").expect("resolve SUI");
        let sukhum4 = reg.resolve("UG29").expect("resolve UG29");
        assert_eq!(sukhum1.entity_id, sukhum2.entity_id);
        assert_eq!(sukhum1.entity_id, sukhum3.entity_id);
        assert_eq!(sukhum1.entity_id, sukhum4.entity_id);
        assert_eq!(sukhum1.entity_id, AerodromeEntityId::sukhumi_babushara());

        // 3. Collision Prevention: UG29 must NOT resolve to Gudauta
        let gudauta = reg.resolve("UGSG").expect("resolve UGSG");
        assert_eq!(gudauta.entity_id, AerodromeEntityId::gudauta());
        assert_ne!(gudauta.entity_id, sukhum4.entity_id);

        // 4. Collision Prevention: UG28 must NOT resolve to Pskhu or Gudauta (resolves to Bolshiye Shiraki)
        let shiraki = reg.resolve("UG28").expect("resolve UG28");
        assert_eq!(shiraki.entity_id.as_str(), "aerodrome_bolshiye_shiraki");
        assert_ne!(shiraki.entity_id, gudauta.entity_id);
        assert_ne!(shiraki.entity_id, sukhum1.entity_id);

        // 5. Ensure Zavodskoye (URFW) is distinct from Simferopol Intl (URFF)
        let zavodskoye = reg.resolve("URFW").expect("resolve URFW");
        assert_ne!(simf1.entity_id, zavodskoye.entity_id);
    }
}
