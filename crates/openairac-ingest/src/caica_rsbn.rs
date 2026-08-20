//! RSBN (Радиотехническая система ближней навигации) Radio Navigation Station Parser.
//!
//! Provides canonical ingestion and data representation for Soviet / Russian RSBN ground stations
//! used for radio navigation by Tu-154, Il-76, Il-96, An-24, Tu-134, Yak-40, and other domestic aircraft.
//!
//! Fields:
//! - Channel: 1..88 discrete frequency/code channel
//! - Station Name and Ident (Cyrillic and Latin transliteration)
//! - Coordinates (lat, lon)
//! - Elevation (ft / m)
//! - Range (km / NM)
//! - Declination / Magnetic variation
//! - Associated aerodrome or FIR

use anyhow::Result;
use chrono::{DateTime, Utc};
use openairac_model::{
    CanonicalNavaid, FrequencyKhz, NavaidId, NavaidKind, SourceSnapshot, SourceSnapshotId,
    TemporalValidity,
};
use openairac_store::WorldStore;
use sha2::{Digest, Sha256};

/// Parsed RSBN Station record.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedRsbnStation {
    pub ident: String,
    pub name: String,
    pub channel: u8,
    pub latitude: f64,
    pub longitude: f64,
    pub elevation_ft: Option<i32>,
    pub range_km: Option<f64>,
    pub associated_airport: Option<String>,
    pub magnetic_variation_deg: Option<f64>,
}

/// Ingestion provider for RSBN stations in the Local AIP Vault.
pub struct CaicaRsbnProvider {
    pub provider_name: String,
    pub namespace: String,
    pub license: String,
}

impl Default for CaicaRsbnProvider {
    fn default() -> Self {
        Self::new("RU_CAICA_RADIONAV", "caica_rsbn", "CAICA-TermsOfUse")
    }
}

impl CaicaRsbnProvider {
    pub fn new(provider_name: &str, namespace: &str, license: &str) -> Self {
        Self {
            provider_name: provider_name.to_string(),
            namespace: namespace.to_string(),
            license: license.to_string(),
        }
    }

    /// Parse RSBN table text or CSV snippet.
    ///
    /// Expected format:
    /// `IDENT,NAME,CHANNEL,LAT,LON,ELEV_FT,RANGE_KM,ASSOCIATED_APT,MAG_VAR`
    pub fn parse_rsbn_table(text: &str) -> Result<Vec<ParsedRsbnStation>> {
        let mut stations = Vec::new();

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
                continue;
            }

            let parts: Vec<&str> = trimmed.split(',').map(|s| s.trim()).collect();
            if parts.len() < 5 {
                continue;
            }

            // Skip header line
            if parts[0].eq_ignore_ascii_case("IDENT") || parts[0].contains("КАНАЛ") {
                continue;
            }

            let ident = parts[0].to_string();
            let name = parts[1].to_string();
            let channel: u8 = parts[2].parse().unwrap_or(1);
            let lat: f64 = parts[3].parse()?;
            let lon: f64 = parts[4].parse()?;

            let elev: Option<i32> = if parts.len() > 5 && !parts[5].is_empty() {
                parts[5].parse().ok()
            } else {
                None
            };

            let range: Option<f64> = if parts.len() > 6 && !parts[6].is_empty() {
                parts[6].parse().ok()
            } else {
                None
            };

            let apt: Option<String> = if parts.len() > 7 && !parts[7].is_empty() {
                Some(parts[7].to_string())
            } else {
                None
            };

            let mag_var: Option<f64> = if parts.len() > 8 && !parts[8].is_empty() {
                parts[8].parse().ok()
            } else {
                None
            };

            stations.push(ParsedRsbnStation {
                ident,
                name,
                channel,
                latitude: lat,
                longitude: lon,
                elevation_ft: elev,
                range_km: range,
                associated_airport: apt,
                magnetic_variation_deg: mag_var,
            });
        }

        Ok(stations)
    }

    /// Ingest RSBN stations into WorldStore as both typed RSBN records and CanonicalNavaids.
    pub fn ingest_rsbn_stations(
        &self,
        store: &mut WorldStore,
        stations: &[ParsedRsbnStation],
        effective_from: DateTime<Utc>,
        airac_cycle: Option<&str>,
        source_uri: &str,
    ) -> Result<crate::provider::IngestReport> {
        let content_hash = format!("{:x}", Sha256::digest(source_uri.as_bytes()));
        let snap_id = SourceSnapshotId(format!("caica_rsbn_{}", &content_hash[..8]));
        let snapshot = SourceSnapshot {
            id: snap_id.clone(),
            provider: self.provider_name.clone(),
            dataset: "CAICA_RSBN_RADIONAV".to_string(),
            provider_revision: airac_cycle.map(|s| s.to_string()),
            airac_cycle: airac_cycle.map(|s| s.to_string()),
            effective_from: Some(effective_from),
            effective_until: None,
            retrieved_at: Utc::now(),
            source_uri: source_uri.to_string(),
            content_sha256: content_hash.clone(),
            license_id: Some(self.license.clone()),
            license_notes: Some(
                "Official Russian Federation RSBN Ground Navigation Stations (Local AIP Vault)"
                    .to_string(),
            ),
            parser_version: "1.0.0".to_string(),
        };

        store.insert_source_snapshot(&snapshot)?;
        let mut report = crate::provider::IngestReport::new(
            &self.provider_name,
            "CAICA_RSBN_RADIONAV",
            &content_hash,
        );

        for st in stations {
            let navaid_id = NavaidId(format!(
                "{}_{}_CH{:02}",
                self.namespace, st.ident, st.channel
            ));

            // Frequency for RSBN channel representation: e.g. 1000 + channel (in kHz)
            let freq_khz = FrequencyKhz(100_000 + (st.channel as u32 * 100));

            let navaid = CanonicalNavaid {
                object_id: navaid_id.clone(),
                ident: st.ident.clone(),
                name: st.name.clone(),
                kind: NavaidKind::Rsbn,
                frequency: freq_khz,
                latitude: st.latitude,
                longitude: st.longitude,
                elevation_ft: st.elevation_ft,
                region_code: Some("RU".to_string()),
                associated_airport: st.associated_airport.clone(),
                magnetic_variation_deg: st.magnetic_variation_deg,
                slaved_variation_deg: None,
                service_volume_nm: st.range_km.map(|k| (k / 1.852).round() as u16),
                dme_paired: false,
                associated_runway: None,
                localizer_bearing_true_deg: None,
                localizer_bearing_mag_deg: None,
                glideslope_angle_deg: None,
                temporal: TemporalValidity {
                    valid_from: effective_from,
                    valid_until: None,
                    source_snapshot_id: snap_id.clone(),
                },
            };

            let write = store.insert_navaid(&navaid)?;
            report.record_write(write);
        }

        Ok(report)
    }
}
