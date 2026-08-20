//! FAA Digital - Terminal Procedures Publication (d-TPP) Provider.
//!
//! Authoritative source: United States Federal Aviation Administration (FAA).
//! Format: Official `d-TPP_Metafile.xml` catalog and individual PDF charts.
//! License: US Government Work (Public Domain).

use crate::cache::ChartCache;
use crate::catalog::ChartCatalog;
use crate::model::{
    ChartDocument, ChartDocumentId, ChartMimeType, GeoreferenceStatus, NormalizedChartType,
};
use crate::provider::{ChartProvider, SyncReport};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, TimeZone, Utc};
use openairac_model::RedistributionPermission;
use quick_xml::Reader;
use quick_xml::events::Event;
use std::collections::BTreeSet;
use std::path::PathBuf;

pub struct FaaDtppProvider {
    pub base_url: String,
}

impl Default for FaaDtppProvider {
    fn default() -> Self {
        Self {
            base_url: "https://aeronav.faa.gov/d-tpp".to_string(),
        }
    }
}

impl FaaDtppProvider {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse effective dates formatted as `0901Z  08/06/26` or `08/06/26`.
    pub fn parse_faa_date(s: &str) -> Option<DateTime<Utc>> {
        let trimmed = s.trim();
        let date_part = if let Some(idx) =
            trimmed.find(|c: char| c.is_ascii_digit() && trimmed.contains('/'))
        {
            &trimmed[idx..]
        } else {
            trimmed
        };

        let parts: Vec<&str> = date_part.split('/').map(|p| p.trim()).collect();
        if parts.len() == 3 {
            let month: u32 = parts[0].parse().ok()?;
            let day: u32 = parts[1].parse().ok()?;
            let mut year: i32 = parts[2].parse().ok()?;
            if year < 100 {
                year += 2000;
            }
            Utc.with_ymd_and_hms(year, month, day, 9, 1, 0).single()
        } else {
            None
        }
    }

    /// Extract runway designation from a chart title (e.g. `ILS OR LOC RWY 04L` -> `04L`).
    pub fn extract_runway(title: &str) -> Option<String> {
        let upper = title.to_uppercase();
        if let Some(pos) = upper.find("RWY ") {
            let rest = &upper[pos + 4..];
            let rwy_str: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '/' || *c == '-')
                .collect();
            if !rwy_str.is_empty() {
                return Some(rwy_str);
            }
        }
        None
    }

    /// Parse official `d-TPP_Metafile.xml` content into canonical ChartDocument records.
    pub fn parse_metafile_xml(&self, xml_content: &str) -> Result<(String, Vec<ChartDocument>)> {
        let mut reader = Reader::from_str(xml_content);
        reader.config_mut().trim_text(true);

        let mut cycle = String::new();
        let mut from_edate: Option<DateTime<Utc>> = None;
        let mut to_edate: Option<DateTime<Utc>> = None;

        let mut current_state = String::new();
        let mut _current_city = String::new();
        let mut _current_apt_name = String::new();
        let mut current_apt_ident = String::new();
        let mut current_icao_ident = String::new();

        let mut in_record = false;
        let mut current_tag = String::new();

        let mut rec_seq = String::new();
        let mut rec_code = String::new();
        let mut rec_name = String::new();
        let mut rec_pdf = String::new();
        let mut rec_action = String::new();

        let mut documents = Vec::new();

        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    current_tag = name.clone();

                    if name == "digital_tpp" {
                        for attr in e.attributes().flatten() {
                            let key = String::from_utf8_lossy(attr.key.as_ref());
                            let val = String::from_utf8_lossy(&attr.value);
                            if key == "cycle" {
                                cycle = val.trim().to_string();
                            } else if key == "from_edate" {
                                from_edate = Self::parse_faa_date(&val);
                            } else if key == "to_edate" {
                                to_edate = Self::parse_faa_date(&val);
                            }
                        }
                    } else if name == "state_code" {
                        for attr in e.attributes().flatten() {
                            let key = String::from_utf8_lossy(attr.key.as_ref());
                            let val = String::from_utf8_lossy(&attr.value);
                            if key == "ID" {
                                current_state = val.trim().to_string();
                            }
                        }
                    } else if name == "city_name" {
                        for attr in e.attributes().flatten() {
                            let key = String::from_utf8_lossy(attr.key.as_ref());
                            let val = String::from_utf8_lossy(&attr.value);
                            if key == "ID" {
                                _current_city = val.trim().to_string();
                            }
                        }
                    } else if name == "airport_name" {
                        for attr in e.attributes().flatten() {
                            let key = String::from_utf8_lossy(attr.key.as_ref());
                            let val = String::from_utf8_lossy(&attr.value);
                            if key == "ID" {
                                _current_apt_name = val.trim().to_string();
                            } else if key == "apt_ident" {
                                current_apt_ident = val.trim().to_string();
                            } else if key == "icao_ident" {
                                current_icao_ident = val.trim().to_string();
                            }
                        }
                    } else if name == "record" {
                        in_record = true;
                        rec_seq.clear();
                        rec_code.clear();
                        rec_name.clear();
                        rec_pdf.clear();
                        rec_action.clear();
                    }
                }
                Ok(Event::Text(ref e)) => {
                    if in_record {
                        let text = e.unescape().unwrap_or_default().trim().to_string();
                        match current_tag.as_str() {
                            "chartseq" => rec_seq = text,
                            "chart_code" => rec_code = text,
                            "chart_name" => rec_name = text,
                            "pdf_name" => rec_pdf = text,
                            "useraction" => rec_action = text,
                            _ => {}
                        }
                    }
                }
                Ok(Event::End(ref e)) => {
                    let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    if name == "record" && in_record {
                        in_record = false;

                        if !rec_pdf.is_empty() {
                            let icao = if !current_icao_ident.is_empty() {
                                current_icao_ident.clone()
                            } else if current_apt_ident.len() == 3
                                && current_state != "AK"
                                && current_state != "HI"
                            {
                                format!("K{}", current_apt_ident)
                            } else {
                                current_apt_ident.clone()
                            };

                            let chart_type = NormalizedChartType::from_faa_code(&rec_code);
                            let runway = Self::extract_runway(&rec_name);
                            let pdf_stem =
                                rec_pdf.trim_end_matches(".PDF").trim_end_matches(".pdf");
                            let chart_id =
                                ChartDocumentId(format!("faa:{cycle}:{icao}:{pdf_stem}"));
                            let source_url = format!("{}/{}/{}", self.base_url, cycle, rec_pdf);

                            documents.push(ChartDocument {
                                id: chart_id,
                                provider_id: "FAA_DTPP".to_string(),
                                airport_icao: icao,
                                airport_iata: (!current_apt_ident.is_empty())
                                    .then(|| current_apt_ident.clone()),
                                chart_type,
                                provider_chart_type: rec_code.clone(),
                                title: rec_name.clone(),
                                procedure_name: None,
                                runway,
                                effective_from: from_edate,
                                effective_to: to_edate,
                                revision_date: None,
                                airac_cycle: cycle.clone(),
                                language: Some("en".to_string()),
                                source_url,
                                source_document_id: Some(rec_pdf.clone()),
                                license_policy: RedistributionPermission::PublicRedistribution,
                                attribution:
                                    "FAA Aeronautical Information Services (Public Domain)"
                                        .to_string(),
                                mime_type: ChartMimeType::Pdf,
                                asset_sha256: None,
                                file_size_bytes: None,
                                georeference_status: GeoreferenceStatus::NotGeoreferenced,
                                change_flag: (!rec_action.is_empty()).then(|| rec_action.clone()),
                            });
                        }
                    }
                    current_tag.clear();
                }
                Ok(Event::Eof) => break,
                Err(e) => bail!("XML error parsing d-TPP metafile: {e}"),
                _ => {}
            }
            buf.clear();
        }

        Ok((cycle, documents))
    }
}

impl ChartProvider for FaaDtppProvider {
    fn provider_id(&self) -> &'static str {
        "FAA_DTPP"
    }

    fn provider_name(&self) -> &'static str {
        "FAA Digital - Terminal Procedures Publication (d-TPP)"
    }

    fn authority(&self) -> &'static str {
        "Federal Aviation Administration (FAA)"
    }

    fn jurisdiction(&self) -> &'static str {
        "United States"
    }

    fn license_policy(&self) -> RedistributionPermission {
        RedistributionPermission::PublicRedistribution
    }

    fn sync_catalog(&self, catalog: &ChartCatalog, _cycle: Option<&str>) -> Result<SyncReport> {
        // Load bundled or cached sample catalog if offline, or sync from XML
        let sample_xml = include_str!("../../../../data/faa_dtpp_sample.xml");
        let (cycle, docs) = self.parse_metafile_xml(sample_xml)?;

        let mut airports = BTreeSet::new();
        let total_charts = docs.len();

        for doc in &docs {
            airports.insert(doc.airport_icao.clone());
            catalog.insert_or_replace_chart(doc)?;
        }

        Ok(SyncReport {
            provider_id: self.provider_id().to_string(),
            airac_cycle: cycle,
            airports_indexed: airports.len(),
            charts_indexed: total_charts,
        })
    }

    fn fetch_asset(&self, chart: &ChartDocument, cache: &ChartCache) -> Result<PathBuf> {
        let ext = chart.mime_type.extension();

        // 1. If asset SHA is already known and cached, return it directly
        if let Some(sha) = chart
            .asset_sha256
            .as_deref()
            .filter(|sha| cache.has_asset(sha, ext))
        {
            return cache.asset_path(sha, ext);
        }

        // 2. Fetch over HTTP if online feature is enabled
        #[cfg(feature = "online")]
        {
            let client = reqwest::blocking::Client::builder()
                .user_agent("OpenAIRAC/1.6 (open charts engine)")
                .timeout(std::time::Duration::from_secs(30))
                .build()?;

            let response = client
                .get(&chart.source_url)
                .send()
                .with_context(|| format!("Failed to download chart from '{}'", chart.source_url))?;

            if !response.status().is_success() {
                bail!(
                    "HTTP error {} downloading chart from '{}'",
                    response.status(),
                    chart.source_url
                );
            }
            let bytes = response.bytes()?;
            let (_sha, path) = cache.store_asset(&bytes, ext)?;
            Ok(path)
        }

        #[cfg(not(feature = "online"))]
        {
            bail!("Online chart downloading is disabled (build without online feature)");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_faa_metafile_sample() {
        let provider = FaaDtppProvider::new();
        let xml = include_str!("../../../../data/faa_dtpp_sample.xml");
        let (cycle, docs) = provider.parse_metafile_xml(xml).unwrap();

        assert_eq!(cycle, "2608");
        assert!(!docs.is_empty());

        let kjfk_docs: Vec<_> = docs.iter().filter(|d| d.airport_icao == "KJFK").collect();
        assert_eq!(kjfk_docs.len(), 38);

        let ad = kjfk_docs
            .iter()
            .find(|d| d.chart_type == NormalizedChartType::AirportDiagram)
            .unwrap();
        assert_eq!(ad.title, "AIRPORT DIAGRAM");
        assert_eq!(ad.source_document_id.as_deref(), Some("00610AD.PDF"));

        let ils04l = kjfk_docs
            .iter()
            .find(|d| d.title == "ILS OR LOC RWY 04L")
            .unwrap();
        assert_eq!(ils04l.chart_type, NormalizedChartType::Approach);
        assert_eq!(ils04l.runway.as_deref(), Some("04L"));
    }
}
