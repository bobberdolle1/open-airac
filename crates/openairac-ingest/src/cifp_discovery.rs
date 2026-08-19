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

/// Parse the authoritative effective interval from FAA CIFP Readme text.
///
/// Under ICAO Annex 15 and FAA Order 8260.19, the international standard
/// AIRAC effective time is exactly `09:01:00Z` UTC (0901Z) on the effective date.
/// The official FAA CIFP Readme publishes:
/// ```text
/// Effective:  0901Z
/// 03 September 2026
/// To:  0901Z
/// 01 October 2026
/// ```
pub fn parse_cifp_readme_effective(content: &str) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let mut words = content.split_whitespace();
    let mut eff_from: Option<DateTime<Utc>> = None;
    let mut eff_until: Option<DateTime<Utc>> = None;

    while let Some(w) = words.next() {
        if w.eq_ignore_ascii_case("Effective:")
            && let (Some(time_str), Some(day_str), Some(mon_str), Some(year_str)) =
                (words.next(), words.next(), words.next(), words.next())
        {
            eff_from = parse_readme_date(time_str, day_str, mon_str, year_str);
        } else if w.eq_ignore_ascii_case("To:")
            && let (Some(time_str), Some(day_str), Some(mon_str), Some(year_str)) =
                (words.next(), words.next(), words.next(), words.next())
        {
            eff_until = parse_readme_date(time_str, day_str, mon_str, year_str);
        }
    }

    match (eff_from, eff_until) {
        (Some(from), Some(until)) => Some((from, until)),
        _ => None,
    }
}

fn parse_readme_date(time_str: &str, day: &str, mon: &str, year: &str) -> Option<DateTime<Utc>> {
    let t = time_str.trim_end_matches('Z').trim_end_matches('z');
    if t.len() != 4 {
        return None;
    }
    let hour: u32 = t[..2].parse().ok()?;
    let min: u32 = t[2..].parse().ok()?;
    let day: u32 = day.parse().ok()?;
    let year: i32 = year.parse().ok()?;
    let month: u32 = match mon.to_lowercase().as_str() {
        "january" | "jan" => 1,
        "february" | "feb" => 2,
        "march" | "mar" => 3,
        "april" | "apr" => 4,
        "may" => 5,
        "june" | "jun" => 6,
        "july" | "jul" => 7,
        "august" | "aug" => 8,
        "september" | "sep" => 9,
        "october" | "oct" => 10,
        "november" | "nov" => 11,
        "december" | "dec" => 12,
        _ => return None,
    };

    use chrono::TimeZone;
    Utc.with_ymd_and_hms(year, month, day, hour, min, 0)
        .single()
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_parse_cifp_readme_effective_2609_golden() {
        let readme = r#"
FAA/Aeronautical Information Services CIFP Readme 
 
Volume:  2609 
 
Effective:  0901Z  
03 September 2026 
To:  0901Z  
01 October 2026 
 
Last Transmittal Letter:  31 July 2026 
"#;
        let (eff_from, eff_until) =
            parse_cifp_readme_effective(readme).expect("should parse authoritative 2609 dates");
        use chrono::TimeZone;
        assert_eq!(eff_from, Utc.with_ymd_and_hms(2026, 9, 3, 9, 1, 0).unwrap());
        assert_eq!(
            eff_until,
            Utc.with_ymd_and_hms(2026, 10, 1, 9, 1, 0).unwrap()
        );
    }

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
