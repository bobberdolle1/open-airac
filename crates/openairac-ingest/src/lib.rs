pub mod aixm;
pub mod aixm45;
pub mod caica_procedures;
pub mod caica_rsbn;
pub mod cifp_discovery;
pub mod faa_cifp;
pub mod local_vault;
pub mod ourairports;
pub mod provider;
pub mod registry;
pub mod sia_procedures;
pub mod world_composer;

pub use caica_procedures::{
    CaicaAltitudeConstraint, CaicaParsedProcedure, CaicaProcedureProvider, CaicaRawLegRow,
};
pub use caica_rsbn::{CaicaRsbnProvider, ParsedRsbnStation};
pub use local_vault::{LocalAipVault, VaultEntityCounts, VaultPackageManifest, VaultSourceFile};

use anyhow::Result;
use chrono::{DateTime, Utc};
use openairac_store::WorldStore;
pub use sia_procedures::{SiaParsedProcedure, SiaProcedureProvider, SiaRawLegRow};

/// Automatic version-detecting AIXM ingest helper (supports AIXM 4.5 and AIXM 5.x).
/// Options for version-detecting AIXM ingestion.
pub struct AixmIngestOptions<'a> {
    pub provider_name: &'a str,
    pub namespace: &'a str,
    pub license: &'a str,
    pub effective_from: DateTime<Utc>,
    pub airac_cycle: Option<&'a str>,
    pub source_uri: &'a str,
}

/// Automatic version-detecting AIXM ingest helper (supports AIXM 4.5 and AIXM 5.x).
pub fn ingest_aixm_auto(
    store: &mut WorldStore,
    xml_content: &str,
    opts: &AixmIngestOptions,
) -> Result<provider::IngestReport> {
    if xml_content.contains("<AIXM-Snapshot")
        || xml_content.contains("version=\"4.5\"")
        || xml_content.contains("<Adn>")
        || xml_content.contains("<Adn ")
        || xml_content.contains("<Ahp>")
        || xml_content.contains("<Ahp ")
    {
        let prov = aixm45::Aixm45Provider::new(opts.provider_name, opts.namespace, opts.license);
        prov.ingest_xml_content(
            store,
            xml_content,
            opts.effective_from,
            opts.airac_cycle,
            opts.source_uri,
        )
    } else {
        let prov = aixm::Aixm5Provider::new(opts.provider_name, opts.namespace, opts.license);
        prov.ingest_xml_content(
            store,
            xml_content,
            opts.effective_from,
            opts.airac_cycle,
            opts.source_uri,
        )
    }
}
