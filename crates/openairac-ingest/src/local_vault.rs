//! Local AIP Vault: Secure, Provenance-Preserving Local Aeronautical Data Overlay.
//!
//! Manages locally imported or authenticated aeronautical datasets (e.g. Russian CAICA RNAV
//! procedure coding tables, ARNAD ARINC datasets, Spanish ENAIRE AIP tables, Eurocontrol EAD)
//! that are authorized for local execution by the user, but strictly restricted from public
//! worldwide redistribution.
//!
//! Features:
//! - Isolated storage directory (`data/vault/` or `~/.openairac/vault/`)
//! - Atomic staging, validation, activation, and rollback
//! - Strict leak-guard verification preventing local payloads from entering public bundles
//! - Multi-provider overlay resolution on top of canonical world-open baseline

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use openairac_model::RedistributionPermission;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Metadata record for a package stored in the Local AIP Vault.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultPackageManifest {
    pub package_id: String,
    pub provider_name: String,
    pub jurisdiction: String,
    pub airac_cycle: Option<String>,
    pub effective_from: DateTime<Utc>,
    pub effective_until: Option<DateTime<Utc>>,
    pub license_id: String,
    pub redistribution: RedistributionPermission,
    pub source_files: Vec<VaultSourceFile>,
    pub entity_counts: VaultEntityCounts,
    pub imported_at: DateTime<Utc>,
    pub is_active: bool,
}

/// A source file tracked in the Local AIP Vault.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultSourceFile {
    pub relative_path: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub source_uri: Option<String>,
    pub format: String,
}

/// Entity counts tracked in the Local AIP Vault package.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VaultEntityCounts {
    pub airports: usize,
    pub runways: usize,
    pub navaids: usize,
    pub rsbn_stations: usize,
    pub waypoints: usize,
    pub airways: usize,
    pub sids: usize,
    pub stars: usize,
    pub approaches: usize,
    pub procedure_legs: usize,
}

/// High-level Local AIP Vault controller.
pub struct LocalAipVault {
    vault_root: PathBuf,
}

impl LocalAipVault {
    /// Initialize or open the Local AIP Vault at a specified root path.
    pub fn new(vault_root: impl Into<PathBuf>) -> Self {
        Self {
            vault_root: vault_root.into(),
        }
    }

    /// Default vault location in the workspace: `data/vault`.
    pub fn default_workspace_vault() -> Self {
        Self::new("data/vault")
    }

    /// Ensure the vault directory structure exists.
    pub fn init(&self) -> Result<()> {
        std::fs::create_dir_all(&self.vault_root)
            .with_context(|| format!("Failed to create vault directory: {:?}", self.vault_root))?;
        std::fs::create_dir_all(self.vault_root.join("packages"))?;
        std::fs::create_dir_all(self.vault_root.join("staging"))?;
        std::fs::create_dir_all(self.vault_root.join("history"))?;
        Ok(())
    }

    /// List all packages currently stored in the vault.
    pub fn list_packages(&self) -> Result<Vec<VaultPackageManifest>> {
        let manifest_path = self.vault_root.join("packages_manifest.json");
        if !manifest_path.exists() {
            return Ok(Vec::new());
        }
        let data = std::fs::read_to_string(&manifest_path)?;
        let manifests: Vec<VaultPackageManifest> = serde_json::from_str(&data)?;
        Ok(manifests)
    }

    /// Find an active package for a specific provider.
    pub fn find_active_package(&self, provider_name: &str) -> Result<Option<VaultPackageManifest>> {
        let pkgs = self.list_packages()?;
        Ok(pkgs
            .into_iter()
            .find(|p| p.provider_name.eq_ignore_ascii_case(provider_name) && p.is_active))
    }

    /// Stage and register a new local package into the vault.
    pub fn register_package(&self, manifest: VaultPackageManifest) -> Result<()> {
        let mut packages = self.list_packages()?;
        // Deactivate previous versions of the same provider
        for p in &mut packages {
            if p.provider_name
                .eq_ignore_ascii_case(&manifest.provider_name)
            {
                p.is_active = false;
            }
        }
        packages.push(manifest);
        let manifest_path = self.vault_root.join("packages_manifest.json");
        let json = serde_json::to_string_pretty(&packages)?;
        std::fs::write(&manifest_path, json)?;
        Ok(())
    }
    /// Stage a local file into the vault's staging area.
    pub fn stage_file(
        &self,
        provider_name: &str,
        file_path: &std::path::Path,
    ) -> Result<VaultSourceFile> {
        self.init()?;
        let bytes = std::fs::read(file_path)
            .with_context(|| format!("Reading source file: {:?}", file_path))?;
        let sha256 = crate::provider::sha256_hex(&bytes);
        let file_name = file_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let staging_dest = self
            .vault_root
            .join("staging")
            .join(format!("{provider_name}_{file_name}"));
        std::fs::write(&staging_dest, &bytes)?;

        Ok(VaultSourceFile {
            relative_path: file_name,
            sha256,
            size_bytes: bytes.len() as u64,
            source_uri: Some(file_path.to_string_lossy().to_string()),
            format: "unknown".to_string(),
        })
    }

    /// Stage a whole directory into the vault.
    pub fn stage_directory(
        &self,
        provider_name: &str,
        dir_path: &std::path::Path,
    ) -> Result<Vec<VaultSourceFile>> {
        self.init()?;
        let mut files = Vec::new();
        if dir_path.is_dir() {
            for entry in std::fs::read_dir(dir_path)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() {
                    files.push(self.stage_file(provider_name, &path)?);
                }
            }
        }
        Ok(files)
    }

    /// Rollback an active provider package to a previously active revision.
    pub fn rollback_provider(&self, provider_name: &str) -> Result<Option<VaultPackageManifest>> {
        let mut packages = self.list_packages()?;
        let mut target_idx = None;
        let mut prev_idx = None;

        for (i, p) in packages.iter().enumerate() {
            if p.provider_name.eq_ignore_ascii_case(provider_name) {
                if p.is_active {
                    target_idx = Some(i);
                } else {
                    prev_idx = Some(i);
                }
            }
        }

        if let (Some(cur), Some(prev)) = (target_idx, prev_idx) {
            packages[cur].is_active = false;
            packages[prev].is_active = true;
            let res = packages[prev].clone();
            let manifest_path = self.vault_root.join("packages_manifest.json");
            let json = serde_json::to_string_pretty(&packages)?;
            std::fs::write(&manifest_path, json)?;
            Ok(Some(res))
        } else {
            Ok(None)
        }
    }

    /// Strict leak-guard verification: Verify that a list of candidate package/provider names
    /// contains ONLY publicly redistributable datasets when building a public release bundle.
    pub fn verify_public_leak_guard(provider_names: &[&str]) -> Result<()> {
        for name in provider_names {
            if let Some(policy) = openairac_model::get_provider_policy(name) {
                if !policy.redistribution.is_publicly_redistributable() {
                    bail!(
                        "LEAK GUARD VIOLATION: Provider '{}' has permission '{:?}' (is_local_only: {}) and CANNOT be included in a public release bundle!",
                        name,
                        policy.redistribution,
                        policy.is_local_only
                    );
                }
            } else {
                bail!(
                    "LEAK GUARD VIOLATION: Unknown provider '{}' cannot be verified for public release!",
                    name
                );
            }
        }
        Ok(())
    }
}
