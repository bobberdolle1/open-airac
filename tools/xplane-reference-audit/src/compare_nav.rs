use crate::parser::{NavRecord, PackageSource, parse_earth_nav};
use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Serialize, Default)]
pub struct NavDiffReport {
    pub total_a: usize,
    pub total_b: usize,
    pub row_counts_a: BTreeMap<u8, usize>,
    pub row_counts_b: BTreeMap<u8, usize>,
    pub identical_lines: usize,
    pub data_equivalent: usize,
    pub row_code_mismatch: usize,
    pub frequency_mismatch: usize,
    pub elevation_mismatch: usize,
    pub bearing_mismatch: usize,
    pub coordinate_mismatch: usize,
    pub only_in_a: usize,
    pub only_in_b: usize,
    pub discrepancies: Vec<NavDiscrepancy>,
}

#[derive(Debug, Serialize, Clone)]
pub struct NavDiscrepancy {
    pub kind: String,
    pub ident: String,
    pub facility_type: String,
    pub airport_or_enrt: String,
    pub region: String,
    pub row_code_a: Option<u8>,
    pub row_code_b: Option<u8>,
    pub freq_a: Option<i32>,
    pub freq_b: Option<i32>,
    pub elev_a: Option<i32>,
    pub elev_b: Option<i32>,
    pub bearing_a: Option<f64>,
    pub bearing_b: Option<f64>,
    pub lat_a: Option<f64>,
    pub lon_a: Option<f64>,
    pub lat_b: Option<f64>,
    pub lon_b: Option<f64>,
    pub line_a: Option<String>,
    pub line_b: Option<String>,
    pub explanation: String,
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

pub fn compare_nav(
    pkg_a: &PackageSource,
    pkg_b: &PackageSource,
    max_discrepancies: usize,
) -> Result<NavDiffReport> {
    let content_a = pkg_a
        .read_file("earth_nav.dat")?
        .context("earth_nav.dat not found in Package A")?;
    let content_b = pkg_b
        .read_file("earth_nav.dat")?
        .context("earth_nav.dat not found in Package B")?;

    let navaids_a = parse_earth_nav(&content_a);
    let navaids_b = parse_earth_nav(&content_b);

    let mut report = NavDiffReport {
        total_a: navaids_a.len(),
        total_b: navaids_b.len(),
        ..Default::default()
    };

    for na in &navaids_a {
        *report.row_counts_a.entry(na.row_code).or_default() += 1;
    }
    for nb in &navaids_b {
        *report.row_counts_b.entry(nb.row_code).or_default() += 1;
    }

    // Index key: for terminal rows (4, 5, 6, 7, 8, 9, 14, 15, 16): (row_code_family, ident, airport, runway)
    // for enroute rows (2, 3, 12, 13): (row_code_family, ident, region)
    // Row code families:
    // NDB: 2
    // VOR/VORTAC/TACAN: 3
    // ILS LOC / LOC: 4 or 5
    // GS: 6
    // Markers: 7, 8, 9
    // DME: 12 or 13
    // LPV: 14 or 16
    // GLS: 15
    fn key_for(rec: &NavRecord) -> (u8, String, String, String) {
        let family = match rec.row_code {
            4 | 5 => 4,    // Localizer family
            12 | 13 => 12, // DME family
            14 | 16 => 14, // LPV family
            c => c,
        };
        (
            family,
            rec.ident.clone(),
            rec.airport_or_enrt.clone(),
            rec.region.clone(),
        )
    }

    let mut map_a: BTreeMap<(u8, String, String, String), &NavRecord> = BTreeMap::new();
    for na in &navaids_a {
        map_a.insert(key_for(na), na);
    }

    let mut map_b: BTreeMap<(u8, String, String, String), &NavRecord> = BTreeMap::new();
    for nb in &navaids_b {
        map_b.insert(key_for(nb), nb);
    }

    let all_keys: BTreeSet<(u8, String, String, String)> =
        map_a.keys().chain(map_b.keys()).cloned().collect();

    for key in &all_keys {
        match (map_a.get(key), map_b.get(key)) {
            (Some(na), Some(nb)) => {
                if na.raw_line.trim() == nb.raw_line.trim() {
                    report.identical_lines += 1;
                    continue;
                }

                let mut is_equiv = true;
                let mut reasons = Vec::new();

                if na.row_code != nb.row_code {
                    report.row_code_mismatch += 1;
                    is_equiv = false;
                    reasons.push(format!("Row code: A={} vs B={}", na.row_code, nb.row_code));
                }

                if na.frequency_raw != nb.frequency_raw {
                    report.frequency_mismatch += 1;
                    is_equiv = false;
                    reasons.push(format!(
                        "Frequency: A={} vs B={}",
                        na.frequency_raw, nb.frequency_raw
                    ));
                }

                let dist =
                    haversine_distance_meters(na.latitude, na.longitude, nb.latitude, nb.longitude);
                if dist > 50.0 {
                    report.coordinate_mismatch += 1;
                    is_equiv = false;
                    reasons.push(format!("Coordinates delta: {:.1}m", dist));
                }

                if (na.elevation_ft - nb.elevation_ft).abs() > 5 {
                    report.elevation_mismatch += 1;
                    is_equiv = false;
                    reasons.push(format!(
                        "Elevation: A={}ft vs B={}ft",
                        na.elevation_ft, nb.elevation_ft
                    ));
                }

                if (na.bearing_or_var - nb.bearing_or_var).abs() > 0.1 {
                    report.bearing_mismatch += 1;
                    is_equiv = false;
                    reasons.push(format!(
                        "Bearing/Var: A={:.3} vs B={:.3}",
                        na.bearing_or_var, nb.bearing_or_var
                    ));
                }

                if is_equiv {
                    report.data_equivalent += 1;
                } else if report.discrepancies.len() < max_discrepancies {
                    report.discrepancies.push(NavDiscrepancy {
                        kind: "DIFFERENCE".into(),
                        ident: na.ident.clone(),
                        facility_type: format!("Row {}/{}", na.row_code, nb.row_code),
                        airport_or_enrt: na.airport_or_enrt.clone(),
                        region: na.region.clone(),
                        row_code_a: Some(na.row_code),
                        row_code_b: Some(nb.row_code),
                        freq_a: Some(na.frequency_raw),
                        freq_b: Some(nb.frequency_raw),
                        elev_a: Some(na.elevation_ft),
                        elev_b: Some(nb.elevation_ft),
                        bearing_a: Some(na.bearing_or_var),
                        bearing_b: Some(nb.bearing_or_var),
                        lat_a: Some(na.latitude),
                        lon_a: Some(na.longitude),
                        lat_b: Some(nb.latitude),
                        lon_b: Some(nb.longitude),
                        line_a: Some(na.raw_line.clone()),
                        line_b: Some(nb.raw_line.clone()),
                        explanation: reasons.join("; "),
                    });
                }
            }
            (Some(na), None) => {
                report.only_in_a += 1;
                if report.discrepancies.len() < max_discrepancies {
                    report.discrepancies.push(NavDiscrepancy {
                        kind: "ONLY_IN_A".into(),
                        ident: na.ident.clone(),
                        facility_type: format!("Row {}", na.row_code),
                        airport_or_enrt: na.airport_or_enrt.clone(),
                        region: na.region.clone(),
                        row_code_a: Some(na.row_code),
                        row_code_b: None,
                        freq_a: Some(na.frequency_raw),
                        freq_b: None,
                        elev_a: Some(na.elevation_ft),
                        elev_b: None,
                        bearing_a: Some(na.bearing_or_var),
                        bearing_b: None,
                        lat_a: Some(na.latitude),
                        lon_a: Some(na.longitude),
                        lat_b: None,
                        lon_b: None,
                        line_a: Some(na.raw_line.clone()),
                        line_b: None,
                        explanation: format!(
                            "Facility present only in Package A ({})",
                            na.runway_or_name
                        ),
                    });
                }
            }
            (None, Some(nb)) => {
                report.only_in_b += 1;
                if report.discrepancies.len() < max_discrepancies {
                    report.discrepancies.push(NavDiscrepancy {
                        kind: "ONLY_IN_B".into(),
                        ident: nb.ident.clone(),
                        facility_type: format!("Row {}", nb.row_code),
                        airport_or_enrt: nb.airport_or_enrt.clone(),
                        region: nb.region.clone(),
                        row_code_a: None,
                        row_code_b: Some(nb.row_code),
                        freq_a: None,
                        freq_b: Some(nb.frequency_raw),
                        elev_a: None,
                        elev_b: Some(nb.elevation_ft),
                        bearing_a: None,
                        bearing_b: Some(nb.bearing_or_var),
                        lat_a: None,
                        lon_a: None,
                        lat_b: Some(nb.latitude),
                        lon_b: Some(nb.longitude),
                        line_a: None,
                        line_b: Some(nb.raw_line.clone()),
                        explanation: format!(
                            "Facility present only in Package B ({})",
                            nb.runway_or_name
                        ),
                    });
                }
            }
            (None, None) => unreachable!(),
        }
    }

    Ok(report)
}
