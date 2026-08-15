//! FAA CIFP cycle discovery: parse the published directory listing into
//! cycle catalog entries.
//!
//! Fail-closed: effective dates are NOT derivable from the listing (they
//! live in the CIFP Readme PDF), so every discovered cycle carries
//! `effective_from = None` until confirmed. An unconfirmed cycle can
//! never be scheduled or activated.

use anyhow::Result;
use chrono::{DateTime, Utc};

use crate::faa_cifp::FAA_CIFP_BASE_URL;
use crate::provider::fetch_url;

/// One cycle found in the source directory listing.
#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveredCycle {
    /// AIRAC cycle ident (e.g. `2608`).
    pub ident: String,
    /// Zip file stem (e.g. `CIFP_260806`).
    pub file_stem: String,
    /// Full download URL.
    pub source_uri: String,
    /// Always `None` from the listing: effective dates must be confirmed
    /// separately (fail-closed).
    pub effective_from: Option<DateTime<Utc>>,
    pub effective_until: Option<DateTime<Utc>>,
}

/// Parse a directory listing (HTML) for `CIFP_<6 digits>.zip` entries.
/// Deterministic: sorted by stem descending, deduplicated.
pub fn parse_cifp_listing(content: &str) -> Vec<DiscoveredCycle> {
    let mut stems: Vec<String> = Vec::new();
    let mut rest = content;
    while let Some(idx) = rest.find("CIFP_") {
        let candidate = &rest[idx..];
        let Some(zip_idx) = candidate.find(".zip") else {
            rest = &candidate[5..];
            continue;
        };
        let stem = &candidate[..zip_idx];
        let digits = stem.strip_prefix("CIFP_").unwrap_or("");
        if digits.len() == 6 && digits.chars().all(|c| c.is_ascii_digit()) {
            stems.push(stem.to_string());
            rest = &candidate[zip_idx + 4..];
        } else {
            rest = &candidate[5..];
        }
    }
    stems.sort();
    stems.dedup();

    stems
        .into_iter()
        .rev()
        .map(|stem| DiscoveredCycle {
            ident: stem[5..9].to_string(),
            source_uri: format!("{FAA_CIFP_BASE_URL}/{stem}.zip"),
            file_stem: stem,
            effective_from: None,
            effective_until: None,
        })
        .collect()
}

/// Fetch the FAA CIFP directory listing and discover published cycles.
/// Live network; used by `openairac cycle discover` (manual smoke only,
/// never in CI tests).
pub fn discover_cifp_cycles() -> Result<Vec<DiscoveredCycle>> {
    let listing = fetch_url(
        "FAA_CIFP",
        "directory-listing",
        FAA_CIFP_BASE_URL,
        Utc::now(),
    )?;
    Ok(parse_cifp_listing(&listing.raw_content))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cifp_listing() {
        let html = r#"
<html><body>
<a href="CIFP_260806.zip">CIFP_260806.zip</a>
<a href="CIFP_260719.zip">CIFP_260719.zip</a>
<a href="CIFP_260614.zip">CIFP_260614.zip</a>
<a href="Readme.pdf">Readme.pdf</a>
<a href="CIFP_260806.zip">duplicate</a>
<a href="CIFP_abcd06.zip">not a cycle</a>
</body></html>"#;
        let cycles = parse_cifp_listing(html);
        assert_eq!(cycles.len(), 3);
        // Newest first.
        assert_eq!(cycles[0].ident, "2608");
        assert_eq!(cycles[0].file_stem, "CIFP_260806");
        assert_eq!(
            cycles[0].source_uri,
            format!("{FAA_CIFP_BASE_URL}/CIFP_260806.zip")
        );
        assert_eq!(cycles[1].ident, "2607");
        assert_eq!(cycles[2].ident, "2606");
        for c in &cycles {
            assert!(c.effective_from.is_none()); // unconfirmed
        }
    }

    #[test]
    fn test_parse_cifp_listing_empty() {
        assert!(parse_cifp_listing("<html>no cycles</html>").is_empty());
        assert!(parse_cifp_listing("").is_empty());
    }
}
