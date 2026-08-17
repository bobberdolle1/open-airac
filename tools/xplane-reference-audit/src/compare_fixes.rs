use crate::parser::{FixRecord, PackageSource, parse_earth_fix};
use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Serialize, Default)]
pub struct FixDiffReport {
    pub total_a: usize,
    pub total_b: usize,
    pub identical_lines: usize,
    pub coordinate_equivalent: usize,
    pub coordinate_mismatch: usize,
    pub region_mismatch: usize,
    pub terminal_area_mismatch: usize,
    pub only_in_a: usize,
    pub only_in_b: usize,
    pub samples: Vec<FixDiffSample>,
}

#[derive(Debug, Serialize)]
pub struct FixDiffSample {
    pub kind: String,
    pub ident: String,
    pub region_a: Option<String>,
    pub region_b: Option<String>,
    pub lat_a: Option<f64>,
    pub lon_a: Option<f64>,
    pub lat_b: Option<f64>,
    pub lon_b: Option<f64>,
    pub dist_meters: Option<f64>,
    pub note: String,
}

fn haversine_distance_meters(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 6371000.0;
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let lat1_rad = lat1.to_radians();
    let lat2_rad = lat2.to_radians();

    let a =
        (dlat / 2.0).sin().powi(2) + lat1_rad.cos() * lat2_rad.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    r * c
}

pub fn compare_fixes(
    pkg_a: &PackageSource,
    pkg_b: &PackageSource,
    max_samples: usize,
) -> Result<FixDiffReport> {
    let content_a = pkg_a
        .read_file("earth_fix.dat")?
        .context("earth_fix.dat not found in Package A")?;
    let content_b = pkg_b
        .read_file("earth_fix.dat")?
        .context("earth_fix.dat not found in Package B")?;

    let fixes_a = parse_earth_fix(&content_a);
    let fixes_b = parse_earth_fix(&content_b);

    let mut report = FixDiffReport {
        total_a: fixes_a.len(),
        total_b: fixes_b.len(),
        ..Default::default()
    };

    // Index by (ident, region)
    let mut map_a: BTreeMap<(String, String), &FixRecord> = BTreeMap::new();
    for f in &fixes_a {
        map_a.insert((f.ident.clone(), f.region.clone()), f);
    }

    let mut map_b: BTreeMap<(String, String), &FixRecord> = BTreeMap::new();
    for f in &fixes_b {
        map_b.insert((f.ident.clone(), f.region.clone()), f);
    }

    let all_keys: BTreeSet<(String, String)> = map_a.keys().chain(map_b.keys()).cloned().collect();

    for key in &all_keys {
        match (map_a.get(key), map_b.get(key)) {
            (Some(fa), Some(fb)) => {
                if fa.raw_line.trim() == fb.raw_line.trim() {
                    report.identical_lines += 1;
                } else {
                    let dist = haversine_distance_meters(
                        fa.latitude,
                        fa.longitude,
                        fb.latitude,
                        fb.longitude,
                    );
                    if dist < 1.0 {
                        report.coordinate_equivalent += 1;
                    } else {
                        report.coordinate_mismatch += 1;
                        if report.samples.len() < max_samples {
                            report.samples.push(FixDiffSample {
                                kind: "COORDINATE_DELTA".into(),
                                ident: key.0.clone(),
                                region_a: Some(key.1.clone()),
                                region_b: Some(key.1.clone()),
                                lat_a: Some(fa.latitude),
                                lon_a: Some(fa.longitude),
                                lat_b: Some(fb.latitude),
                                lon_b: Some(fb.longitude),
                                dist_meters: Some(dist),
                                note: format!("Distance delta: {:.2} m", dist),
                            });
                        }
                    }
                }
            }
            (Some(fa), None) => {
                report.only_in_a += 1;
                if report.samples.len() < max_samples {
                    report.samples.push(FixDiffSample {
                        kind: "ONLY_IN_A".into(),
                        ident: fa.ident.clone(),
                        region_a: Some(fa.region.clone()),
                        region_b: None,
                        lat_a: Some(fa.latitude),
                        lon_a: Some(fa.longitude),
                        lat_b: None,
                        lon_b: None,
                        dist_meters: None,
                        note: format!("Terminal area: {}, Name: {}", fa.terminal_area, fa.name),
                    });
                }
            }
            (None, Some(fb)) => {
                report.only_in_b += 1;
                if report.samples.len() < max_samples {
                    report.samples.push(FixDiffSample {
                        kind: "ONLY_IN_B".into(),
                        ident: fb.ident.clone(),
                        region_a: None,
                        region_b: Some(fb.region.clone()),
                        lat_a: None,
                        lon_a: None,
                        lat_b: Some(fb.latitude),
                        lon_b: Some(fb.longitude),
                        dist_meters: None,
                        note: format!("Terminal area: {}, Name: {}", fb.terminal_area, fb.name),
                    });
                }
            }
            (None, None) => unreachable!(),
        }
    }

    Ok(report)
}
