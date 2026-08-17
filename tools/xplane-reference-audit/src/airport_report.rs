use crate::parser::{PackageSource, parse_cifp_file, parse_earth_fix, parse_earth_nav};
use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Debug, Serialize)]
pub struct AirportReport {
    pub airport_ident: String,
    pub runways_a: Vec<String>,
    pub runways_b: Vec<String>,
    pub navaids_a: Vec<AirportNavSummary>,
    pub navaids_b: Vec<AirportNavSummary>,
    pub terminal_fixes_a: Vec<String>,
    pub terminal_fixes_b: Vec<String>,
    pub sids_a: Vec<String>,
    pub sids_b: Vec<String>,
    pub stars_a: Vec<String>,
    pub stars_b: Vec<String>,
    pub approaches_a: Vec<String>,
    pub approaches_b: Vec<String>,
    pub rf_legs_a: usize,
    pub rf_legs_b: usize,
    pub holds_a: usize,
    pub holds_b: usize,
    pub summary_note: String,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct AirportNavSummary {
    pub row_code: u8,
    pub ident: String,
    pub runway: String,
    pub frequency_raw: i32,
    pub elevation_ft: i32,
    pub bearing_or_var: f64,
    pub name: String,
}

pub fn generate_airport_report(
    airport: &str,
    pkg_a: &PackageSource,
    pkg_b: &PackageSource,
) -> Result<AirportReport> {
    let airport_upper = airport.to_uppercase();

    // 1. CIFP procedures and runways
    let file_rel = format!("CIFP/{}.dat", airport_upper);
    let content_a = pkg_a.read_file(&file_rel)?;
    let content_b = pkg_b.read_file(&file_rel)?;

    let legs_a = content_a
        .as_deref()
        .map(parse_cifp_file)
        .unwrap_or_default();
    let legs_b = content_b
        .as_deref()
        .map(parse_cifp_file)
        .unwrap_or_default();

    let mut runways_a = BTreeSet::new();
    let mut sids_a = BTreeSet::new();
    let mut stars_a = BTreeSet::new();
    let mut approaches_a = BTreeSet::new();
    let mut rf_legs_a = 0;
    let mut holds_a = 0;

    for l in &legs_a {
        match l.record_type.as_str() {
            "RWY" => {
                runways_a.insert(l.proc_ident.clone());
            }
            "SID" => {
                sids_a.insert(l.proc_ident.clone());
            }
            "STAR" => {
                stars_a.insert(l.proc_ident.clone());
            }
            "APPCH" => {
                approaches_a.insert(l.proc_ident.clone());
            }
            _ => {}
        }
        if l.path_terminator == "RF" {
            rf_legs_a += 1;
        }
        if l.path_terminator == "HA" || l.path_terminator == "HF" || l.path_terminator == "HM" {
            holds_a += 1;
        }
    }

    let mut runways_b = BTreeSet::new();
    let mut sids_b = BTreeSet::new();
    let mut stars_b = BTreeSet::new();
    let mut approaches_b = BTreeSet::new();
    let mut rf_legs_b = 0;
    let mut holds_b = 0;

    for l in &legs_b {
        match l.record_type.as_str() {
            "RWY" => {
                runways_b.insert(l.proc_ident.clone());
            }
            "SID" => {
                sids_b.insert(l.proc_ident.clone());
            }
            "STAR" => {
                stars_b.insert(l.proc_ident.clone());
            }
            "APPCH" => {
                approaches_b.insert(l.proc_ident.clone());
            }
            _ => {}
        }
        if l.path_terminator == "RF" {
            rf_legs_b += 1;
        }
        if l.path_terminator == "HA" || l.path_terminator == "HF" || l.path_terminator == "HM" {
            holds_b += 1;
        }
    }

    // 2. Navaids for this airport
    let nav_content_a = pkg_a.read_file("earth_nav.dat")?.unwrap_or_default();
    let nav_content_b = pkg_b.read_file("earth_nav.dat")?.unwrap_or_default();

    let all_nav_a = parse_earth_nav(&nav_content_a);
    let all_nav_b = parse_earth_nav(&nav_content_b);

    let navaids_a: Vec<AirportNavSummary> = all_nav_a
        .into_iter()
        .filter(|n| n.airport_or_enrt == airport_upper)
        .map(|n| AirportNavSummary {
            row_code: n.row_code,
            ident: n.ident,
            runway: n.runway_or_name.clone(),
            frequency_raw: n.frequency_raw,
            elevation_ft: n.elevation_ft,
            bearing_or_var: n.bearing_or_var,
            name: n.runway_or_name,
        })
        .collect();

    let navaids_b: Vec<AirportNavSummary> = all_nav_b
        .into_iter()
        .filter(|n| n.airport_or_enrt == airport_upper)
        .map(|n| AirportNavSummary {
            row_code: n.row_code,
            ident: n.ident,
            runway: n.runway_or_name.clone(),
            frequency_raw: n.frequency_raw,
            elevation_ft: n.elevation_ft,
            bearing_or_var: n.bearing_or_var,
            name: n.runway_or_name,
        })
        .collect();

    // 3. Terminal fixes
    let fix_content_a = pkg_a.read_file("earth_fix.dat")?.unwrap_or_default();
    let fix_content_b = pkg_b.read_file("earth_fix.dat")?.unwrap_or_default();

    let fixes_a = parse_earth_fix(&fix_content_a);
    let fixes_b = parse_earth_fix(&fix_content_b);

    let terminal_fixes_a: Vec<String> = fixes_a
        .into_iter()
        .filter(|f| f.terminal_area == airport_upper)
        .map(|f| format!("{}:{}", f.ident, f.region))
        .collect();

    let terminal_fixes_b: Vec<String> = fixes_b
        .into_iter()
        .filter(|f| f.terminal_area == airport_upper)
        .map(|f| format!("{}:{}", f.ident, f.region))
        .collect();

    let summary_note = format!(
        "Airport {}: SIDs A/B={}/{}, STARs A/B={}/{}, APPs A/B={}/{}, Navaids A/B={}/{}, TermFixes A/B={}/{}",
        airport_upper,
        sids_a.len(),
        sids_b.len(),
        stars_a.len(),
        stars_b.len(),
        approaches_a.len(),
        approaches_b.len(),
        navaids_a.len(),
        navaids_b.len(),
        terminal_fixes_a.len(),
        terminal_fixes_b.len(),
    );

    Ok(AirportReport {
        airport_ident: airport_upper,
        runways_a: runways_a.into_iter().collect(),
        runways_b: runways_b.into_iter().collect(),
        navaids_a,
        navaids_b,
        terminal_fixes_a,
        terminal_fixes_b,
        sids_a: sids_a.into_iter().collect(),
        sids_b: sids_b.into_iter().collect(),
        stars_a: stars_a.into_iter().collect(),
        stars_b: stars_b.into_iter().collect(),
        approaches_a: approaches_a.into_iter().collect(),
        approaches_b: approaches_b.into_iter().collect(),
        rf_legs_a,
        rf_legs_b,
        holds_a,
        holds_b,
        summary_note,
    })
}
