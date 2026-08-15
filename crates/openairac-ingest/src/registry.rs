//! Provider manifest registry: the ownership contract between providers,
//! object-id namespaces, and entity tables.
//!
//! The object-id prefix (`<namespace>:` in entity ids) is the ONLY
//! ownership signal in the store. Every provider MUST use its registered
//! namespace; full-snapshot removal (`close_absent_at`) and rollback
//! scoping are derived from provider metadata + `cycle_snapshots ->
//! source_snapshots.provider` + this registry.

/// One published dataset of a provider.
#[derive(Debug, Clone, Copy)]
pub struct DatasetManifest {
    pub name: &'static str,
    /// Entity tables this dataset writes.
    pub entity_tables: &'static [&'static str],
}

/// Static metadata of one provider.
#[derive(Debug, Clone, Copy)]
pub struct ProviderManifest {
    /// Provider string as stored in `source_snapshots.provider`.
    pub name: &'static str,
    /// Object-id namespace prefix (ids are `<namespace>:...`).
    pub namespace: &'static str,
    pub datasets: &'static [DatasetManifest],
}

/// Registry of known providers. Stored-provider strings, CLI keys, and
/// namespaces:
///
/// | snapshot.provider | CLI key      | namespace     |
/// |-------------------|--------------|---------------|
/// | `OurAirports`     | `ourairports`| `ourairports` |
/// | `FAA_CIFP`        | `faa_cifp`   | `faa`         |
pub const PROVIDERS: &[ProviderManifest] = &[
    ProviderManifest {
        name: "OurAirports",
        namespace: "ourairports",
        datasets: &[
            DatasetManifest {
                name: "airports",
                entity_tables: &["airports"],
            },
            DatasetManifest {
                name: "runways",
                entity_tables: &["runways"],
            },
            DatasetManifest {
                name: "navaids",
                entity_tables: &["navaids"],
            },
        ],
    },
    ProviderManifest {
        name: "FAA_CIFP",
        namespace: "faa",
        datasets: &[DatasetManifest {
            name: "FAACIFP18",
            entity_tables: &[
                "airports",
                "runways",
                "waypoints",
                "navaids",
                "airway_legs",
                "procedure_legs",
            ],
        }],
    },
];

/// Manifest by stored provider string (source_snapshots.provider).
pub fn manifest_by_snapshot_provider(provider: &str) -> Option<&'static ProviderManifest> {
    PROVIDERS.iter().find(|m| m.name == provider)
}

/// Namespace prefix by stored provider string.
pub fn namespace_for_snapshot_provider(provider: &str) -> Option<&'static str> {
    manifest_by_snapshot_provider(provider).map(|m| m.namespace)
}

/// Entity tables a provider publishes (union over its datasets).
pub fn entity_tables_for_snapshot_provider(provider: &str) -> Option<Vec<&'static str>> {
    let manifest = manifest_by_snapshot_provider(provider)?;
    let mut tables: Vec<&'static str> = manifest
        .datasets
        .iter()
        .flat_map(|d| d.entity_tables.iter().copied())
        .collect();
    tables.sort_unstable();
    tables.dedup();
    Some(tables)
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
        assert!(!tables.contains(&"source_snapshots"));
    }

    #[test]
    fn test_ourairports_tables_scoped() {
        let tables = entity_tables_for_snapshot_provider("OurAirports").unwrap();
        assert_eq!(tables, vec!["airports", "navaids", "runways"]);
    }
}
