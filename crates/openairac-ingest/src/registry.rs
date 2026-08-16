//! Provider manifest registry: re-exported from the canonical model, so
//! the store (rollback scoping) and the ingest layer share ONE ownership
//! contract. The object-id prefix (`<namespace>:` in entity ids) is the
//! ONLY ownership signal in the store.

pub use openairac_model::{
    DatasetManifest, PROVIDER_MANIFESTS, ProviderManifest, manifest_for_provider,
    namespace_for_provider, tables_for_provider,
};

/// Manifest by stored provider string (source_snapshots.provider).
pub fn manifest_by_snapshot_provider(provider: &str) -> Option<&'static ProviderManifest> {
    manifest_for_provider(provider)
}

/// Namespace prefix by stored provider string.
pub fn namespace_for_snapshot_provider(provider: &str) -> Option<&'static str> {
    namespace_for_provider(provider)
}

/// Entity tables a provider publishes (union over its datasets).
pub fn entity_tables_for_snapshot_provider(provider: &str) -> Option<Vec<&'static str>> {
    tables_for_provider(provider)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_mapping() {
        assert_eq!(namespace_for_snapshot_provider("FAA_CIFP"), Some("faa"));
        assert_eq!(
            namespace_for_snapshot_provider("OurAirports"),
            Some("ourairports")
        );
        assert_eq!(namespace_for_snapshot_provider("Unknown"), None);
    }

    #[test]
    fn test_cifp_tables_include_procedure_legs() {
        let tables = entity_tables_for_snapshot_provider("FAA_CIFP").unwrap();
        assert!(tables.contains(&"procedure_legs"));
        assert!(tables.contains(&"airway_legs"));
        assert!(tables.contains(&"waypoints"));
        assert!(!tables.contains(&"source_snapshots"));
        assert!(tables.contains(&"airports"));
        assert!(tables.contains(&"runways"));
    }

    #[test]
    fn test_ourairports_tables_scoped() {
        let tables = entity_tables_for_snapshot_provider("OurAirports").unwrap();
        assert_eq!(tables, vec!["airports", "navaids", "runways"]);
    }
}
