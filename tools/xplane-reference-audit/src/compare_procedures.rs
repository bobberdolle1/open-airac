use crate::parser::{CifpProcedureLeg, PackageSource, parse_cifp_file};
use anyhow::Result;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Serialize, Default)]
pub struct ProcedureDiffReport {
    pub airports_in_a: usize,
    pub airports_in_b: usize,
    pub common_airports: usize,
    pub total_procedures_a: usize,
    pub total_procedures_b: usize,
    pub identical_procedures: usize,
    pub semantically_equivalent: usize,
    pub true_discrepancies: usize,
    pub only_in_a: usize,
    pub only_in_b: usize,
    pub rnp_ar_normalized_matches: usize,
    pub loc_folded_matches: usize,
    pub gls_approaches_in_ref: usize,
    pub sid_count_a: usize,
    pub sid_count_b: usize,
    pub star_count_a: usize,
    pub star_count_b: usize,
    pub appch_count_a: usize,
    pub appch_count_b: usize,
    pub true_discrepancy_samples: Vec<ProcedureTrueDiscrepancy>,
}

#[derive(Debug, Serialize)]
pub struct ProcedureTrueDiscrepancy {
    pub airport: String,
    pub kind: String,
    pub proc_ident: String,
    pub classification: String,
    pub reason: String,
    pub legs_a_count: usize,
    pub legs_b_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NormalizedProcKey {
    pub airport: String,
    pub record_type: String, // SID, STAR, APPCH
    pub proc_ident: String,
    pub transition_ident: String,
}

pub type ProcedureMap = BTreeMap<NormalizedProcKey, NormalizedProcSignature>;

#[derive(Debug, Clone, PartialEq)]
pub struct NormalizedProcSignature {
    pub legs: Vec<NormalizedLeg>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NormalizedLeg {
    pub seq: String,
    pub path_terminator: String,
    pub fix_ident: String,
    pub turn_direction: String,
    pub alt_desc: String,
    pub alt1: String,
    pub alt2: String,
    pub speed_limit: String,
}

/// Normalize procedure identifier for cross-provider comparison.
/// In ARINC 424 / FAA CIFP, RNP AR approaches often begin with 'H' (e.g. H07-Z),
/// while commercial providers like Navigraph standardize them under 'R' (e.g. R07-Z).
pub fn normalize_proc_ident(kind: &str, ident: &str) -> String {
    let mut normalized = ident.trim().to_string();
    if kind == "APPCH" && normalized.starts_with('H') {
        normalized = format!("R{}", &normalized[1..]);
    }
    normalized
}

pub fn build_procedure_map(
    legs: &[CifpProcedureLeg],
    airport: &str,
    normalize: bool,
) -> ProcedureMap {
    let mut map: BTreeMap<NormalizedProcKey, Vec<NormalizedLeg>> = BTreeMap::new();
    for leg in legs {
        if leg.record_type != "SID" && leg.record_type != "STAR" && leg.record_type != "APPCH" {
            continue;
        }
        let proc_ident = if normalize {
            normalize_proc_ident(&leg.record_type, &leg.proc_ident)
        } else {
            leg.proc_ident.clone()
        };

        let key = NormalizedProcKey {
            airport: airport.to_string(),
            record_type: leg.record_type.clone(),
            proc_ident,
            transition_ident: leg.transition_ident.clone(),
        };
        map.entry(key).or_default().push(NormalizedLeg {
            seq: leg.seq.clone(),
            path_terminator: leg.path_terminator.clone(),
            fix_ident: leg.fix_ident.clone(),
            turn_direction: leg.turn_direction.clone(),
            alt_desc: leg.alt_desc.clone(),
            alt1: leg.alt1.clone(),
            alt2: leg.alt2.clone(),
            speed_limit: leg.speed_limit.clone(),
        });
    }

    let mut result = BTreeMap::new();
    for (k, v) in map {
        result.insert(k, NormalizedProcSignature { legs: v });
    }
    result
}

pub fn compare_airport_procedures(
    airport: &str,
    pkg_a: &PackageSource,
    pkg_b: &PackageSource,
) -> Result<Option<(ProcedureMap, ProcedureMap)>> {
    let file_rel = format!("CIFP/{}.dat", airport);
    let content_a = pkg_a.read_file(&file_rel)?;
    let content_b = pkg_b.read_file(&file_rel)?;

    if content_a.is_none() && content_b.is_none() {
        return Ok(None);
    }

    let legs_a = content_a
        .as_deref()
        .map(parse_cifp_file)
        .unwrap_or_default();
    let legs_b = content_b
        .as_deref()
        .map(parse_cifp_file)
        .unwrap_or_default();

    let map_a = build_procedure_map(&legs_a, airport, true);
    let map_b = build_procedure_map(&legs_b, airport, true);

    Ok(Some((map_a, map_b)))
}

pub fn compare_procedures_global(
    pkg_a: &PackageSource,
    pkg_b: &PackageSource,
    max_airports: Option<usize>,
    max_samples: usize,
) -> Result<ProcedureDiffReport> {
    let files_a = pkg_a.list_cifp_files()?;
    let files_b = pkg_b.list_cifp_files()?;

    let mut set_a: BTreeSet<String> = BTreeSet::new();
    for f in &files_a {
        if let Some(stem) = f.strip_prefix("CIFP/").and_then(|s| s.strip_suffix(".dat")) {
            set_a.insert(stem.to_uppercase());
        }
    }

    let mut set_b: BTreeSet<String> = BTreeSet::new();
    for f in &files_b {
        if let Some(stem) = f.strip_prefix("CIFP/").and_then(|s| s.strip_suffix(".dat")) {
            set_b.insert(stem.to_uppercase());
        }
    }

    let all_airports: BTreeSet<String> = set_a.union(&set_b).cloned().collect();
    let common_airports: BTreeSet<String> = set_a.intersection(&set_b).cloned().collect();

    let mut report = ProcedureDiffReport {
        airports_in_a: set_a.len(),
        airports_in_b: set_b.len(),
        common_airports: common_airports.len(),
        ..Default::default()
    };

    let mut airports_to_check: Vec<&String> = all_airports.iter().collect();
    if let Some(limit) = max_airports {
        airports_to_check.truncate(limit);
    }

    for airport in airports_to_check {
        if let Some((map_a, map_b)) = compare_airport_procedures(airport, pkg_a, pkg_b)? {
            for (k, sig_a) in &map_a {
                match k.record_type.as_str() {
                    "SID" => report.sid_count_a += 1,
                    "STAR" => report.star_count_a += 1,
                    "APPCH" => report.appch_count_a += 1,
                    _ => {}
                }
                report.total_procedures_a += 1;

                if let Some(sig_b) = map_b.get(k) {
                    if sig_a == sig_b {
                        report.identical_procedures += 1;
                    } else {
                        // Check if leg sequence is semantically equivalent
                        let same_fix_sequence = sig_a.legs.len() == sig_b.legs.len()
                            && sig_a.legs.iter().zip(sig_b.legs.iter()).all(|(la, lb)| {
                                la.fix_ident == lb.fix_ident
                                    && la.path_terminator == lb.path_terminator
                            });

                        if same_fix_sequence {
                            report.semantically_equivalent += 1;
                        } else {
                            report.true_discrepancies += 1;
                            if report.true_discrepancy_samples.len() < max_samples {
                                report
                                    .true_discrepancy_samples
                                    .push(ProcedureTrueDiscrepancy {
                                        airport: airport.clone(),
                                        kind: k.record_type.clone(),
                                        proc_ident: format!(
                                            "{}:{}",
                                            k.proc_ident, k.transition_ident
                                        ),
                                        classification: "LEG_STRUCTURE_DIFFERENCE".into(),
                                        reason: format!(
                                            "Package A has {} legs, Package B has {} legs",
                                            sig_a.legs.len(),
                                            sig_b.legs.len()
                                        ),
                                        legs_a_count: sig_a.legs.len(),
                                        legs_b_count: sig_b.legs.len(),
                                    });
                            }
                        }
                    }
                } else {
                    // Check if it is a LOC procedure folded into ILS
                    if k.proc_ident.starts_with('L') {
                        let ils_ident = format!("I{}", &k.proc_ident[1..]);
                        let ils_key = NormalizedProcKey {
                            airport: k.airport.clone(),
                            record_type: k.record_type.clone(),
                            proc_ident: ils_ident,
                            transition_ident: k.transition_ident.clone(),
                        };
                        if map_b.contains_key(&ils_key) {
                            report.loc_folded_matches += 1;
                            report.semantically_equivalent += 1;
                            continue;
                        }
                    }

                    // Check if it's an RNP AR match normalized
                    if k.proc_ident.starts_with('H') {
                        report.rnp_ar_normalized_matches += 1;
                    }

                    report.only_in_a += 1;
                }
            }

            for k in map_b.keys() {
                match k.record_type.as_str() {
                    "SID" => report.sid_count_b += 1,
                    "STAR" => report.star_count_b += 1,
                    "APPCH" => report.appch_count_b += 1,
                    _ => {}
                }
                report.total_procedures_b += 1;

                if !map_a.contains_key(k) {
                    if k.proc_ident.starts_with('J') {
                        report.gls_approaches_in_ref += 1;
                    }
                    report.only_in_b += 1;
                }
            }
        }
    }

    Ok(report)
}
