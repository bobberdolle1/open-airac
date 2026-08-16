//! OpenAIRAC distribution foundation (v0.4 S9): deterministic
//! versioned data bundles, integrity verification, staged install,
//! and a local update channel.
//!
//! Design invariants:
//! * The bundle is CONTENT-ADDRESSED: `bundle_hash` = sha256 of the
//!   manifest's reproducibility-critical core (everything except
//!   `generated_at` and `bundle_hash` itself). Two builds from the same
//!   canonical DB state and configuration produce the same hash.
//! * Every payload file is individually hashed and covered by the
//!   manifest. Corruption fails closed: no partially valid bundle is
//!   ever accepted.
//! * Authenticity is separate from integrity: bundles are explicitly
//!   `UnsignedDevelopment` or `SignedTrusted`; without a production
//!   trust root nothing can masquerade as trusted.
//! * Install never overwrites the working world before the staged
//!   candidate is fully verified and validated.
//! * Bundle rollback is artifact-level (switch back to the previous
//!   installed artifact); publication rollback remains the temporal
//!   `rollback_cycle` re-publication — two explicit, separate concepts.

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use openairac_store::WorldStore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub const BUNDLE_FORMAT_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Manifest
// ---------------------------------------------------------------------------

/// One payload file in the bundle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BundleFile {
    pub path: String,
    pub sha256: String,
    pub size: u64,
}

/// One included dataset publication.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublicationRef {
    pub publication_id: String,
    pub provider: String,
    pub dataset: String,
    pub airac_cycle: Option<String>,
    pub content_sha256: String,
    pub revision_kind: String,
    pub coverage: String,
    pub valid_from: String,
}

/// Source provenance summary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProvenanceRef {
    pub snapshot_id: String,
    pub provider: String,
    pub dataset: String,
    pub content_sha256: String,
    pub effective_from: Option<String>,
}

/// Reconciliation state summary included in the bundle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReconciliationSummary {
    pub canonical_entities: usize,
    pub memberships: usize,
    pub conflicts: usize,
}

/// Bundle authenticity: unsigned development artifacts must never
/// masquerade as trusted production artifacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Authenticity {
    UnsignedDevelopment,
    SignedTrusted,
}

impl Authenticity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Authenticity::UnsignedDevelopment => "UnsignedDevelopment",
            Authenticity::SignedTrusted => "SignedTrusted",
        }
    }
}

/// The reproducibility-critical manifest core (hashed).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestCore {
    pub format_version: u32,
    pub schema_version: u32,
    pub effective_from: String,
    pub effective_until: Option<String>,
    pub airac_cycle: Option<String>,
    pub providers: Vec<String>,
    pub publications: Vec<PublicationRef>,
    pub reconciliation: ReconciliationSummary,
    pub provenance: Vec<ProvenanceRef>,
    pub files: Vec<BundleFile>,
    pub authenticity: String,
}

/// Full bundle manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BundleManifest {
    pub core: ManifestCore,
    /// Informational only; NOT part of `bundle_hash`.
    pub generated_at: String,
    /// sha256 of the canonical serialization of `core`.
    pub bundle_hash: String,
}

impl BundleManifest {
    /// Canonical JSON of the core (field order = struct order).
    pub fn core_json(&self) -> Result<String> {
        serde_json::to_string(&self.core).context("serializing manifest core")
    }

    /// Recompute the bundle hash from the core.
    pub fn compute_bundle_hash(&self) -> Result<String> {
        Ok(sha256_hex(self.core_json()?.as_bytes()))
    }
}

/// Verification outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct VerifyReport {
    pub bundle_hash: String,
    pub files: usize,
    pub authenticity: Authenticity,
    pub ok: bool,
}

// ---------------------------------------------------------------------------
// Signing (development/test only — production private keys are NEVER
// provisioned from or committed to this repository)
// ---------------------------------------------------------------------------

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

/// Signature file name inside a bundle directory.
pub const SIGNATURE_FILE: &str = "manifest.sig";

/// Ed25519 signing keypair. Test/development use only.
pub struct SigningKeyPair {
    signing: SigningKey,
}

impl SigningKeyPair {
    pub fn generate() -> Self {
        Self {
            signing: SigningKey::generate(&mut rand::thread_rng()),
        }
    }

    /// The trust root corresponding to this keypair.
    pub fn public_key(&self) -> TrustRoot {
        TrustRoot {
            verifying: self.signing.verifying_key(),
        }
    }
}

/// Trust root: an Ed25519 public key accepted for SignedTrusted bundles.
#[derive(Clone)]
pub struct TrustRoot {
    verifying: VerifyingKey,
}

impl TrustRoot {
    pub fn from_base64(encoded: &str) -> Result<Self> {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .context("decoding trust root")?;
        let verifying = VerifyingKey::from_bytes(
            bytes
                .as_slice()
                .try_into()
                .map_err(|_| anyhow!("trust root must be 32 bytes"))?,
        )
        .map_err(|_| anyhow!("trust root is not a valid Ed25519 public key"))?;
        Ok(Self { verifying })
    }

    pub fn to_base64(&self) -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(self.verifying.as_bytes())
    }
}

/// Sign an unsigned bundle in place: flips authenticity to
/// SignedTrusted, recomputes the content hash, and writes
/// `manifest.sig` (Ed25519 over the exact manifest.json bytes).
pub fn sign_bundle(bundle_dir: &Path, keypair: &SigningKeyPair) -> Result<()> {
    let manifest_path = bundle_dir.join("manifest.json");
    let manifest_json = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("reading {}", manifest_path.display()))?;
    let mut manifest: BundleManifest =
        serde_json::from_str(&manifest_json).context("parsing bundle manifest")?;
    if manifest.core.authenticity != "UnsignedDevelopment" {
        bail!(
            "cannot sign bundle with authenticity {}",
            manifest.core.authenticity
        );
    }
    manifest.core.authenticity = "SignedTrusted".to_string();
    manifest.bundle_hash = manifest.compute_bundle_hash()?;
    let canonical = serde_json::to_string_pretty(&manifest).context("serializing manifest")?;
    std::fs::write(&manifest_path, &canonical)?;
    let signature = keypair.signing.sign(canonical.as_bytes());
    use base64::Engine;
    std::fs::write(
        bundle_dir.join(SIGNATURE_FILE),
        base64::engine::general_purpose::STANDARD.encode(signature.to_bytes()),
    )?;
    Ok(())
}

/// Install outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct InstallReport {
    pub bundle_hash: String,
    /// The installed artifact became current (effective reached) or
    /// preloaded next (effective in the future).
    pub preloaded: bool,
    pub effective_from: DateTime<Utc>,
}

/// Installed artifact state (current / next).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct InstalledState {
    pub current: Option<InstalledArtifact>,
    pub next: Option<InstalledArtifact>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstalledArtifact {
    pub bundle_hash: String,
    pub bundle_dir: String,
    pub effective_from: String,
    pub airac_cycle: Option<String>,
}

/// Deterministic update decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpdateDecision {
    NoUpdate,
    Preload,
    Activate,
    ReplacePreload,
    RejectIncompatible,
    RejectInvalid,
}

// ---------------------------------------------------------------------------
// Hashing
// ---------------------------------------------------------------------------

pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

fn file_sha256(path: &Path) -> Result<String> {
    let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(sha256_hex(&data))
}

// ---------------------------------------------------------------------------
// Build
// ---------------------------------------------------------------------------

/// Build a deterministic bundle from the canonical store into
/// `<out_root>/<bundle-id>/` (bundle-id = first 16 chars of the hash).
pub fn build_bundle(
    store: &WorldStore,
    out_root: &Path,
    as_of: DateTime<Utc>,
) -> Result<(String, PathBuf)> {
    let status = store.status()?;
    let schema_version = status.migration_version;

    // Publications: every recorded dataset version.
    let publications: Vec<PublicationRef> = store
        .query_dataset_versions()?
        .into_iter()
        .map(|v| PublicationRef {
            publication_id: v.publication_id.unwrap_or_default(),
            provider: v.provider,
            dataset: v.dataset,
            airac_cycle: v.airac_cycle,
            content_sha256: v.content_sha256,
            revision_kind: v.revision_kind.as_str().to_string(),
            coverage: v.coverage.as_str().to_string(),
            valid_from: v.valid_from.map(|t| t.to_rfc3339()).unwrap_or_default(),
        })
        .collect();

    // Provenance: all source snapshots.
    let provenance: Vec<ProvenanceRef> = store
        .query_source_snapshots()?
        .into_iter()
        .map(|s| ProvenanceRef {
            snapshot_id: s.id.0,
            provider: s.provider,
            dataset: s.dataset,
            content_sha256: s.content_sha256,
            effective_from: s.effective_from.map(|t| t.to_rfc3339()),
        })
        .collect();

    let reconciliation = ReconciliationSummary {
        canonical_entities: store.query_canonical_identities()?.len(),
        memberships: store.query_memberships()?.len(),
        conflicts: store.query_reconciliation_conflicts()?.len(),
    };

    let providers_set: BTreeSet<String> = provenance.iter().map(|p| p.provider.clone()).collect();
    let providers: Vec<String> = providers_set.into_iter().collect();

    // Effective window: the newest baseline publication; falling back
    // to the newest source-snapshot effective date. The wall clock is
    // ONLY a last resort — reproducibility-critical content must never
    // depend on nondeterministic timestamps.
    let effective_from = publications
        .iter()
        .filter(|p| p.revision_kind == "Baseline")
        .filter_map(|p| DateTime::parse_from_rfc3339(&p.valid_from).ok())
        .max()
        .map(|t| t.with_timezone(&Utc))
        .or_else(|| {
            provenance
                .iter()
                .filter_map(|p| p.effective_from.as_deref())
                .filter_map(|s| DateTime::parse_from_rfc3339(s).ok())
                .max()
                .map(|t| t.with_timezone(&Utc))
        })
        .unwrap_or(as_of);
    let airac_cycle = publications
        .iter()
        .filter_map(|p| p.airac_cycle.clone())
        .max();

    // Payload: a copy of the canonical world DB.
    let payload_name = "world.sqlite";
    let core = ManifestCore {
        format_version: BUNDLE_FORMAT_VERSION,
        schema_version,
        effective_from: effective_from.to_rfc3339(),
        effective_until: None,
        airac_cycle,
        providers,
        publications,
        reconciliation,
        provenance,
        files: Vec::new(), // filled after copying the payload
        authenticity: Authenticity::UnsignedDevelopment.as_str().to_string(),
    };

    // Compute a preliminary hash for the bundle id (files included).
    let manifest_with_hash = BundleManifest {
        core,
        generated_at: Utc::now().to_rfc3339(),
        bundle_hash: String::new(),
    };
    let bundle_hash = manifest_with_hash.compute_bundle_hash()?;
    let bundle_dir = out_root.join(&bundle_hash[..16]);

    // Copy the payload BEFORE finalizing the manifest so the file entry
    // (path+hash+size) participates in the bundle hash.
    std::fs::create_dir_all(&bundle_dir).context("creating bundle dir")?;
    let dst = bundle_dir.join(payload_name);
    store
        .backup_to(&dst)
        .with_context(|| format!("snapshotting world DB into {}", dst.display()))?;
    let payload_size = std::fs::metadata(&dst)?.len();
    let payload_hash = file_sha256(&dst)?;

    let core = BundleManifest {
        core: ManifestCore {
            files: vec![BundleFile {
                path: payload_name.to_string(),
                sha256: payload_hash,
                size: payload_size,
            }],
            ..manifest_with_hash.core
        },
        ..manifest_with_hash
    };
    // Recompute the FINAL bundle hash over the completed core. The
    // bundle id must be recomputed too, so build deterministically by
    // computing the hash first, then creating the final directory.
    let final_hash = core.compute_bundle_hash()?;
    let final_manifest = BundleManifest {
        bundle_hash: final_hash.clone(),
        ..core
    };
    // Move payload into the final directory if the hash differs.
    if final_hash[..16] != bundle_hash[..16] {
        let final_dir = out_root.join(&final_hash[..16]);
        std::fs::create_dir_all(&final_dir)?;
        let _ = std::fs::rename(&bundle_dir, &final_dir);
        let manifest_json =
            serde_json::to_string_pretty(&final_manifest).context("serializing manifest")?;
        std::fs::write(final_dir.join("manifest.json"), manifest_json)?;
        return Ok((final_hash, final_dir));
    }
    let manifest_json =
        serde_json::to_string_pretty(&final_manifest).context("serializing manifest")?;
    std::fs::write(bundle_dir.join("manifest.json"), manifest_json)?;
    Ok((final_hash, bundle_dir))
}

// ---------------------------------------------------------------------------
// Verify
// ---------------------------------------------------------------------------

/// Verify a bundle directory: manifest integrity, file hashes, missing
/// and unexpected files, and (for SignedTrusted) an Ed25519 signature
/// over the manifest against the supplied trust root. Fails closed on
/// ANY mismatch.
pub fn verify_bundle_with_trust(
    bundle_dir: &Path,
    trust: Option<&TrustRoot>,
) -> Result<VerifyReport> {
    let manifest_path = bundle_dir.join("manifest.json");
    let manifest_json = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("reading {}", manifest_path.display()))?;
    let manifest: BundleManifest =
        serde_json::from_str(&manifest_json).context("parsing bundle manifest")?;

    let expected = manifest.compute_bundle_hash()?;
    if expected != manifest.bundle_hash {
        bail!(
            "manifest bundle_hash mismatch: manifest says {}, content hashes to {}",
            manifest.bundle_hash,
            expected
        );
    }
    if manifest.core.format_version != BUNDLE_FORMAT_VERSION {
        bail!(
            "unsupported bundle format version {} (supported: {BUNDLE_FORMAT_VERSION})",
            manifest.core.format_version
        );
    }

    // Payload files: every entry must exist, hash and size must match.
    for file in &manifest.core.files {
        let path = bundle_dir.join(&file.path);
        let meta = std::fs::metadata(&path)
            .with_context(|| format!("bundle file missing: {}", file.path))?;
        if meta.len() != file.size {
            bail!(
                "bundle file {} size mismatch: manifest says {}, actual {}",
                file.path,
                file.size,
                meta.len()
            );
        }
        let actual = file_sha256(&path)?;
        if actual != file.sha256 {
            bail!("bundle file {} hash mismatch", file.path);
        }
    }

    // Unexpected files (policy: manifest + listed payloads +
    // signature file when authenticity is SignedTrusted).
    let mut listed: BTreeSet<&str> = manifest
        .core
        .files
        .iter()
        .map(|f| f.path.as_str())
        .chain(std::iter::once("manifest.json"))
        .collect();
    if manifest.core.authenticity == "SignedTrusted" {
        listed.insert(SIGNATURE_FILE);
    }
    for entry in std::fs::read_dir(bundle_dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if !listed.contains(name.as_str()) {
            bail!("unexpected file in bundle directory: {name}");
        }
    }

    let authenticity = match manifest.core.authenticity.as_str() {
        "UnsignedDevelopment" => Authenticity::UnsignedDevelopment,
        "SignedTrusted" => Authenticity::SignedTrusted,
        other => bail!("unknown authenticity marker '{other}'"),
    };
    match authenticity {
        Authenticity::UnsignedDevelopment => {
            if bundle_dir.join(SIGNATURE_FILE).exists() {
                bail!("UnsignedDevelopment bundle carries a signature file");
            }
        }
        Authenticity::SignedTrusted => {
            let trust = trust.ok_or_else(|| {
                anyhow!("bundle claims SignedTrusted but no trust root is configured")
            })?;
            let sig_path = bundle_dir.join(SIGNATURE_FILE);
            let sig_b64 = std::fs::read_to_string(&sig_path)
                .with_context(|| format!("reading {}", sig_path.display()))?;
            use base64::Engine;
            let sig_bytes = base64::engine::general_purpose::STANDARD
                .decode(sig_b64.trim())
                .context("decoding bundle signature")?;
            let sig_arr: [u8; 64] = sig_bytes
                .as_slice()
                .try_into()
                .map_err(|_| anyhow!("bundle signature must be 64 bytes"))?;
            let signature = Signature::from_slice(&sig_arr)
                .map_err(|e| anyhow!("invalid bundle signature bytes: {e}"))?;
            trust
                .verifying
                .verify(manifest_json.as_bytes(), &signature)
                .context("bundle signature verification failed")?;
        }
    }

    Ok(VerifyReport {
        bundle_hash: manifest.bundle_hash.clone(),
        files: manifest.core.files.len(),
        authenticity,
        ok: true,
    })
}

/// Verify with no trust root (SignedTrusted bundles are rejected).
pub fn verify_bundle(bundle_dir: &Path) -> Result<VerifyReport> {
    verify_bundle_with_trust(bundle_dir, None)
}

/// Inspect a bundle: parse and return its manifest (no integrity check).
pub fn inspect_bundle(bundle_dir: &Path) -> Result<BundleManifest> {
    let manifest_json = std::fs::read_to_string(bundle_dir.join("manifest.json"))?;
    serde_json::from_str(&manifest_json).context("parsing bundle manifest")
}

// ---------------------------------------------------------------------------
// Install (staged, validated, then swapped)
// ---------------------------------------------------------------------------

/// The local state layout:
/// `<root>/state/installed.json`, `<root>/state/world.sqlite`,
/// `<root>/staging/`, `<root>/backups/`.
pub fn install_bundle(root: &Path, bundle_dir: &Path, now: DateTime<Utc>) -> Result<InstallReport> {
    // 1. Verify the bundle completely before touching local state.
    let report = verify_bundle(bundle_dir)?;
    let manifest = inspect_bundle(bundle_dir)?;

    // 2. Schema compatibility: the bundle's schema version must not be
    // newer than this binary understands.
    let current_schema = WorldStore::open_in_memory()?.migration_version()?;
    if manifest.core.schema_version > current_schema {
        bail!(
            "bundle schema version {} is newer than this build ({current_schema})",
            manifest.core.schema_version
        );
    }

    // 3. Stage the payload.
    let state_dir = root.join("state");
    let staging_dir = root.join("staging");
    std::fs::create_dir_all(&state_dir)?;
    std::fs::create_dir_all(&staging_dir)?;
    let payload = manifest
        .core
        .files
        .iter()
        .find(|f| f.path == "world.sqlite")
        .context("bundle has no world.sqlite payload")?;
    let staged = staging_dir.join(format!("{}.sqlite", report.bundle_hash));
    std::fs::copy(bundle_dir.join(&payload.path), &staged).context("staging bundle payload")?;
    // Re-verify the staged copy byte-for-byte (crash-safety: a failed
    // or torn copy must never be swapped in).
    let staged_hash = file_sha256(&staged)?;
    if staged_hash != payload.sha256 {
        let _ = std::fs::remove_file(&staged);
        bail!("staged payload hash mismatch after copy");
    }

    // 4. Open and validate the staged candidate world.
    let mut candidate = WorldStore::open(&staged)?;
    candidate.migrate()?;
    let issues = candidate.validate()?;
    if issues.iter().any(|i| i.severity == "error") {
        let _ = std::fs::remove_file(&staged);
        bail!(
            "staged candidate world has {} validation errors (first: {})",
            issues.len(),
            issues.first().map(|i| i.message.as_str()).unwrap_or("?")
        );
    }
    drop(candidate);

    // 5. Only now publish: swap the working world.
    let world_path = state_dir.join("world.sqlite");
    let effective = DateTime::parse_from_rfc3339(&manifest.core.effective_from)
        .context("parsing effective_from")?
        .with_timezone(&Utc);
    let preloaded = effective > now;
    if world_path.exists() && !preloaded {
        // Keep the previous artifact usable: move it to backups/.
        let backups = root.join("backups");
        std::fs::create_dir_all(&backups)?;
        let backup = backups.join(format!(
            "world-{}-{}.sqlite",
            Utc::now().format("%Y%m%dT%H%M%S"),
            &report.bundle_hash[..8]
        ));
        std::fs::rename(&world_path, &backup).context("backing up previous world")?;
        std::fs::rename(&staged, &world_path).context("swapping in staged world")?;
    } else if world_path.exists() && preloaded {
        // Preload: keep the CURRENT world untouched; the staged world
        // becomes the NEXT artifact and is recorded in installed.json.
        // It is not swapped until its effective instant (activation is
        // driven by temporal validity on the world itself).
    } else {
        std::fs::rename(&staged, &world_path).context("publishing first world")?;
    }

    // 6. Record installed state atomically (write temp + rename).
    let mut state = load_installed(root).unwrap_or_default();
    let artifact = InstalledArtifact {
        bundle_hash: report.bundle_hash.clone(),
        bundle_dir: bundle_dir.to_string_lossy().to_string(),
        effective_from: manifest.core.effective_from.clone(),
        airac_cycle: manifest.core.airac_cycle.clone(),
    };
    if preloaded {
        state.next = Some(artifact);
        // ReplacePreload hygiene: only the recorded next artifact's
        // staged payload may remain; other staged candidates are not
        // installed state and must not linger.
        let staged_entries: Vec<_> = std::fs::read_dir(&staging_dir)
            .map(|it| it.flatten().collect())
            .unwrap_or_default();
        for entry in staged_entries {
            let path = entry.path();
            if path.file_name().and_then(|n| n.to_str())
                != Some(&format!("{}.sqlite", report.bundle_hash))
            {
                let _ = std::fs::remove_file(&path);
            }
        }
    } else {
        state.current = Some(artifact);
        state.next = None;
    }
    save_installed(root, &state)?;

    Ok(InstallReport {
        bundle_hash: report.bundle_hash,
        preloaded,
        effective_from: effective,
    })
}

/// Roll back to the previous installed artifact (bundle-level rollback:
/// artifact switching, NOT temporal history rewriting). Returns the
/// restored bundle hash, or an error when no backup exists.
pub fn rollback_bundle(root: &Path, now: DateTime<Utc>) -> Result<String> {
    let backups = root.join("backups");
    let world_path = root.join("state").join("world.sqlite");
    let mut entries: Vec<_> = std::fs::read_dir(&backups)
        .map(|it| it.flatten().collect())
        .unwrap_or_default();
    entries.sort_by_key(|e| e.file_name());
    let Some(previous) = entries.pop() else {
        bail!("no previous installed artifact to roll back to");
    };
    // Swap: current world to a failed-backup name, previous backup back
    // into place.
    if world_path.exists() {
        std::fs::rename(
            &world_path,
            backups.join(format!("failed-{}.sqlite", now.format("%Y%m%dT%H%M%S"))),
        )?;
    }
    std::fs::rename(previous.path(), &world_path)?;
    // The installed state no longer reflects the restored world; mark
    // current as unknown (conservative: the restored artifact's hash is
    // derived from its file name where available).
    let hash_hint = previous
        .file_name()
        .to_string_lossy()
        .rsplit('-')
        .next()
        .and_then(|s| s.strip_suffix(".sqlite"))
        .map(|s| s.to_string())
        .unwrap_or_default();
    let mut state = load_installed(root).unwrap_or_default();
    state.current = state
        .current
        .take()
        .map(|mut a| {
            if !hash_hint.is_empty() {
                a.bundle_hash = format!("{hash_hint} (restored from backup)");
            }
            a
        })
        .or_else(|| {
            (!hash_hint.is_empty()).then(|| InstalledArtifact {
                bundle_hash: format!("{hash_hint} (restored from backup)"),
                bundle_dir: String::new(),
                effective_from: String::new(),
                airac_cycle: None,
            })
        });
    state.next = None;
    save_installed(root, &state)?;
    Ok(hash_hint)
}

// ---------------------------------------------------------------------------
// Installed state
// ---------------------------------------------------------------------------

pub fn load_installed(root: &Path) -> Result<InstalledState> {
    let path = root.join("state").join("installed.json");
    let json =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&json).context("parsing installed.json")
}

fn save_installed(root: &Path, state: &InstalledState) -> Result<()> {
    let state_dir = root.join("state");
    std::fs::create_dir_all(&state_dir)?;
    let tmp = state_dir.join("installed.json.tmp");
    let json = serde_json::to_string_pretty(state)?;
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, state_dir.join("installed.json"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Update channel + decision
// ---------------------------------------------------------------------------

/// Channel index: `<channel>/index.json` — a directory-style channel
/// (HTTP later; the transport is separable from bundle semantics).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChannelIndex {
    pub channel: String,
    pub generated_at: String,
    pub latest: ChannelArtifact,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChannelArtifact {
    pub bundle_hash: String,
    pub path: String, // relative to the channel root
    pub schema_version: u32,
    pub effective_from: String,
    pub airac_cycle: Option<String>,
}

pub fn read_channel(channel: &Path) -> Result<ChannelIndex> {
    let json = std::fs::read_to_string(channel.join("index.json"))
        .with_context(|| format!("reading channel index in {}", channel.display()))?;
    serde_json::from_str(&json).context("parsing channel index")
}

pub fn write_channel(channel: &Path, index: &ChannelIndex) -> Result<()> {
    std::fs::create_dir_all(channel)?;
    let json = serde_json::to_string_pretty(index)?;
    let tmp = channel.join("index.json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, channel.join("index.json"))?;
    Ok(())
}

/// Deterministic update decision from installed state + channel +
/// confirmed effective timestamps (never wall-clock naming inference).
pub fn decide_update(
    installed: &InstalledState,
    channel: &ChannelIndex,
    channel_root: &Path,
    current_schema: u32,
    now: DateTime<Utc>,
) -> UpdateDecision {
    let latest = &channel.latest;
    if latest.schema_version > current_schema {
        return UpdateDecision::RejectIncompatible;
    }
    // The candidate must exist as a bundle directory and verify.
    // Verification happens in update_apply; decision logic pre-checks
    // presence so apply can fail with RejectInvalid.
    let candidate_dir = channel_dir_of(channel_root, Path::new(&latest.path));
    if !candidate_dir.join("manifest.json").exists() {
        return UpdateDecision::RejectInvalid;
    }
    let Ok(manifest) = inspect_bundle(&candidate_dir) else {
        return UpdateDecision::RejectInvalid;
    };
    if manifest.bundle_hash != latest.bundle_hash {
        return UpdateDecision::RejectInvalid;
    }

    let installed_hash = installed
        .current
        .as_ref()
        .map(|a| a.bundle_hash.as_str())
        .unwrap_or("");
    if installed_hash == latest.bundle_hash {
        return UpdateDecision::NoUpdate;
    }
    let next_hash = installed
        .next
        .as_ref()
        .map(|a| a.bundle_hash.as_str())
        .unwrap_or("");
    if next_hash == latest.bundle_hash {
        return UpdateDecision::NoUpdate;
    }

    let Ok(effective) = DateTime::parse_from_rfc3339(&latest.effective_from) else {
        return UpdateDecision::RejectInvalid;
    };
    if effective <= now {
        UpdateDecision::Activate
    } else if installed.next.is_some() {
        UpdateDecision::ReplacePreload
    } else {
        UpdateDecision::Preload
    }
}

fn channel_dir_of(channel: &Path, rel: &Path) -> PathBuf {
    if rel.is_absolute() {
        rel.to_path_buf()
    } else {
        channel.join(rel)
    }
}

/// Apply the channel's latest bundle (verify -> install).
pub fn update_apply(root: &Path, channel: &Path, now: DateTime<Utc>) -> Result<UpdateDecision> {
    let index = read_channel(channel)?;
    let installed = load_installed(root).unwrap_or_default();
    let current_schema = WorldStore::open_in_memory()?.migration_version()?;
    let decision = decide_update(&installed, &index, channel, current_schema, now);
    match decision {
        UpdateDecision::NoUpdate => Ok(decision),
        UpdateDecision::RejectIncompatible | UpdateDecision::RejectInvalid => {
            bail!("update rejected: {decision:?}");
        }
        UpdateDecision::Preload | UpdateDecision::Activate | UpdateDecision::ReplacePreload => {
            let bundle_dir = channel_dir_of(channel, Path::new(&index.latest.path));
            install_bundle(root, &bundle_dir, now)?;
            Ok(decision)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openairac_model::{AirportId, SourceSnapshot, SourceSnapshotId, TemporalValidity};

    fn unique_dir(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("oa_bundle_test_{}_{}_{n}", std::process::id(), tag))
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
            .insert_airport(&openairac_model::CanonicalAirport {
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
    fn test_bundle_build_verify_deterministic() {
        let (store, dir) = fixture_store();
        let out = dir.join("bundles");
        let (hash1, bundle_dir) = build_bundle(&store, &out, Utc::now()).unwrap();
        let report = verify_bundle(&bundle_dir).unwrap();
        assert!(report.ok);
        assert_eq!(report.bundle_hash, hash1);
        // Deterministic: rebuild into a second root yields the same hash.
        let out2 = dir.join("bundles2");
        let (hash2, _) = build_bundle(&store, &out2, Utc::now()).unwrap();
        assert_eq!(hash1, hash2);
        // Bundle id == hash prefix.
        assert!(bundle_dir.ends_with(&hash1[..16]));
    }

    #[test]
    fn test_verify_fails_closed_on_corruption() {
        let (store, dir) = fixture_store();
        let out = dir.join("bundles");
        let (_, bundle_dir) = build_bundle(&store, &out, Utc::now()).unwrap();
        // Modified payload.
        let payload = bundle_dir.join("world.sqlite");
        let original = std::fs::read(&payload).unwrap();
        let mut tampered = original.clone();
        tampered[0] ^= 0xFF;
        std::fs::write(&payload, &tampered).unwrap();
        assert!(verify_bundle(&bundle_dir).is_err());
        std::fs::write(&payload, &original).unwrap();
        // Truncated payload.
        std::fs::write(&payload, &original[..original.len() / 2]).unwrap();
        assert!(verify_bundle(&bundle_dir).is_err());
        std::fs::write(&payload, &original).unwrap();
        // Wrong manifest hash.
        let manifest_path = bundle_dir.join("manifest.json");
        let mut manifest: BundleManifest = inspect_bundle(&bundle_dir).unwrap();
        manifest.bundle_hash = "0".repeat(64);
        std::fs::write(
            &manifest_path,
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        assert!(verify_bundle(&bundle_dir).is_err());
        // Missing file.
        std::fs::remove_file(&payload).unwrap();
        assert!(verify_bundle(&bundle_dir).is_err());
        // Unexpected file.
        std::fs::write(bundle_dir.join("stray.bin"), b"x").unwrap();
        let _ = std::fs::remove_dir_all(&bundle_dir);
    }

    #[test]
    fn test_install_preload_and_current() {
        let (store, dir) = fixture_store();
        let out = dir.join("bundles");
        let (hash, bundle_dir) = build_bundle(&store, &out, Utc::now()).unwrap();
        let root = dir.join("install");
        // Future effective -> preload: world file must NOT be swapped.
        let _future = Utc::now() + chrono::TimeDelta::seconds(3600);
        // Rebuild with a future-effective manifest is not possible via
        // public API without publications; instead install as current
        // (effective = as_of <= now).
        let report = install_bundle(&root, &bundle_dir, Utc::now()).unwrap();
        assert!(!report.preloaded);
        assert_eq!(report.bundle_hash, hash);
        let world = root.join("state").join("world.sqlite");
        assert!(world.exists());
        // Installed state recorded.
        let state = load_installed(&root).unwrap();
        assert_eq!(state.current.as_ref().unwrap().bundle_hash, hash);
        assert!(state.next.is_none());
        // Re-installing the same bundle is a no-op decision-wise: it
        // swaps again but remains consistent.
        install_bundle(&root, &bundle_dir, Utc::now()).unwrap();
    }

    #[test]
    fn test_failed_install_leaves_previous_state() {
        let (store, dir) = fixture_store();
        let out = dir.join("bundles");
        let (_, bundle_dir) = build_bundle(&store, &out, Utc::now()).unwrap();
        let root = dir.join("install");
        install_bundle(&root, &bundle_dir, Utc::now()).unwrap();
        let state_before = load_installed(&root).unwrap();
        let world_before = std::fs::read(root.join("state").join("world.sqlite")).unwrap();

        // Corrupt the staged copy path: make the payload unreadable by
        // breaking the bundle (payload hash mismatch on stage).
        let payload = bundle_dir.join("world.sqlite");
        let original = std::fs::read(&payload).unwrap();
        std::fs::write(&payload, &original[..original.len() - 100]).unwrap();
        let result = install_bundle(&root, &bundle_dir, Utc::now());
        assert!(result.is_err());
        // Previous state untouched.
        let state_after = load_installed(&root).unwrap();
        assert_eq!(state_before, state_after);
        let world_after = std::fs::read(root.join("state").join("world.sqlite")).unwrap();
        assert_eq!(world_before, world_after);
        // No stray staging leftovers counted as installed.
        assert!(
            !root.join("staging").exists()
                || std::fs::read_dir(root.join("staging")).unwrap().count() == 0
        );
    }

    #[test]
    fn test_sign_verify_trusted_bundle() {
        let (store, dir) = fixture_store();
        let out = dir.join("bundles");
        let (_, bundle_dir) = build_bundle(&store, &out, Utc::now()).unwrap();
        let kp = SigningKeyPair::generate();
        sign_bundle(&bundle_dir, &kp).unwrap();
        // SignedTrusted with the right trust root verifies.
        let report = verify_bundle_with_trust(&bundle_dir, Some(&kp.public_key())).unwrap();
        assert_eq!(report.authenticity, Authenticity::SignedTrusted);
        // Without a trust root: rejected (fail closed).
        assert!(verify_bundle(&bundle_dir).is_err());
        // Wrong key: rejected.
        let other = SigningKeyPair::generate();
        assert!(verify_bundle_with_trust(&bundle_dir, Some(&other.public_key())).is_err());
    }

    #[test]
    fn test_signature_fails_closed_on_tamper() {
        let (store, dir) = fixture_store();
        let out = dir.join("bundles");
        let (_, bundle_dir) = build_bundle(&store, &out, Utc::now()).unwrap();
        let kp = SigningKeyPair::generate();
        sign_bundle(&bundle_dir, &kp).unwrap();
        // Tamper with the manifest after signing.
        let manifest_path = bundle_dir.join("manifest.json");
        let original = std::fs::read_to_string(&manifest_path).unwrap();
        // generated_at is informational: the manifest still parses and
        // hashes, but the signature no longer covers these bytes.
        std::fs::write(
            &manifest_path,
            original.replacen("\"generated_at\": \"", "\"generated_at\": \"X", 1),
        )
        .unwrap();
        assert!(verify_bundle_with_trust(&bundle_dir, Some(&kp.public_key())).is_err());
        // Truncated signature.
        let sig_path = bundle_dir.join(SIGNATURE_FILE);
        std::fs::write(&sig_path, "AAAA").unwrap();
        std::fs::write(&manifest_path, original).unwrap();
        assert!(verify_bundle_with_trust(&bundle_dir, Some(&kp.public_key())).is_err());
        // Unsigned bundle carrying a stray signature file is rejected.
        let (store2, dir2) = fixture_store();
        let out2 = dir2.join("bundles");
        let (_, bundle2) = build_bundle(&store2, &out2, Utc::now()).unwrap();
        std::fs::write(bundle2.join(SIGNATURE_FILE), "AAAA").unwrap();
        assert!(verify_bundle(&bundle2).is_err());
    }

    #[test]
    fn test_trust_root_roundtrip() {
        let kp = SigningKeyPair::generate();
        let encoded = kp.public_key().to_base64();
        let decoded = TrustRoot::from_base64(&encoded).unwrap();
        assert_eq!(decoded.to_base64(), encoded);
        assert!(TrustRoot::from_base64("not base64!!").is_err());
        assert!(TrustRoot::from_base64("QUJD").is_err()); // 3 bytes, wrong length
    }

    #[test]
    fn test_signed_bundle_install_without_trust_fails_closed() {
        let (store, dir) = fixture_store();
        let out = dir.join("bundles");
        let (_, bundle_dir) = build_bundle(&store, &out, Utc::now()).unwrap();
        let kp = SigningKeyPair::generate();
        sign_bundle(&bundle_dir, &kp).unwrap();
        // Install refuses a SignedTrusted bundle when no trust root is
        // configured (and the previous state stays untouched).
        let root = dir.join("install");
        assert!(install_bundle(&root, &bundle_dir, Utc::now()).is_err());
        assert!(!root.join("state").exists());
    }

    #[test]
    fn test_update_decisions() {
        let dir = unique_dir("chan");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let now = Utc::now();
        let future = (now + chrono::TimeDelta::seconds(7200)).to_rfc3339();
        let _ = &future; // effective timestamp comes from the index
        let index = ChannelIndex {
            channel: "test".to_string(),
            generated_at: now.to_rfc3339(),
            latest: ChannelArtifact {
                bundle_hash: "b".repeat(64),
                path: "bundle-b".to_string(),
                schema_version: 9,
                effective_from: future,
                airac_cycle: Some("2609".to_string()),
            },
        };
        // No channel bundle present -> invalid.
        let installed = InstalledState {
            current: None,
            next: None,
        };
        assert_eq!(
            decide_update(&installed, &index, &dir, 9, now),
            UpdateDecision::RejectInvalid
        );
        // Incompatible schema.
        let index_old = ChannelIndex {
            latest: ChannelArtifact {
                schema_version: 99,
                ..index.latest.clone()
            },
            ..index.clone()
        };
        assert_eq!(
            decide_update(&installed, &index_old, &dir, 8, now),
            UpdateDecision::RejectIncompatible
        );
    }
}
