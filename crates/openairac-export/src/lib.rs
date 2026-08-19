//! Generic export/target architecture.
//!
//! The canonical aviation model never changes for a simulator. The
//! pipeline is:
//!
//! ```text
//! WorldStore -> FormatExporter -> GeneratedArtifactSet
//!             -> TargetDescriptor -> transactional TargetInstaller
//! ```
//!
//! One exporter serves many targets (same format family, different
//! install roots); installers are declarative where practical and all
//! reuse the shared transactional primitives (staging, backup,
//! journal, swap, post-validation, rollback, recovery).

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use openairac_store::WorldStore;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Format families
// ---------------------------------------------------------------------------

/// Stable identifier of a distribution format family. Format families
/// are independent of target products: the same family may serve many
/// targets.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FormatFamilyId(pub String);

impl FormatFamilyId {
    pub fn new(id: &str) -> Self {
        Self(id.to_string())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Known format families. Registering a family here does NOT claim any
/// target is supported; support is declared per target descriptor.
pub mod families {
    use super::FormatFamilyId;

    /// X-Plane XPNAV1200/XPFIX1200/XPAWY1101 dat files (Custom Data).
    pub fn xplane_dat() -> FormatFamilyId {
        FormatFamilyId::new("xplane-dat")
    }
    /// X-Plane CIFP1250 per-airport terminal procedures (converter output).
    pub fn xplane_cifp() -> FormatFamilyId {
        FormatFamilyId::new("xplane-cifp1250")
    }
    /// MSFS SimpleNavData-style BGL navdata package.
    pub fn msfs_bgl() -> FormatFamilyId {
        FormatFamilyId::new("msfs-bgl")
    }
    /// Little Navmap SQLite nav database.
    pub fn lnm_sqlite() -> FormatFamilyId {
        FormatFamilyId::new("little-navmap-sqlite")
    }
    /// Aerosoft / NavDataPro text.
    pub fn navdatapro_text() -> FormatFamilyId {
        FormatFamilyId::new("navdatapro-text")
    }
    /// PMDG classic text.
    pub fn pmdg_text() -> FormatFamilyId {
        FormatFamilyId::new("pmdg-text")
    }
}

// ---------------------------------------------------------------------------
// Generated artifacts
// ---------------------------------------------------------------------------

/// One generated file inside an artifact set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactEntry {
    /// Relative path inside the artifact set directory.
    pub path: String,
    pub sha256: String,
    pub size: u64,
    /// Semantics: "navdata-layer", "cycle-metadata", "manifest".
    pub kind: String,
}

/// What one FormatExporter produced: a directory of files plus the
/// metadata installers need (cycle, fingerprint, provenance).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedArtifactSet {
    pub family: FormatFamilyId,
    pub cycle: String,
    /// Effective instant the export was generated for.
    pub as_of: String,
    pub generator: String,
    /// Deterministic world fingerprint (content identity).
    pub world_fingerprint: String,
    pub artifacts: Vec<ArtifactEntry>,
}

impl GeneratedArtifactSet {
    /// Recompute every artifact sha256 from disk and fail on mismatch.
    pub fn verify(&self, root: &Path) -> Result<()> {
        for entry in &self.artifacts {
            let p = root.join(&entry.path);
            let data =
                std::fs::read(&p).with_context(|| format!("reading artifact {}", entry.path))?;
            use sha2::Digest;
            let actual = format!("{:x}", sha2::Sha256::digest(&data));
            if actual != entry.sha256 {
                anyhow::bail!("artifact {} checksum mismatch", entry.path);
            }
            let meta = std::fs::metadata(&p)?;
            if meta.len() != entry.size {
                anyhow::bail!(
                    "artifact {} size mismatch: manifest {}, actual {}",
                    entry.path,
                    entry.size,
                    meta.len()
                );
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Exporter
// ---------------------------------------------------------------------------

/// A serializer from the canonical world into one format family.
pub trait FormatExporter {
    fn family(&self) -> FormatFamilyId;

    /// Export the world at `as_of` into `out_dir`. Returns the
    /// artifact set description (relative to `out_dir`).
    fn export(
        &self,
        store: &WorldStore,
        as_of: DateTime<Utc>,
        out_dir: &Path,
    ) -> Result<GeneratedArtifactSet>;
}

// ---------------------------------------------------------------------------
// Target descriptors
// ---------------------------------------------------------------------------

/// Honest support state. A target is SUPPORTED only when export,
/// install, post-install validation, and rollback all pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SupportState {
    Supported,
    Experimental,
    Research,
    Unsupported,
}

impl SupportState {
    pub fn as_str(&self) -> &'static str {
        match self {
            SupportState::Supported => "SUPPORTED",
            SupportState::Experimental => "EXPERIMENTAL",
            SupportState::Research => "RESEARCH",
            SupportState::Unsupported => "UNSUPPORTED",
        }
    }
}

/// How the installer places artifacts on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstallStrategy {
    /// Copy the layer files into the target root (X-Plane Custom Data).
    CustomData {
        /// Artifact paths that form the layer (installed into the root).
        layer_files: Vec<String>,
        /// Identity file installed alongside (resolver marker).
        identity_file: String,
    },
    /// Place the whole artifact set into a subdirectory of the target
    /// root (packages, Little Navmap, etc.).
    Subdirectory { relative: String },
}

/// How post-install validation is performed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValidationStrategy {
    /// Re-hash installed artifacts against the artifact set.
    HashVerify,
    /// Hash verify + call the format-specific semantic checker.
    HashAndSemantic,
}

/// A detection rule for discovering a simulator installation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DetectionRule {
    /// The install root exists and contains this relative entry.
    DirContains(String),
    /// The install root exists and contains a file matching this glob.
    FileGlob(String),
}

/// One candidate install root (platform-independent list; the first
/// existing candidate is used, operator override always wins).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallRoot {
    pub platform: String,
    pub path: String,
    /// Relative subdirectory inside the root where artifacts go.
    pub subdir: String,
}

/// A version constraint on the target product.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionConstraint {
    pub min: Option<String>,
    pub max: Option<String>,
    pub note: String,
}

/// Declarative description of one installable target product.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetDescriptor {
    pub id: String,
    pub display_name: String,
    pub simulator: String,
    pub format_family: FormatFamilyId,
    pub detection_rules: Vec<DetectionRule>,
    pub install_roots: Vec<InstallRoot>,
    pub required_artifacts: Vec<String>,
    pub optional_artifacts: Vec<String>,
    pub version_constraints: Vec<VersionConstraint>,
    pub install_strategy: InstallStrategy,
    pub validation_strategy: ValidationStrategy,
    pub support_state: SupportState,
}

/// Detect the first install root that exists (in declaration order).
pub fn detect_install_root(target: &TargetDescriptor) -> Option<PathBuf> {
    for root in &target.install_roots {
        let candidate = PathBuf::from(&root.path).join(&root.subdir);
        if candidate.is_dir() {
            let ok = target.detection_rules.iter().all(|rule| match rule {
                DetectionRule::DirContains(rel) => candidate.join(rel).exists(),
                DetectionRule::FileGlob(_) => true, // globs checked by callers with dir listings
            });
            if ok {
                return Some(candidate);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Installer
// ---------------------------------------------------------------------------

/// Outcome of a transactional target install.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetInstallReport {
    pub target_id: String,
    pub operation_id: String,
    pub cycle: String,
    pub installed: Vec<String>,
    pub restored: Vec<String>,
    pub removed: Vec<String>,
}

/// Transactional installer for one target product. Implementations
/// MUST reuse the shared transactional primitives (staging, backup,
/// journal, swap, post-validation, rollback, recovery).
pub trait TargetInstaller {
    fn descriptor(&self) -> &TargetDescriptor;

    /// Install `artifacts` (located under `artifacts_root`) into
    /// `target_root` transactionally. Fails closed; a failure leaves
    /// the previous state byte-identical.
    fn install(
        &self,
        artifacts_root: &Path,
        artifacts: &GeneratedArtifactSet,
        target_root: &Path,
    ) -> Result<TargetInstallReport>;

    /// Roll back the last install (journal-driven).
    fn rollback(&self, target_root: &Path) -> Result<TargetInstallReport>;

    /// Recover from an interrupted install (crash/journal).
    fn recover(&self, target_root: &Path) -> Result<Option<TargetInstallReport>>;
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// All registered target descriptors (honest support states).
pub fn target_registry() -> &'static [TargetDescriptor] {
    registry::targets()
}

/// Find a target descriptor by id.
pub fn target(id: &str) -> Option<&'static TargetDescriptor> {
    target_registry().iter().find(|t| t.id == id)
}

pub mod xplane_adapter;
pub use xplane_adapter::{XPlaneDatExporter, XPlaneTargetInstaller, resolve_xplane_target};

pub mod transactional;
pub use transactional::{
    FileInstallReport, InstallPhase, install_files_transactionally, recover_file_install,
};

pub mod registry {
    use super::*;

    static TARGETS: std::sync::OnceLock<Vec<TargetDescriptor>> = std::sync::OnceLock::new();

    pub fn targets() -> &'static [TargetDescriptor] {
        TARGETS.get_or_init(|| {
            let v: Vec<TargetDescriptor> = vec![
                TargetDescriptor {
                    id: "xplane12".to_string(),
                    display_name: "X-Plane 12".to_string(),
                    simulator: "X-Plane".to_string(),
                    format_family: families::xplane_dat(),
                    detection_rules: vec![DetectionRule::DirContains("Resources".to_string())],
                    install_roots: vec![InstallRoot {
                        platform: "any".to_string(),
                        path: "%XPLANE%".to_string(),
                        subdir: "Custom Data".to_string(),
                    }],
                    required_artifacts: vec![
                        "earth_fix.dat".to_string(),
                        "earth_nav.dat".to_string(),
                        "earth_awy.dat".to_string(),
                    ],
                    optional_artifacts: vec![],
                    version_constraints: vec![VersionConstraint {
                        min: Some("12.0".to_string()),
                        max: None,
                        note: "XPNAV1200/XPFIX1200/XPAWY1101; verified vs convert424toxplane v12.4"
                            .to_string(),
                    }],
                    install_strategy: InstallStrategy::CustomData {
                        layer_files: vec![
                            "earth_fix.dat".to_string(),
                            "earth_nav.dat".to_string(),
                            "earth_awy.dat".to_string(),
                        ],
                        identity_file: "openairac_layer.json".to_string(),
                    },
                    validation_strategy: ValidationStrategy::HashAndSemantic,
                    support_state: SupportState::Supported,
                },
                TargetDescriptor {
                    id: "xplane11".to_string(),
                    display_name: "X-Plane 11".to_string(),
                    simulator: "X-Plane".to_string(),
                    format_family: families::xplane_dat(),
                    detection_rules: vec![DetectionRule::DirContains("Resources".to_string())],
                    install_roots: vec![InstallRoot {
                        platform: "any".to_string(),
                        path: "%XPLANE%".to_string(),
                        subdir: "Custom Data".to_string(),
                    }],
                    required_artifacts: vec![
                        "earth_fix.dat".to_string(),
                        "earth_nav.dat".to_string(),
                        "earth_awy.dat".to_string(),
                    ],
                    optional_artifacts: vec![],
                    version_constraints: vec![VersionConstraint {
                        min: Some("11.30".to_string()),
                        max: None,
                        note: "Same format family as X-Plane 12; XP11 install path untested in CI"
                            .to_string(),
                    }],
                    install_strategy: InstallStrategy::CustomData {
                        layer_files: vec![
                            "earth_fix.dat".to_string(),
                            "earth_nav.dat".to_string(),
                            "earth_awy.dat".to_string(),
                        ],
                        identity_file: "openairac_layer.json".to_string(),
                    },
                    validation_strategy: ValidationStrategy::HashVerify,
                    support_state: SupportState::Experimental,
                },
                TargetDescriptor {
                    id: "msfs2024".to_string(),
                    display_name: "Microsoft Flight Simulator 2024".to_string(),
                    simulator: "MSFS".to_string(),
                    format_family: families::msfs_bgl(),
                    detection_rules: vec![],
                    install_roots: vec![InstallRoot {
                        platform: "windows".to_string(),
                        path: "%MSFS_COMMUNITY%".to_string(),
                        subdir: String::new(),
                    }],
                    required_artifacts: vec![],
                    optional_artifacts: vec![],
                    version_constraints: vec![VersionConstraint {
                        min: None,
                        max: None,
                        note: "Official SDK path under investigation (stage 2)".to_string(),
                    }],
                    install_strategy: InstallStrategy::Subdirectory {
                        relative: "openairac-navdata".to_string(),
                    },
                    validation_strategy: ValidationStrategy::HashVerify,
                    support_state: SupportState::Research,
                },
                TargetDescriptor {
                    id: "msfs2020".to_string(),
                    display_name: "Microsoft Flight Simulator 2020".to_string(),
                    simulator: "MSFS".to_string(),
                    format_family: families::msfs_bgl(),
                    detection_rules: vec![],
                    install_roots: vec![InstallRoot {
                        platform: "windows".to_string(),
                        path: "%MSFS_COMMUNITY%".to_string(),
                        subdir: String::new(),
                    }],
                    required_artifacts: vec![],
                    optional_artifacts: vec![],
                    version_constraints: vec![VersionConstraint {
                        min: None,
                        max: None,
                        note: "Official SDK path under investigation (stage 2)".to_string(),
                    }],
                    install_strategy: InstallStrategy::Subdirectory {
                        relative: "openairac-navdata".to_string(),
                    },
                    validation_strategy: ValidationStrategy::HashVerify,
                    support_state: SupportState::Research,
                },
                TargetDescriptor {
                    id: "little-navmap".to_string(),
                    display_name: "Little Navmap".to_string(),
                    simulator: "Little Navmap".to_string(),
                    format_family: families::lnm_sqlite(),
                    detection_rules: vec![],
                    install_roots: vec![InstallRoot {
                        platform: "any".to_string(),
                        path: "%LNM_DATABASES%".to_string(),
                        subdir: String::new(),
                    }],
                    required_artifacts: vec![],
                    optional_artifacts: vec![],
                    version_constraints: vec![VersionConstraint {
                        min: None,
                        max: None,
                        note: "SQLite schema under research (stage 3)".to_string(),
                    }],
                    install_strategy: InstallStrategy::Subdirectory {
                        relative: String::new(),
                    },
                    validation_strategy: ValidationStrategy::HashVerify,
                    support_state: SupportState::Research,
                },
                TargetDescriptor {
                    id: "aerosoft-crj".to_string(),
                    display_name: "Aerosoft CRJ (NavDataPro)".to_string(),
                    simulator: "Various".to_string(),
                    format_family: families::navdatapro_text(),
                    detection_rules: vec![],
                    install_roots: vec![],
                    required_artifacts: vec![],
                    optional_artifacts: vec![],
                    version_constraints: vec![VersionConstraint {
                        min: None,
                        max: None,
                        note: "Text format under research (stage 3)".to_string(),
                    }],
                    install_strategy: InstallStrategy::Subdirectory {
                        relative: String::new(),
                    },
                    validation_strategy: ValidationStrategy::HashVerify,
                    support_state: SupportState::Research,
                },
                TargetDescriptor {
                    id: "pmdg-legacy".to_string(),
                    display_name: "PMDG legacy".to_string(),
                    simulator: "Various".to_string(),
                    format_family: families::pmdg_text(),
                    detection_rules: vec![],
                    install_roots: vec![],
                    required_artifacts: vec![],
                    optional_artifacts: vec![],
                    version_constraints: vec![VersionConstraint {
                        min: None,
                        max: None,
                        note: "Text format under research (stage 3)".to_string(),
                    }],
                    install_strategy: InstallStrategy::Subdirectory {
                        relative: String::new(),
                    },
                    validation_strategy: ValidationStrategy::HashVerify,
                    support_state: SupportState::Research,
                },
            ];
            v
        })
    }
}
