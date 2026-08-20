//! Official Russian Federation CAICA RNAV Procedure Publication Parser.
//!
//! Parses structured procedure database tables published by CAICA / Rosaviatsiya
//! (Центр Аэронавигационной Информации ФГУП «Госкорпорация по ОрВД») in the official
//! "Электронный Сборник данных, обеспечивающих кодирование схем с применением RNAV
//! для навигационной базы данных для аэродромов Российской Федерации".
//!
//! Semantics:
//! - Exact path terminators: IF, TF, CF, DF, CA, FA, VA, VI, VM, RF, HA, HF, HM.
//! - Preserves dual Magnetic and True course: `Путевой угол °M(T)` -> `course_mag_deg`, `course_true_deg`.
//! - Robust Russian altitude syntax: `+1100`, `-4000`, `7000-5000`, `FL150-FL140`, `+FL230`, `-FL190`, `4000`.
//! - Russian speed limit syntax: `230`, `-250`, `250`, `-205`.
//! - Distance unit handling: Explicit km to NM conversion (`distance_nm = distance_km / 1.852`).
//! - High-precision coordinate parsing: `DD MM SS.ss N DDD MM SS.ss E`.
//! - Mixed AIRAC row provenance tracking: Preserves row-level revision cycle while treating current page as effective publication state.

use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use openairac_model::{
    CanonicalProcedureLeg, ProcedureLegId, SourceSnapshot, SourceSnapshotId, TemporalValidity,
};
use openairac_procedures::ProcedureKind;
use openairac_store::WorldStore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Altitude constraint descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaicaAltitudeConstraint {
    At(i32),
    AtOrAbove(i32),
    AtOrBelow(i32),
    Between(i32, i32),
}

/// Parsed procedure table row from official CAICA RNAV publication.
#[derive(Debug, Clone, PartialEq)]
pub struct CaicaRawLegRow {
    pub sequence_number: u32,
    pub procedure_ident: String,
    pub runway_transition: Option<String>,
    pub enroute_transition: Option<String>,
    pub path_terminator: String,
    pub fix_ident: String,
    pub fix_lat: Option<f64>,
    pub fix_lon: Option<f64>,
    pub raw_coordinates_str: Option<String>,
    pub is_flyover: bool,
    pub course_mag_deg: Option<f64>,
    pub course_true_deg: Option<f64>,
    pub turn_direction: Option<char>,
    pub altitude_constraint: Option<CaicaAltitudeConstraint>,
    pub speed_limit_kts: Option<u32>,
    pub is_speed_max: bool,
    pub distance_km: Option<f64>,
    pub distance_nm: Option<f64>,
    pub vertical_angle_deg: Option<f64>,
    pub tch_ft: Option<f64>,
    pub nav_spec: Option<String>,
    pub airac_row_cycle: Option<String>,
    pub remarks: Option<String>,
}

/// A complete structured Russian procedure parsed from official CAICA publication.
#[derive(Debug, Clone, PartialEq)]
pub struct CaicaParsedProcedure {
    pub airport_icao: String,
    pub airport_ru_name: Option<String>,
    pub procedure_ident: String,
    pub procedure_kind: ProcedureKind,
    pub runway: Option<String>,
    pub transition: Option<String>,
    pub nav_spec: Option<String>,
    pub legs: Vec<CaicaRawLegRow>,
    pub source_doc_title: String,
}

/// Discovered Russian airport entry from official CAICA ProcedureList index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaicaDiscoveredAirport {
    pub icao: String,
    pub name_ru: String,
    pub name_en: Option<String>,
    pub source_page_url: String,
    pub has_sids: bool,
    pub has_stars: bool,
    pub has_approaches: bool,
    pub airac_cycle: Option<String>,
}

/// National statistics summary across all parsed Russian CAICA procedures.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CaicaNationalStatistics {
    pub total_airports_discovered: usize,
    pub airports_with_sids: usize,
    pub airports_with_stars: usize,
    pub airports_with_approaches: usize,
    pub total_procedures: usize,
    pub total_sids: usize,
    pub total_stars: usize,
    pub total_approaches: usize,
    pub total_legs: usize,
    pub total_holds: usize,
    pub path_terminator_histogram: std::collections::BTreeMap<String, usize>,
    pub row_revision_histogram: std::collections::BTreeMap<String, usize>,
    pub rejected_pages_count: usize,
    pub rejection_reasons: Vec<String>,
}

/// Dynamic indexer for official CAICA ProcedureList navigation collections.
#[derive(Debug, Clone, Default)]
pub struct CaicaProcedureIndex {
    pub airports: Vec<CaicaDiscoveredAirport>,
}

impl CaicaProcedureIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Discover airports from official CAICA ProcedureList HTML navigation index.
    pub fn discover_from_index_text(&mut self, html_text: &str) -> usize {
        let mut count = 0;
        for line in html_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // Example patterns:
            // <a href="book/rus/uhna.htm">АЯН (МУНУК) / AYAN (MUNUK) [UHNA]</a>
            // UERS | САСКЫЛАХ | SASKYLAKH | book/rus/uers.htm | SID,STAR,APCH | 2608
            if trimmed.contains('|') {
                let parts: Vec<&str> = trimmed.split('|').map(|s| s.trim()).collect();
                if parts.len() >= 4 {
                    let icao = parts[0].to_uppercase();
                    let ru_name = parts[1].to_string();
                    let en_name = if !parts[2].is_empty() {
                        Some(parts[2].to_string())
                    } else {
                        None
                    };
                    let url = parts[3].to_string();
                    let procs_str = if parts.len() > 4 {
                        parts[4].to_uppercase()
                    } else {
                        "SID,STAR,APCH".to_string()
                    };
                    let cycle = if parts.len() > 5 {
                        Some(parts[5].to_string())
                    } else {
                        None
                    };

                    let discovered = CaicaDiscoveredAirport {
                        icao: icao.clone(),
                        name_ru: ru_name,
                        name_en: en_name,
                        source_page_url: url,
                        has_sids: procs_str.contains("SID"),
                        has_stars: procs_str.contains("STAR"),
                        has_approaches: procs_str.contains("APCH")
                            || procs_str.contains("RNP")
                            || procs_str.contains("APP"),
                        airac_cycle: cycle,
                    };

                    if !self.airports.iter().any(|a| a.icao == icao) {
                        self.airports.push(discovered);
                        count += 1;
                    }
                }
            } else if let Some(href_idx) = trimmed.find("href=\"") {
                let rest = &trimmed[href_idx + 6..];
                if let Some(end_quote) = rest.find('\"') {
                    let url = rest[..end_quote].to_string();
                    // Extract ICAO from url or link text
                    let link_text = if let Some(tag_end) = rest.find('>') {
                        if let Some(closing) = rest.find("</a>") {
                            &rest[tag_end + 1..closing]
                        } else {
                            ""
                        }
                    } else {
                        ""
                    };

                    let icao = if let Some(open_b) = link_text.find('[') {
                        if let Some(close_b) = link_text.find(']') {
                            link_text[open_b + 1..close_b].trim().to_uppercase()
                        } else {
                            url.trim_end_matches(".htm")
                                .trim_start_matches("book/rus/")
                                .to_uppercase()
                        }
                    } else {
                        url.trim_end_matches(".htm")
                            .trim_start_matches("book/rus/")
                            .to_uppercase()
                    };

                    if icao.len() == 4
                        && icao.starts_with('U')
                        && !self.airports.iter().any(|a| a.icao == icao)
                    {
                        self.airports.push(CaicaDiscoveredAirport {
                            icao,
                            name_ru: link_text.to_string(),
                            name_en: None,
                            source_page_url: url,
                            has_sids: true,
                            has_stars: true,
                            has_approaches: true,
                            airac_cycle: Some("2608".to_string()),
                        });
                        count += 1;
                    }
                }
            }
        }
        count
    }

    /// Compute comprehensive national procedure statistics.
    pub fn compute_national_statistics(
        procedures: &[CaicaParsedProcedure],
    ) -> CaicaNationalStatistics {
        let mut stats = CaicaNationalStatistics::default();
        let mut apt_set = std::collections::BTreeSet::new();
        let mut sid_apts = std::collections::BTreeSet::new();
        let mut star_apts = std::collections::BTreeSet::new();
        let mut app_apts = std::collections::BTreeSet::new();

        for p in procedures {
            apt_set.insert(p.airport_icao.clone());
            stats.total_procedures += 1;
            match p.procedure_kind {
                ProcedureKind::Sid => {
                    stats.total_sids += 1;
                    sid_apts.insert(p.airport_icao.clone());
                }
                ProcedureKind::Star => {
                    stats.total_stars += 1;
                    star_apts.insert(p.airport_icao.clone());
                }
                ProcedureKind::Approach => {
                    stats.total_approaches += 1;
                    app_apts.insert(p.airport_icao.clone());
                }
            }

            for leg in &p.legs {
                stats.total_legs += 1;
                *stats
                    .path_terminator_histogram
                    .entry(leg.path_terminator.clone())
                    .or_insert(0) += 1;
                if leg.path_terminator == "HM"
                    || leg.path_terminator == "HA"
                    || leg.path_terminator == "HF"
                {
                    stats.total_holds += 1;
                }
                if let Some(rev) = &leg.airac_row_cycle {
                    *stats.row_revision_histogram.entry(rev.clone()).or_insert(0) += 1;
                }
            }
        }

        stats.total_airports_discovered = apt_set.len();
        stats.airports_with_sids = sid_apts.len();
        stats.airports_with_stars = star_apts.len();
        stats.airports_with_approaches = app_apts.len();

        stats
    }
}

/// Provider for Russian Federation CAICA official structured procedure publications.
pub struct CaicaProcedureProvider {
    pub provider_name: String,
    pub namespace: String,
    pub license: String,
}

impl Default for CaicaProcedureProvider {
    fn default() -> Self {
        Self::new("RU_CAICA_PROCEDURES", "caica_proc", "CAICA-TermsOfUse")
    }
}

impl CaicaProcedureProvider {
    pub fn new(provider_name: &str, namespace: &str, license: &str) -> Self {
        Self {
            provider_name: provider_name.to_string(),
            namespace: namespace.to_string(),
            license: license.to_string(),
        }
    }

    /// Parse raw tabular text or HTML snippet extracted from a CAICA RNAV procedure page.
    pub fn parse_procedure_text(
        text: &str,
        airport_icao: &str,
        default_kind: ProcedureKind,
        doc_title: &str,
    ) -> Result<Vec<CaicaParsedProcedure>> {
        let mut clean_icao = airport_icao.trim().to_uppercase();
        let mut raw_rows = Vec::new();

        let mut current_proc = String::new();
        let mut current_rwy = None;
        let mut current_nav_spec = None;
        let mut airport_ru_name = None;

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with('#') || trimmed.starts_with("//") {
                // Extract any 4-letter Russian ICAO code starting with 'U' from comment lines
                let words: Vec<&str> = trimmed
                    .split(|c: char| !c.is_ascii_alphanumeric())
                    .collect();
                for w in words {
                    if w.len() == 4
                        && w.starts_with('U')
                        && w.chars().all(|c| c.is_ascii_uppercase())
                    {
                        clean_icao = w.to_string();
                        break;
                    }
                }
                continue;
            }
            // Check for header line: e.g. "PROCEDURE: EMGAS 1A | RWY: 24C | NAV: RNAV 1 | APT: ШЕРЕМЕТЬЕВО"
            if trimmed.to_uppercase().starts_with("PROCEDURE:")
                || trimmed.to_uppercase().starts_with("СХЕМА:")
                || trimmed.to_uppercase().starts_with("SID:")
                || trimmed.to_uppercase().starts_with("STAR:")
                || trimmed.to_uppercase().starts_with("APPROACH:")
                || trimmed.to_uppercase().starts_with("RNP:")
            {
                let parts: Vec<&str> = trimmed.split('|').collect();
                for part in parts {
                    let p_trim = part.trim();
                    if let Some(idx) = p_trim.find(':') {
                        let key = p_trim[..idx].trim().to_uppercase();
                        let val = p_trim[idx + 1..].trim().to_string();
                        if key.contains("PROCEDURE")
                            || key.contains("СХЕМА")
                            || key == "SID"
                            || key == "STAR"
                            || key == "APP"
                            || key == "RNP"
                        {
                            current_proc = val;
                        } else if key.contains("RWY")
                            || key.contains("RUNWAY")
                            || key.contains("ВПП")
                        {
                            current_rwy = Some(val.to_uppercase());
                        } else if key.contains("NAV")
                            || key.contains("SPEC")
                            || key.contains("СПЕЦИФИКАЦИЯ")
                        {
                            current_nav_spec = Some(val);
                        } else if key.contains("APT")
                            || key.contains("АЭРОДРОМ")
                            || key.contains("АЭРОПОРТ")
                        {
                            let ru_name = val.clone();
                            let words: Vec<&str> = ru_name
                                .split(|c: char| !c.is_ascii_alphanumeric())
                                .collect();
                            for w in words {
                                if w.len() == 4
                                    && w.starts_with('U')
                                    && w.chars().all(|c| c.is_ascii_uppercase())
                                {
                                    clean_icao = w.to_string();
                                    break;
                                }
                            }
                            airport_ru_name = Some(ru_name);
                        }
                    }
                }
                continue;
            }

            // Parse tabular data row: comma, pipe, or tab delimited
            if let Some(row) = Self::parse_row_line(
                trimmed,
                &current_proc,
                current_rwy.as_deref(),
                current_nav_spec.as_deref(),
            )? {
                raw_rows.push((clean_icao.clone(), airport_ru_name.clone(), row));
            }
        }

        if raw_rows.is_empty() {
            return Ok(Vec::new());
        }

        // Group rows by airport, procedure identifier, and runway
        type GroupKey = (String, Option<String>, String, Option<String>);
        let mut grouped: HashMap<GroupKey, Vec<CaicaRawLegRow>> = HashMap::new();
        for (apt_icao, apt_name, row) in raw_rows {
            let key = (
                apt_icao,
                apt_name,
                row.procedure_ident.clone(),
                row.runway_transition.clone(),
            );
            grouped.entry(key).or_default().push(row);
        }

        let mut procedures = Vec::new();
        for ((apt_icao, apt_name, proc_id, rwy), mut legs) in grouped {
            legs.sort_by_key(|r| r.sequence_number);

            let kind = if proc_id.to_uppercase().contains("RNP")
                || proc_id.to_uppercase().contains("APCH")
                || proc_id.to_uppercase().contains("ILS")
                || proc_id.to_uppercase().contains("RNAV") && proc_id.contains("RW")
            {
                ProcedureKind::Approach
            } else if proc_id.ends_with('A')
                || proc_id.ends_with('B')
                || proc_id.ends_with('C')
                || proc_id.ends_with('D')
                || proc_id.ends_with('E')
                || proc_id.ends_with('F')
                || proc_id.ends_with('G')
                || proc_id.ends_with('W')
                || proc_id.ends_with('D')
            {
                if doc_title.to_uppercase().contains("STAR")
                    || doc_title.to_uppercase().contains("ПРИЛЕТ")
                    || default_kind == ProcedureKind::Star
                {
                    ProcedureKind::Star
                } else if doc_title.to_uppercase().contains("SID")
                    || doc_title.to_uppercase().contains("ВЫЛЕТ")
                    || default_kind == ProcedureKind::Sid
                {
                    ProcedureKind::Sid
                } else {
                    default_kind
                }
            } else {
                default_kind
            };

            procedures.push(CaicaParsedProcedure {
                airport_icao: apt_icao,
                airport_ru_name: apt_name,
                procedure_ident: proc_id,
                procedure_kind: kind,
                runway: rwy,
                transition: None,
                nav_spec: current_nav_spec.clone(),
                legs,
                source_doc_title: doc_title.to_string(),
            });
        }

        Ok(procedures)
    }

    /// Parse a single tabular text row from CAICA HTML publication.
    pub fn parse_row_line(
        line: &str,
        current_proc: &str,
        current_rwy: Option<&str>,
        current_nav_spec: Option<&str>,
    ) -> Result<Option<CaicaRawLegRow>> {
        let separator = if line.contains('|') {
            '|'
        } else if line.contains('\t') {
            '\t'
        } else if line.contains(';') {
            ';'
        } else {
            ','
        };

        let cols: Vec<&str> = line.split(separator).map(|s| s.trim()).collect();
        if cols.len() < 5 {
            return Ok(None);
        }

        // Header detection
        let c0_up = cols[0].to_uppercase();
        if c0_up.contains("SEQ")
            || c0_up.contains("№")
            || c0_up.contains("НОМЕР")
            || c0_up.contains("PATH")
            || c0_up.contains("УЧАСТОК")
        {
            return Ok(None);
        }

        let seq_num: u32 = cols[0].parse().unwrap_or(10);
        let path_terminator = cols[1].trim().to_uppercase();
        Self::validate_path_terminator(&path_terminator)?;

        let fix_ident = cols[2].trim().to_uppercase();
        let is_flyover = matches!(
            cols[3].trim().to_uppercase().as_str(),
            "Y" | "YES" | "ДА" | "1" | "*"
        );

        // Course: e.g. "63 (074.5)" or "243 (254.5)" or "074.5"
        let (course_mag, course_true) = Self::parse_dual_course(cols[4]);

        // Turn direction: L / R / -
        let turn_dir = if cols.len() > 5 && !cols[5].is_empty() {
            match cols[5].trim().to_uppercase().chars().next() {
                Some('L') | Some('Л') => Some('L'),
                Some('R') | Some('П') => Some('R'),
                _ => None,
            }
        } else {
            None
        };

        // Distance: e.g. "9.8 km" or "9.8" or "5.3 NM"
        let (dist_km, dist_nm) = if cols.len() > 6 && !cols[6].is_empty() {
            Self::parse_distance(cols[6])
        } else {
            (None, None)
        };

        // Altitude: e.g. "+1100", "-4000", "7000-5000", "FL150-FL140", "+FL230", "4000"
        let alt_constraint = if cols.len() > 7 && !cols[7].is_empty() {
            Self::parse_altitude_constraint(cols[7])
        } else {
            None
        };

        // Speed: e.g. "230", "-250", "250", "-205"
        let (speed_limit, is_speed_max) = if cols.len() > 8 && !cols[8].is_empty() {
            Self::parse_speed_constraint(cols[8])
        } else {
            (None, false)
        };

        // VPA / TCH: e.g. "VPA -3.00 TCH 50" or "-3.00 / 50"
        let (vpa, tch) = if cols.len() > 9 && !cols[9].is_empty() {
            Self::parse_vpa_tch(cols[9])
        } else {
            (None, None)
        };

        // Nav spec & AIRAC revision: e.g. "RNAV 1" | "2608"
        let nav_spec = if cols.len() > 10 && !cols[10].is_empty() {
            Some(cols[10].to_string())
        } else {
            current_nav_spec.map(|s| s.to_string())
        };

        let airac_row_cycle = if cols.len() > 11 && !cols[11].is_empty() {
            Some(cols[11].to_string())
        } else {
            None
        };

        // Coordinates if published in row
        let (fix_lat, fix_lon, raw_coords) = if cols.len() > 12 && !cols[12].is_empty() {
            let (lat, lon) = Self::parse_coordinates(cols[12]).unwrap_or((None, None));
            (lat, lon, Some(cols[12].to_string()))
        } else {
            (None, None, None)
        };

        Ok(Some(CaicaRawLegRow {
            sequence_number: seq_num,
            procedure_ident: current_proc.to_string(),
            runway_transition: current_rwy.map(|s| s.to_string()),
            enroute_transition: None,
            path_terminator,
            fix_ident,
            fix_lat,
            fix_lon,
            raw_coordinates_str: raw_coords,
            is_flyover,
            course_mag_deg: course_mag,
            course_true_deg: course_true,
            turn_direction: turn_dir,
            altitude_constraint: alt_constraint,
            speed_limit_kts: speed_limit,
            is_speed_max,
            distance_km: dist_km,
            distance_nm: dist_nm,
            vertical_angle_deg: vpa,
            tch_ft: tch,
            nav_spec,
            airac_row_cycle,
            remarks: None,
        }))
    }

    /// Strict path terminator validation. Fails closed on unknown terminators.
    pub fn validate_path_terminator(term: &str) -> Result<()> {
        match term {
            "IF" | "TF" | "CF" | "DF" | "CA" | "FA" | "VA" | "VI" | "VM" | "RF" | "HA" | "HF"
            | "HM" | "PI" | "FD" | "CD" | "CR" | "VR" => Ok(()),
            other => bail!("UNKNOWN / UNSUPPORTED PATH TERMINATOR: '{}'", other),
        }
    }

    /// Parse dual magnetic and true course: e.g. "63 (074.5)" -> (Some(63.0), Some(74.5)).
    pub fn parse_dual_course(s: &str) -> (Option<f64>, Option<f64>) {
        let clean = s.trim().replace('°', "");
        if clean.is_empty() || clean == "-" {
            return (None, None);
        }

        if let Some(open_paren) = clean.find('(') {
            let mag_part = clean[..open_paren].trim();
            let true_part = clean[open_paren + 1..].trim_matches(|c| c == ')' || c == ' ');
            let mag = mag_part.parse::<f64>().ok();
            let tru = true_part.parse::<f64>().ok();
            (mag, tru)
        } else {
            let val = clean.parse::<f64>().ok();
            (val, None)
        }
    }

    /// Parse Russian altitude constraint syntax:
    /// `+1100`, `-4000`, `7000-5000`, `FL150-FL140`, `+FL230`, `-FL190`, `4000`.
    pub fn parse_altitude_constraint(s: &str) -> Option<CaicaAltitudeConstraint> {
        let clean = s.trim().to_uppercase().replace(['М', 'M'], "");
        if clean.is_empty() || clean == "-" {
            return None;
        }

        // Check for range: e.g. "7000-5000" or "FL150-FL140"
        if let Some(dash_idx) = clean.find('-').filter(|&idx| idx > 0) {
            let p1 = &clean[..dash_idx].trim();
            let p2 = &clean[dash_idx + 1..].trim();
            if let (Some(a1), Some(a2)) = (Self::parse_alt_num(p1), Self::parse_alt_num(p2)) {
                let min_alt = a1.min(a2);
                let max_alt = a1.max(a2);
                return Some(CaicaAltitudeConstraint::Between(min_alt, max_alt));
            }
        }

        if let Some(stripped) = clean.strip_prefix('+') {
            let num = Self::parse_alt_num(stripped)?;
            Some(CaicaAltitudeConstraint::AtOrAbove(num))
        } else if let Some(stripped) = clean.strip_prefix('-') {
            let num = Self::parse_alt_num(stripped)?;
            Some(CaicaAltitudeConstraint::AtOrBelow(num))
        } else {
            let num = Self::parse_alt_num(&clean)?;
            Some(CaicaAltitudeConstraint::At(num))
        }
    }

    fn parse_alt_num(s: &str) -> Option<i32> {
        let clean = s.trim().to_uppercase();
        if let Some(stripped) = clean.strip_prefix("FL") {
            let fl: i32 = stripped.trim().parse().ok()?;
            Some(fl * 100)
        } else {
            clean.parse().ok()
        }
    }

    /// Parse Russian speed constraint syntax:
    /// `230`, `-250`, `250`, `-205`.
    pub fn parse_speed_constraint(s: &str) -> (Option<u32>, bool) {
        let clean = s.trim();
        if clean.is_empty() || clean == "-" {
            return (None, false);
        }

        if let Some(stripped) = clean.strip_prefix('-') {
            let num = stripped.trim().parse().ok();
            (num, true) // Max speed / At or below
        } else {
            let num = clean.parse().ok();
            (num, false)
        }
    }

    /// Parse distance with km to NM conversion.
    pub fn parse_distance(s: &str) -> (Option<f64>, Option<f64>) {
        let clean = s.trim().to_uppercase();
        if clean.is_empty() || clean == "-" {
            return (None, None);
        }

        if clean.contains("KM") || clean.contains("КМ") {
            let num_str = clean.replace("KM", "").replace("КМ", "");
            if let Ok(km) = num_str.trim().parse::<f64>() {
                let nm = km / 1.852;
                return (Some(km), Some(nm));
            }
        } else if clean.contains("NM") || clean.contains("НМ") {
            let num_str = clean.replace("NM", "").replace("НМ", "");
            if let Ok(nm) = num_str.trim().parse::<f64>() {
                let km = nm * 1.852;
                return (Some(km), Some(nm));
            }
        } else if let Ok(val) = clean.parse::<f64>() {
            // Default in CAICA tables is km
            let nm = val / 1.852;
            return (Some(val), Some(nm));
        }

        (None, None)
    }

    /// Parse VPA and TCH fields: e.g. "VPA -3.00 TCH 50" or "-3.00 / 50".
    pub fn parse_vpa_tch(s: &str) -> (Option<f64>, Option<f64>) {
        let clean = s.trim().to_uppercase();
        let mut vpa = None;
        let mut tch = None;

        for part in clean.split_whitespace() {
            if part.starts_with("VPA") {
                if let Ok(v) = part
                    .trim_start_matches("VPA")
                    .trim_matches(':')
                    .parse::<f64>()
                {
                    vpa = Some(v);
                }
            } else if part.starts_with("TCH") {
                if let Ok(t) = part
                    .trim_start_matches("TCH")
                    .trim_matches(':')
                    .parse::<f64>()
                {
                    tch = Some(t);
                }
            } else if let Ok(num) = part.parse::<f64>() {
                if num < 0.0 || (num > 0.0 && num <= 10.0) {
                    vpa = Some(num);
                } else if (30.0..=100.0).contains(&num) {
                    tch = Some(num);
                }
            }
        }

        (vpa, tch)
    }

    /// Parse coordinate string: `55 58 21.00 N 037 24 53.00 E` or `55°58'21.00"N 037°24'53.00"E`.
    pub fn parse_coordinates(s: &str) -> Result<(Option<f64>, Option<f64>)> {
        let clean = s.replace(['°', '\'', '"'], " ");
        let parts: Vec<&str> = clean.split_whitespace().collect();

        if parts.len() >= 8 {
            // Lat: parts[0..3] + parts[3] (hemisphere)
            let lat_deg: f64 = parts[0].parse().unwrap_or(0.0);
            let lat_min: f64 = parts[1].parse().unwrap_or(0.0);
            let lat_sec: f64 = parts[2].parse().unwrap_or(0.0);
            let lat_hem = parts[3];
            let mut lat = lat_deg + (lat_min / 60.0) + (lat_sec / 3600.0);
            if lat_hem.eq_ignore_ascii_case("S") || lat_hem == "Ю" {
                lat = -lat;
            }

            // Lon: parts[4..7] + parts[7] (hemisphere)
            let lon_deg: f64 = parts[4].parse().unwrap_or(0.0);
            let lon_min: f64 = parts[5].parse().unwrap_or(0.0);
            let lon_sec: f64 = parts[6].parse().unwrap_or(0.0);
            let lon_hem = parts[7];
            let mut lon = lon_deg + (lon_min / 60.0) + (lon_sec / 3600.0);
            if lon_hem.eq_ignore_ascii_case("W") || lon_hem == "З" {
                lon = -lon;
            }

            return Ok((Some(lat), Some(lon)));
        }

        Ok((None, None))
    }

    /// Ingest parsed procedures into the local WorldStore.
    pub fn ingest_parsed_procedures(
        &self,
        store: &mut WorldStore,
        procedures: &[CaicaParsedProcedure],
        effective_from: DateTime<Utc>,
        airac_cycle: Option<&str>,
        source_uri: &str,
    ) -> Result<crate::provider::IngestReport> {
        let content_hash = format!("{:x}", Sha256::digest(source_uri.as_bytes()));
        let snap_id = SourceSnapshotId(format!("caica_proc_{}", &content_hash[..8]));
        let snapshot = SourceSnapshot {
            id: snap_id.clone(),
            provider: self.provider_name.clone(),
            dataset: "CAICA_PROCEDURE_CODING".to_string(),
            provider_revision: airac_cycle.map(|s| s.to_string()),
            airac_cycle: airac_cycle.map(|s| s.to_string()),
            effective_from: Some(effective_from),
            effective_until: None,
            retrieved_at: Utc::now(),
            source_uri: source_uri.to_string(),
            content_sha256: content_hash.clone(),
            license_id: Some(self.license.clone()),
            license_notes: Some(
                "Official CAICA Russian Federation RNAV Procedure Coding Tables (Local AIP Vault)"
                    .to_string(),
            ),
            parser_version: "1.0.0".to_string(),
        };

        store.insert_source_snapshot(&snapshot)?;
        let mut report = crate::provider::IngestReport::new(
            &self.provider_name,
            "CAICA_PROCEDURE_CODING",
            &content_hash,
        );
        let temporal = TemporalValidity {
            valid_from: effective_from,
            valid_until: None,
            source_snapshot_id: snap_id.clone(),
        };

        store.transact(|conn| {
            for proc in procedures {
                let kind_char = match proc.procedure_kind {
                    ProcedureKind::Sid => 'D',
                    ProcedureKind::Star => 'E',
                    ProcedureKind::Approach => 'F',
                };

                for leg in &proc.legs {
                    let leg_id = ProcedureLegId(format!(
                        "{}:{}:{}:{}:{:03}",
                        self.namespace,
                        proc.airport_icao,
                        proc.procedure_ident,
                        proc.runway.as_deref().unwrap_or("NONE"),
                        leg.sequence_number
                    ));

                    let (alt_min, alt_max) = match &leg.altitude_constraint {
                        Some(CaicaAltitudeConstraint::At(a)) => (Some(*a as u32), Some(*a as u32)),
                        Some(CaicaAltitudeConstraint::AtOrAbove(a)) => (Some(*a as u32), None),
                        Some(CaicaAltitudeConstraint::AtOrBelow(a)) => (None, Some(*a as u32)),
                        Some(CaicaAltitudeConstraint::Between(min, max)) => {
                            (Some(*min as u32), Some(*max as u32))
                        }
                        None => (None, None),
                    };

                    let alt_desc = match &leg.altitude_constraint {
                        Some(CaicaAltitudeConstraint::At(_)) => Some(' '),
                        Some(CaicaAltitudeConstraint::AtOrAbove(_)) => Some('+'),
                        Some(CaicaAltitudeConstraint::AtOrBelow(_)) => Some('-'),
                        Some(CaicaAltitudeConstraint::Between(_, _)) => Some('B'),
                        None => None,
                    };

                    let canonical_leg = CanonicalProcedureLeg {
                        object_id: leg_id,
                        airport_ident: proc.airport_icao.clone(),
                        icao_code: "UU".to_string(),
                        procedure_kind: kind_char,
                        procedure_ident: proc.procedure_ident.clone(),
                        route_type: "0".to_string(),
                        transition_ident: proc.runway.clone().unwrap_or_default(),
                        sequence_number: leg.sequence_number,
                        fix_ident: leg.fix_ident.clone(),
                        fix_icao_code: "UU".to_string(),
                        fix_section: "EA".to_string(),
                        waypoint_description: if leg.is_flyover { "E" } else { " " }.to_string(),
                        turn_direction: leg.turn_direction,
                        rnp_nm: None,
                        path_terminator: leg.path_terminator.clone(),
                        recommended_navaid: None,
                        arc_radius_nm: None,
                        course_a_deg: leg.course_mag_deg,
                        distance_a_nm: leg.distance_nm,
                        course_b_deg: leg.course_true_deg,
                        distance_b_nm: leg.distance_nm,
                        altitude_descriptor: alt_desc,
                        altitude_1_ft: alt_min,
                        altitude_2_ft: alt_max,
                        speed_limit_kts: leg.speed_limit_kts,
                        course_c_deg: None,
                        vertical_angle_deg: leg.vertical_angle_deg,
                        msa_center_fix: None,
                        route_qualifiers: leg.nav_spec.clone().unwrap_or_default(),
                        raw: String::new(),
                        temporal: temporal.clone(),
                    };

                    openairac_store::insert_procedure_leg_conn(conn, &canonical_leg)?;
                    report.records_created += 1;
                }
            }
            Ok(())
        })?;

        Ok(report)
    }
}
