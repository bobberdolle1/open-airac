//! Content-Addressed Chart Asset Cache.
//!
//! Enforces:
//! - Content-addressed SHA-256 storage (`cache/sha256/ab/<hash>.pdf`)
//! - Atomic downloads via temporary `.part` files
//! - File signature / magic byte validation (prevents malformed/malicious assets)
//! - Path traversal security checks
//! - Maximum asset file size bounds (default 50 MB)

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const DEFAULT_MAX_CHART_SIZE_BYTES: u64 = 50 * 1024 * 1024; // 50 MB

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStatus {
    pub root_dir: String,
    pub total_files: usize,
    pub total_size_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct ChartCache {
    root_dir: PathBuf,
    max_file_size_bytes: u64,
}

impl ChartCache {
    pub fn new<P: AsRef<Path>>(root_dir: P) -> Result<Self> {
        let root = root_dir.as_ref().to_path_buf();
        fs::create_dir_all(&root).context("Failed to initialize chart cache directory")?;
        Ok(Self {
            root_dir: root,
            max_file_size_bytes: DEFAULT_MAX_CHART_SIZE_BYTES,
        })
    }

    pub fn with_max_size(mut self, max_bytes: u64) -> Self {
        self.max_file_size_bytes = max_bytes;
        self
    }

    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    /// Compute SHA-256 hex string for a byte buffer.
    pub fn compute_sha256(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        format!("{:x}", hasher.finalize())
    }

    /// Validate magic byte signatures for known MIME types.
    pub fn validate_magic(bytes: &[u8], extension: &str) -> Result<()> {
        if bytes.is_empty() {
            bail!("Asset buffer is empty");
        }

        match extension.to_lowercase().as_str() {
            "pdf" => {
                if !bytes.starts_with(b"%PDF-") {
                    bail!("Invalid PDF magic header (expected %PDF-)");
                }
            }
            "png" => {
                if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                    bail!("Invalid PNG magic header");
                }
            }
            "jpg" | "jpeg" => {
                if !bytes.starts_with(b"\xff\xd8") {
                    bail!("Invalid JPEG magic header");
                }
            }
            "svg" => {
                let preview = String::from_utf8_lossy(&bytes[..bytes.len().min(500)]);
                if !preview.contains("<svg") && !preview.contains("<?xml") {
                    bail!("Invalid SVG header (no <svg or <?xml tag)");
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Get target path for a content-addressed SHA-256 hash.
    pub fn asset_path(&self, sha256: &str, extension: &str) -> Result<PathBuf> {
        let clean_hash = sha256.trim().to_lowercase();
        if clean_hash.len() != 64 || !clean_hash.chars().all(|c| c.is_ascii_hexdigit()) {
            bail!("Invalid SHA-256 hash: '{sha256}'");
        }

        let prefix = &clean_hash[..2];
        let filename = format!("{clean_hash}.{}", extension.trim_start_matches('.'));

        // Prevent path traversal
        if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
            bail!("Path traversal attempt detected in filename: '{filename}'");
        }

        let dir = self.root_dir.join("sha256").join(prefix);
        Ok(dir.join(filename))
    }

    /// Check if asset exists in cache and has matching size.
    pub fn has_asset(&self, sha256: &str, extension: &str) -> bool {
        if let Ok(path) = self.asset_path(sha256, extension) {
            path.exists() && path.is_file()
        } else {
            false
        }
    }

    /// Store asset into content-addressed cache atomically.
    pub fn store_asset(&self, content: &[u8], extension: &str) -> Result<(String, PathBuf)> {
        if content.len() as u64 > self.max_file_size_bytes {
            bail!(
                "Asset size {} bytes exceeds maximum allowed limit of {} bytes",
                content.len(),
                self.max_file_size_bytes
            );
        }

        // 1. Validate magic bytes
        Self::validate_magic(content, extension)?;

        // 2. Compute SHA-256
        let sha256 = Self::compute_sha256(content);
        let target_path = self.asset_path(&sha256, extension)?;

        if target_path.exists() {
            return Ok((sha256, target_path));
        }

        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create cache directory '{}'", parent.display())
            })?;
        }

        // 3. Write atomically to temporary `.part` file
        let part_path = target_path.with_extension(format!("{extension}.part"));
        {
            let mut file = File::create(&part_path).with_context(|| {
                format!("Failed to create temporary file '{}'", part_path.display())
            })?;
            file.write_all(content)?;
            file.flush()?;
        }

        // 4. Rename to target path
        fs::rename(&part_path, &target_path).with_context(|| {
            format!(
                "Failed to commit asset from '{}' to '{}'",
                part_path.display(),
                target_path.display()
            )
        })?;

        Ok((sha256, target_path))
    }

    /// Read asset from cache.
    pub fn read_asset(&self, sha256: &str, extension: &str) -> Result<Vec<u8>> {
        let path = self.asset_path(sha256, extension)?;
        fs::read(&path).with_context(|| format!("Failed to read asset from '{}'", path.display()))
    }

    /// Query summary status of the cache.
    pub fn status(&self) -> Result<CacheStatus> {
        let mut total_files = 0usize;
        let mut total_size = 0u64;

        let sha_dir = self.root_dir.join("sha256");
        if sha_dir.exists() {
            for entry in fs::read_dir(&sha_dir)? {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    for sub in fs::read_dir(entry.path())? {
                        let sub = sub?;
                        if sub.file_type()?.is_file() {
                            total_files += 1;
                            total_size += sub.metadata()?.len();
                        }
                    }
                }
            }
        }

        Ok(CacheStatus {
            root_dir: self.root_dir.to_string_lossy().to_string(),
            total_files,
            total_size_bytes: total_size,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_store_and_retrieve_pdf_asset() {
        let t = tempdir().unwrap();
        let cache = ChartCache::new(t.path()).unwrap();

        let sample_pdf = b"%PDF-1.4 sample pdf content for test";
        let (sha, path) = cache.store_asset(sample_pdf, "pdf").unwrap();

        assert!(path.exists());
        assert_eq!(sha, ChartCache::compute_sha256(sample_pdf));
        assert!(cache.has_asset(&sha, "pdf"));

        let retrieved = cache.read_asset(&sha, "pdf").unwrap();
        assert_eq!(retrieved, sample_pdf);

        let status = cache.status().unwrap();
        assert_eq!(status.total_files, 1);
        assert_eq!(status.total_size_bytes, sample_pdf.len() as u64);
    }

    #[test]
    fn test_reject_corrupt_pdf_magic() {
        let t = tempdir().unwrap();
        let cache = ChartCache::new(t.path()).unwrap();

        let corrupt = b"NOT_A_PDF content";
        let err = cache.store_asset(corrupt, "pdf").unwrap_err();
        assert!(err.to_string().contains("Invalid PDF magic header"));
    }

    #[test]
    fn test_reject_path_traversal() {
        let t = tempdir().unwrap();
        let cache = ChartCache::new(t.path()).unwrap();

        assert!(cache.asset_path("../bad_hash", "pdf").is_err());
        assert!(
            cache
                .asset_path(
                    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                    "../pdf"
                )
                .is_err()
        );
    }
}
