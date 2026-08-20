//! Procedure-to-Chart Association Engine.
//!
//! Associates canonical navigation procedures with published chart documents
//! via deterministic rule matching with explicit confidence levels (Exact, Likely, Ambiguous, Unmatched).
//!
//! Safety invariant: Association creates a reference link only. It NEVER converts
//! charts into navdata or generates synthetic navdata procedures.

use crate::model::{AssociationConfidence, ChartAssociation, ChartDocument, NormalizedChartType};

pub struct AssociationEngine;

impl AssociationEngine {
    /// Associate a canonical procedure with candidate airport charts.
    pub fn match_procedure_to_charts(
        airport_icao: &str,
        procedure_kind: char, // 'D' = SID, 'E' = STAR, 'F' = Approach
        procedure_ident: &str,
        runway_hint: Option<&str>,
        candidate_charts: &[ChartDocument],
    ) -> Vec<ChartAssociation> {
        let mut associations = Vec::new();
        let clean_proc = procedure_ident.trim().to_uppercase();
        let clean_rwy = runway_hint
            .map(|r| r.trim().to_uppercase())
            .or_else(|| Self::extract_runway_from_proc_ident(&clean_proc));

        for chart in candidate_charts {
            if chart.airport_icao.to_uppercase() != airport_icao.to_uppercase() {
                continue;
            }

            let (matched, confidence, reason) = match procedure_kind {
                'F' => Self::match_approach(&clean_proc, clean_rwy.as_deref(), chart),
                'D' => Self::match_departure(&clean_proc, clean_rwy.as_deref(), chart),
                'E' => Self::match_arrival(&clean_proc, clean_rwy.as_deref(), chart),
                _ => (false, AssociationConfidence::Unmatched, String::new()),
            };

            if matched {
                associations.push(ChartAssociation {
                    procedure_ident: procedure_ident.to_string(),
                    procedure_kind,
                    airport_icao: airport_icao.to_string(),
                    runway: chart.runway.clone().or_else(|| clean_rwy.clone()),
                    chart_id: chart.id.clone(),
                    confidence,
                    match_reason: reason,
                });
            }
        }

        // Detect ambiguity if multiple charts matched with Exact confidence
        let exact_count = associations
            .iter()
            .filter(|a| a.confidence == AssociationConfidence::Exact)
            .count();
        if exact_count > 1 {
            for a in &mut associations {
                if a.confidence == AssociationConfidence::Exact {
                    a.confidence = AssociationConfidence::Ambiguous;
                    a.match_reason =
                        format!("Multiple qualifying charts ({exact_count}) match procedure");
                }
            }
        }

        associations
    }

    fn match_approach(
        proc_ident: &str,
        runway_hint: Option<&str>,
        chart: &ChartDocument,
    ) -> (bool, AssociationConfidence, String) {
        if chart.chart_type != NormalizedChartType::Approach
            && chart.chart_type != NormalizedChartType::ApproachVisual
        {
            return (false, AssociationConfidence::Unmatched, String::new());
        }

        let title_upper = chart.title.to_uppercase();

        // 1. Runway matching if available
        let rwy_match = match (runway_hint, chart.runway.as_deref()) {
            (Some(r1), Some(r2)) => {
                r1 == r2 || Self::normalize_runway(r1) == Self::normalize_runway(r2)
            }
            (Some(r1), None) => {
                let norm = Self::normalize_runway(r1);
                title_upper.contains(&format!("RWY {norm}"))
                    || title_upper.contains(&format!("RWY {r1}"))
            }
            _ => true,
        };

        if !rwy_match {
            return (false, AssociationConfidence::Unmatched, String::new());
        }

        // 2. Type matching (ILS, RNAV, VOR, NDB, LOC)
        let first_char = proc_ident.chars().next().unwrap_or(' ');
        let is_ils = first_char == 'I' || proc_ident.starts_with("ILS");
        let is_rnav = first_char == 'R' || proc_ident.starts_with("RNAV");
        let is_vor = first_char == 'V' || proc_ident.starts_with("VOR");
        let is_ndb = first_char == 'N' || proc_ident.starts_with("NDB");

        if is_ils {
            if title_upper.contains("ILS") || title_upper.contains("LOC") {
                return (
                    true,
                    AssociationConfidence::Exact,
                    format!("Exact ILS/LOC approach match for procedure '{proc_ident}'"),
                );
            } else {
                return (false, AssociationConfidence::Unmatched, String::new());
            }
        }

        if is_rnav {
            if title_upper.contains("RNAV") || title_upper.contains("GPS") {
                return (
                    true,
                    AssociationConfidence::Exact,
                    format!("Exact RNAV (GPS) approach match for procedure '{proc_ident}'"),
                );
            } else {
                return (false, AssociationConfidence::Unmatched, String::new());
            }
        }

        if is_vor {
            if title_upper.contains("VOR") {
                return (
                    true,
                    AssociationConfidence::Exact,
                    format!("Exact VOR approach match for procedure '{proc_ident}'"),
                );
            } else {
                return (false, AssociationConfidence::Unmatched, String::new());
            }
        }

        if is_ndb {
            if title_upper.contains("NDB") {
                return (
                    true,
                    AssociationConfidence::Exact,
                    format!("Exact NDB approach match for procedure '{proc_ident}'"),
                );
            } else {
                return (false, AssociationConfidence::Unmatched, String::new());
            }
        }

        if rwy_match && (runway_hint.is_some() || chart.runway.is_some()) {
            return (
                true,
                AssociationConfidence::Likely,
                "Likely approach match for runway".to_string(),
            );
        }
        (false, AssociationConfidence::Unmatched, String::new())
    }

    fn match_departure(
        proc_ident: &str,
        _runway_hint: Option<&str>,
        chart: &ChartDocument,
    ) -> (bool, AssociationConfidence, String) {
        if chart.chart_type != NormalizedChartType::Sid {
            return (false, AssociationConfidence::Unmatched, String::new());
        }

        let title_upper = chart.title.to_uppercase();
        let name_prefix = proc_ident.trim_end_matches(char::is_numeric);

        if !name_prefix.is_empty() && title_upper.contains(name_prefix) {
            return (
                true,
                AssociationConfidence::Exact,
                format!("Exact SID departure name match on '{name_prefix}'"),
            );
        }

        (false, AssociationConfidence::Unmatched, String::new())
    }

    fn match_arrival(
        proc_ident: &str,
        _runway_hint: Option<&str>,
        chart: &ChartDocument,
    ) -> (bool, AssociationConfidence, String) {
        if chart.chart_type != NormalizedChartType::Star {
            return (false, AssociationConfidence::Unmatched, String::new());
        }

        let title_upper = chart.title.to_uppercase();
        let name_prefix = proc_ident.trim_end_matches(char::is_numeric);

        if !name_prefix.is_empty() && title_upper.contains(name_prefix) {
            return (
                true,
                AssociationConfidence::Exact,
                format!("Exact STAR arrival name match on '{name_prefix}'"),
            );
        }

        (false, AssociationConfidence::Unmatched, String::new())
    }

    fn normalize_runway(rwy: &str) -> String {
        rwy.trim_start_matches('0').to_string()
    }

    fn extract_runway_from_proc_ident(proc_ident: &str) -> Option<String> {
        if proc_ident.len() >= 2 {
            let candidate = &proc_ident[1..];
            if candidate
                .chars()
                .next()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false)
            {
                return Some(candidate.to_string());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use openairac_model::RedistributionPermission;

    fn sample_doc(
        id: &str,
        icao: &str,
        chart_type: NormalizedChartType,
        title: &str,
        rwy: Option<&str>,
    ) -> ChartDocument {
        ChartDocument {
            id: ChartDocumentId(id.to_string()),
            provider_id: "FAA_DTPP".to_string(),
            airport_icao: icao.to_string(),
            airport_iata: None,
            chart_type,
            provider_chart_type: "IAP".to_string(),
            title: title.to_string(),
            procedure_name: None,
            runway: rwy.map(|s| s.to_string()),
            effective_from: None,
            effective_to: None,
            revision_date: None,
            airac_cycle: "2608".to_string(),
            language: Some("en".to_string()),
            source_url: "https://aeronav.faa.gov/".to_string(),
            source_document_id: None,
            license_policy: RedistributionPermission::PublicRedistribution,
            attribution: "FAA".to_string(),
            mime_type: ChartMimeType::Pdf,
            asset_sha256: None,
            file_size_bytes: None,
            georeference_status: GeoreferenceStatus::NotGeoreferenced,
            change_flag: None,
        }
    }

    #[test]
    fn test_match_kjfk_ils04l() {
        let docs = vec![
            sample_doc(
                "doc-1",
                "KJFK",
                NormalizedChartType::Approach,
                "ILS OR LOC RWY 04L",
                Some("04L"),
            ),
            sample_doc(
                "doc-2",
                "KJFK",
                NormalizedChartType::Approach,
                "ILS OR LOC RWY 22R",
                Some("22R"),
            ),
            sample_doc(
                "doc-3",
                "KJFK",
                NormalizedChartType::Sid,
                "JFK TWO DEPARTURE",
                None,
            ),
        ];

        let matches =
            AssociationEngine::match_procedure_to_charts("KJFK", 'F', "I04L", Some("04L"), &docs);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].chart_id.0, "doc-1");
        assert_eq!(matches[0].confidence, AssociationConfidence::Exact);
    }

    #[test]
    fn test_match_kjfk_sid() {
        let docs = vec![sample_doc(
            "doc-sid",
            "KJFK",
            NormalizedChartType::Sid,
            "SKORR SIX DEPARTURE (RNAV)",
            None,
        )];

        let matches =
            AssociationEngine::match_procedure_to_charts("KJFK", 'D', "SKORR6", None, &docs);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].chart_id.0, "doc-sid");
        assert_eq!(matches[0].confidence, AssociationConfidence::Exact);
    }
}
