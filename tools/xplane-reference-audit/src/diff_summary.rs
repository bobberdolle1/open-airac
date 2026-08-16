use crate::compare_airways::{AirwayDiffReport, compare_airways};
use crate::compare_fixes::{FixDiffReport, compare_fixes};
use crate::compare_nav::{NavDiffReport, compare_nav};
use crate::compare_procedures::{ProcedureDiffReport, compare_procedures_global};
use crate::parser::PackageSource;
use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ComprehensiveDiffSummary {
    pub fixes: FixDiffReport,
    pub nav: NavDiffReport,
    pub airways: AirwayDiffReport,
    pub procedures: ProcedureDiffReport,
}

pub fn run_diff_summary(
    pkg_a: &PackageSource,
    pkg_b: &PackageSource,
    max_airports: Option<usize>,
    max_samples: usize,
) -> Result<ComprehensiveDiffSummary> {
    let fixes = compare_fixes(pkg_a, pkg_b, max_samples)?;
    let nav = compare_nav(pkg_a, pkg_b, max_samples)?;
    let airways = compare_airways(pkg_a, pkg_b, max_samples)?;
    let procedures = compare_procedures_global(pkg_a, pkg_b, max_airports, max_samples)?;

    Ok(ComprehensiveDiffSummary {
        fixes,
        nav,
        airways,
        procedures,
    })
}
