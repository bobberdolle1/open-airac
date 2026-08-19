//! X-Plane format adapter: the generic FormatExporter/TargetInstaller
//! implemented on top of the existing `openairac-export-xplane`
//! primitives (export_from_db, install_layer, resolve_sim_world).
//!
//! One exporter serves both X-Plane 12 and X-Plane 11 targets (same
//! format family; only the descriptor differs).

use super::*;
use anyhow::Context;

/// FormatExporter for the X-Plane dat family (XPNAV1200/XPFIX1200/
/// XPAWY1101). One instance serves all X-Plane targets.
pub struct XPlaneDatExporter;

impl FormatExporter for XPlaneDatExporter {
    fn family(&self) -> FormatFamilyId {
        families::xplane_dat()
    }

    fn export(
        &self,
        store: &WorldStore,
        as_of: DateTime<Utc>,
        out_dir: &Path,
    ) -> Result<GeneratedArtifactSet> {
        std::fs::create_dir_all(out_dir).with_context(|| format!("creating {:?}", out_dir))?;
        let report =
            openairac_export_xplane::XPlane12Exporter::export_from_db(store, as_of, out_dir, true)?;
        let cycle = openairac_export_xplane::airac_cycle(as_of);
        let manifest_path = out_dir.join("manifest.json");
        let manifest_json = std::fs::read_to_string(&manifest_path)
            .with_context(|| format!("reading {}", manifest_path.display()))?;
        let manifest: openairac_export_xplane::NavdataLayerManifest =
            serde_json::from_str(&manifest_json).context("parsing layer manifest")?;
        let world_fingerprint = manifest.world_fingerprint.unwrap_or_default();
        let mut artifacts = Vec::new();
        for entry in &manifest.files {
            let path = out_dir.join(&entry.name);
            let meta = std::fs::metadata(&path)?;
            artifacts.push(ArtifactEntry {
                path: entry.name.clone(),
                sha256: entry.sha256.clone(),
                size: meta.len(),
                kind: "navdata-layer".to_string(),
            });
        }
        let _ = report;
        Ok(GeneratedArtifactSet {
            family: self.family(),
            cycle,
            as_of: as_of.to_rfc3339(),
            generator: manifest.generator,
            world_fingerprint,
            artifacts,
        })
    }
}

/// Transactional installer for any X-Plane Custom Data target. The
/// descriptor supplies the layer file list; the implementation is the
/// existing journaled installer.
pub struct XPlaneTargetInstaller {
    descriptor: TargetDescriptor,
}

impl XPlaneTargetInstaller {
    pub fn new(descriptor: TargetDescriptor) -> Self {
        Self { descriptor }
    }
}

impl TargetInstaller for XPlaneTargetInstaller {
    fn descriptor(&self) -> &TargetDescriptor {
        &self.descriptor
    }

    fn install(
        &self,
        artifacts_root: &Path,
        artifacts: &GeneratedArtifactSet,
        target_root: &Path,
    ) -> Result<TargetInstallReport> {
        // Artifact set must verify before anything touches the target.
        artifacts.verify(artifacts_root)?;
        // The shared journaled installer needs a staging dir shaped
        // like the export output (dat files + manifest.json).
        let report = openairac_export_xplane::install_layer(artifacts_root, target_root)?;
        Ok(TargetInstallReport {
            target_id: self.descriptor.id.clone(),
            operation_id: report.operation_id,
            cycle: report.cycle,
            installed: report.installed,
            restored: report.restored,
            removed: report.removed,
        })
    }

    fn rollback(&self, target_root: &Path) -> Result<TargetInstallReport> {
        let report =
            openairac_export_xplane::recover_interrupted(target_root)?.unwrap_or_else(|| {
                // No journal: nothing to roll back. Return an
                // empty report rather than failing - idempotent.
                openairac_export_xplane::LayerInstallReport {
                    operation_id: "none".to_string(),
                    cycle: String::new(),
                    installed: Vec::new(),
                    restored: Vec::new(),
                    removed: Vec::new(),
                }
            });
        Ok(TargetInstallReport {
            target_id: self.descriptor.id.clone(),
            operation_id: report.operation_id,
            cycle: report.cycle,
            installed: report.installed,
            restored: report.restored,
            removed: report.removed,
        })
    }

    fn recover(&self, target_root: &Path) -> Result<Option<TargetInstallReport>> {
        Ok(
            openairac_export_xplane::recover_interrupted(target_root)?.map(|r| {
                TargetInstallReport {
                    target_id: self.descriptor.id.clone(),
                    operation_id: r.operation_id,
                    cycle: r.cycle,
                    installed: r.installed,
                    restored: r.restored,
                    removed: r.removed,
                }
            }),
        )
    }
}

/// Resolve an installed X-Plane layer's identity/consistency.
pub fn resolve_xplane_target(
    target_root: &Path,
) -> Result<openairac_export_xplane::SimWorldReport> {
    openairac_export_xplane::resolve_sim_world(target_root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use openairac_model::{
        AirportId, CanonicalAirport, SourceSnapshot, SourceSnapshotId, TemporalValidity,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn unique_dir(tag: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("oa_export_test_{}_{}_{n}", std::process::id(), tag))
    }

    fn fixture_store() -> (WorldStore, PathBuf) {
        let dir = unique_dir("fixture");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = WorldStore::open(dir.join("src.sqlite")).unwrap();
        let t = Utc::now();
        store
            .insert_source_snapshot(&SourceSnapshot {
                id: SourceSnapshotId("snap-1".to_string()),
                provider: "OurAirports".to_string(),
                dataset: "airports".to_string(),
                provider_revision: None,
                airac_cycle: None,
                effective_from: Some(t),
                effective_until: None,
                retrieved_at: t,
                source_uri: "fixture".to_string(),
                content_sha256: "0".repeat(64),
                license_id: None,
                license_notes: None,
                parser_version: "test".to_string(),
            })
            .unwrap();
        store
            .insert_airport(&CanonicalAirport {
                id: AirportId("ourairports:1".to_string()),
                ident: "KSFO".to_string(),
                name: "San Francisco".to_string(),
                airport_type: "large_airport".to_string(),
                latitude: 37.6188,
                longitude: -122.375,
                elevation_ft: Some(13.0),
                iso_country: Some("US".to_string()),
                municipality: None,
                runways: Vec::new(),
                temporal: TemporalValidity {
                    valid_from: t,
                    valid_until: None,
                    source_snapshot_id: SourceSnapshotId("snap-1".to_string()),
                },
            })
            .unwrap();
        (store, dir)
    }

    #[test]
    fn test_xplane_exporter_generates_artifact_set() {
        let (store, dir) = fixture_store();
        let out = dir.join("layer");
        let set = XPlaneDatExporter.export(&store, Utc::now(), &out).unwrap();
        assert_eq!(set.family.as_str(), "xplane-dat");
        assert_eq!(set.artifacts.len(), 3);
        let names: Vec<&str> = set.artifacts.iter().map(|a| a.path.as_str()).collect();
        assert!(names.contains(&"earth_fix.dat"));
        assert!(names.contains(&"earth_nav.dat"));
        assert!(names.contains(&"earth_awy.dat"));
        set.verify(&out).unwrap();
        // Tamper -> verify fails.
        std::fs::write(out.join("earth_nav.dat"), "tampered\n").unwrap();
        assert!(set.verify(&out).is_err());
    }

    #[test]
    fn test_xplane_installer_transactional() {
        let (store, dir) = fixture_store();
        let out = dir.join("layer");
        let set = XPlaneDatExporter.export(&store, Utc::now(), &out).unwrap();
        let target = target("xplane12").unwrap().clone();
        let installer = XPlaneTargetInstaller::new(target.clone());
        let root = dir.join("custom_data");
        let report = installer.install(&out, &set, &root).unwrap();
        assert_eq!(report.installed.len(), 4); // 3 dat files + identity
        let resolved = resolve_xplane_target(&root).unwrap();
        assert_eq!(
            resolved.verdict,
            openairac_export_xplane::SimWorldVerdict::Consistent
        );
    }
}
