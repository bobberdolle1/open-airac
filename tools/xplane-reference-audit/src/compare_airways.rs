use crate::parser::{AirwayRecord, PackageSource, parse_earth_awy};
use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Serialize, Default)]
pub struct AirwayDiffReport {
    pub total_a: usize,
    pub total_b: usize,
    pub identical_rows: usize,
    pub data_equivalent: usize,
    pub altitude_mismatch: usize,
    pub direction_mismatch: usize,
    pub level_mismatch: usize,
    pub endpoint_type_mismatch: usize,
    pub only_in_a: usize,
    pub only_in_b: usize,
    pub samples: Vec<AirwayDiffSample>,
}

#[derive(Debug, Serialize)]
pub struct AirwayDiffSample {
    pub kind: String,
    pub start: String,
    pub end: String,
    pub names_a: Option<Vec<String>>,
    pub names_b: Option<Vec<String>>,
    pub base_a: Option<u32>,
    pub top_a: Option<u32>,
    pub base_b: Option<u32>,
    pub top_b: Option<u32>,
    pub dir_a: Option<char>,
    pub dir_b: Option<char>,
    pub line_a: Option<String>,
    pub line_b: Option<String>,
    pub note: String,
}

pub fn compare_airways(
    pkg_a: &PackageSource,
    pkg_b: &PackageSource,
    max_samples: usize,
) -> Result<AirwayDiffReport> {
    let content_a = pkg_a
        .read_file("earth_awy.dat")?
        .context("earth_awy.dat not found in Package A")?;
    let content_b = pkg_b
        .read_file("earth_awy.dat")?
        .context("earth_awy.dat not found in Package B")?;

    let awy_a = parse_earth_awy(&content_a);
    let awy_b = parse_earth_awy(&content_b);

    let mut report = AirwayDiffReport {
        total_a: awy_a.len(),
        total_b: awy_b.len(),
        ..Default::default()
    };

    // Index key: (start_ident, start_region, end_ident, end_region, level)
    // Note: direction and altitudes can vary
    type Key = (String, String, String, String, char);

    let mut map_a: BTreeMap<Key, &AirwayRecord> = BTreeMap::new();
    for a in &awy_a {
        map_a.insert(
            (
                a.start_ident.clone(),
                a.start_region.clone(),
                a.end_ident.clone(),
                a.end_region.clone(),
                a.level,
            ),
            a,
        );
    }

    let mut map_b: BTreeMap<Key, &AirwayRecord> = BTreeMap::new();
    for b in &awy_b {
        map_b.insert(
            (
                b.start_ident.clone(),
                b.start_region.clone(),
                b.end_ident.clone(),
                b.end_region.clone(),
                b.level,
            ),
            b,
        );
    }

    let all_keys: BTreeSet<Key> = map_a.keys().chain(map_b.keys()).cloned().collect();

    for key in &all_keys {
        match (map_a.get(key), map_b.get(key)) {
            (Some(ra), Some(rb)) => {
                if ra.raw_line.trim() == rb.raw_line.trim() {
                    report.identical_rows += 1;
                    continue;
                }

                let mut is_equiv = true;
                let mut reasons = Vec::new();

                if ra.base_fl != rb.base_fl || ra.top_fl != rb.top_fl {
                    report.altitude_mismatch += 1;
                    is_equiv = false;
                    reasons.push(format!(
                        "Altitudes: A={}-{} vs B={}-{}",
                        ra.base_fl, ra.top_fl, rb.base_fl, rb.top_fl
                    ));
                }

                if ra.direction != rb.direction {
                    report.direction_mismatch += 1;
                    is_equiv = false;
                    reasons.push(format!(
                        "Direction: A={} vs B={}",
                        ra.direction, rb.direction
                    ));
                }

                if ra.start_type != rb.start_type || ra.end_type != rb.end_type {
                    report.endpoint_type_mismatch += 1;
                    is_equiv = false;
                    reasons.push(format!(
                        "Endpoint types: A=({}/{}) vs B=({}/{})",
                        ra.start_type, ra.end_type, rb.start_type, rb.end_type
                    ));
                }

                if is_equiv {
                    report.data_equivalent += 1;
                } else if report.samples.len() < max_samples {
                    report.samples.push(AirwayDiffSample {
                        kind: "DIFFERENCE".into(),
                        start: format!("{}/{}", key.0, key.1),
                        end: format!("{}/{}", key.2, key.3),
                        names_a: Some(ra.names.clone()),
                        names_b: Some(rb.names.clone()),
                        base_a: Some(ra.base_fl),
                        top_a: Some(ra.top_fl),
                        base_b: Some(rb.base_fl),
                        top_b: Some(rb.top_fl),
                        dir_a: Some(ra.direction),
                        dir_b: Some(rb.direction),
                        line_a: Some(ra.raw_line.clone()),
                        line_b: Some(rb.raw_line.clone()),
                        note: reasons.join("; "),
                    });
                }
            }
            (Some(ra), None) => {
                report.only_in_a += 1;
                if report.samples.len() < max_samples {
                    report.samples.push(AirwayDiffSample {
                        kind: "ONLY_IN_A".into(),
                        start: format!("{}/{}", ra.start_ident, ra.start_region),
                        end: format!("{}/{}", ra.end_ident, ra.end_region),
                        names_a: Some(ra.names.clone()),
                        names_b: None,
                        base_a: Some(ra.base_fl),
                        top_a: Some(ra.top_fl),
                        base_b: None,
                        top_b: None,
                        dir_a: Some(ra.direction),
                        dir_b: None,
                        line_a: Some(ra.raw_line.clone()),
                        line_b: None,
                        note: format!("Names: {}", ra.names.join("-")),
                    });
                }
            }
            (None, Some(rb)) => {
                report.only_in_b += 1;
                if report.samples.len() < max_samples {
                    report.samples.push(AirwayDiffSample {
                        kind: "ONLY_IN_B".into(),
                        start: format!("{}/{}", rb.start_ident, rb.start_region),
                        end: format!("{}/{}", rb.end_ident, rb.end_region),
                        names_a: None,
                        names_b: Some(rb.names.clone()),
                        base_a: None,
                        top_a: None,
                        base_b: Some(rb.base_fl),
                        top_b: Some(rb.top_fl),
                        dir_a: None,
                        dir_b: Some(rb.direction),
                        line_a: None,
                        line_b: Some(rb.raw_line.clone()),
                        note: format!("Names: {}", rb.names.join("-")),
                    });
                }
            }
            (None, None) => unreachable!(),
        }
    }

    Ok(report)
}
