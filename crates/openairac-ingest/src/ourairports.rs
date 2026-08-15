use crate::provider::{DataProvider, FetchedDataset, IngestReport, decimal_year, fetch_url};
use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use openairac_magnetic::analyze_runway_magnetic_drift;
use openairac_model::*;
use openairac_store::{
    WorldStore, insert_airport_conn, insert_navaid_conn, insert_runway_conn,
    insert_source_snapshot_conn, insert_world_revision_conn,
};
use serde::Deserialize;

const AIRPORTS_URL: &str = "https://davidmegginson.github.io/ourairports-data/airports.csv";
const RUNWAYS_URL: &str = "https://davidmegginson.github.io/ourairports-data/runways.csv";
const NAVAIDS_URL: &str = "https://davidmegginson.github.io/ourairports-data/navaids.csv";
const LICENSE_ID: &str = "CC0-1.0";

/// OurAirports raw CSV record structures. Column names match the published
/// OurAirports CSV headers (https://ourairports.com/data/).
#[derive(Debug, Deserialize)]
pub struct OurAirportsAirportRecord {
    pub id: String,
    pub ident: String,
    #[serde(rename = "type")]
    pub airport_type: Option<String>,
    pub name: String,
    pub latitude_deg: Option<f64>,
    pub longitude_deg: Option<f64>,
    pub elevation_ft: Option<f64>,
    pub iso_country: Option<String>,
    pub municipality: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OurAirportsRunwayRecord {
    pub id: String,
    pub airport_ref: String,
    pub airport_ident: String,
    pub length_ft: Option<u32>,
    pub width_ft: Option<u32>,
    pub surface: Option<String>,
    pub le_ident: String,
    pub le_latitude_deg: Option<f64>,
    pub le_longitude_deg: Option<f64>,
    pub le_elevation_ft: Option<f64>,
    #[serde(rename = "le_heading_degT")]
    pub le_heading_deg_t: Option<f64>,
    pub he_ident: Option<String>,
    pub he_latitude_deg: Option<f64>,
    pub he_longitude_deg: Option<f64>,
    pub he_elevation_ft: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct OurAirportsNavaidRecord {
    pub id: String,
    pub filename: Option<String>,
    pub ident: String,
    pub name: String,
    #[serde(rename = "type")]
    pub navaid_type: String,
    pub frequency_khz: Option<u32>,
    pub latitude_deg: Option<f64>,
    pub longitude_deg: Option<f64>,
    pub elevation_ft: Option<i32>,
    pub associated_airport: Option<String>,
    pub magnetic_variation_deg: Option<f64>,
}

fn valid_lat(l: Option<f64>) -> Option<f64> {
    l.filter(|v| (-90.0..=90.0).contains(v))
}

/// Valid frequency band per navaid kind (kHz). Mirrors the store's
/// structural validation so bad data never reaches the canonical store.
fn frequency_in_range(kind: NavaidKind, freq_khz: u32) -> bool {
    let (lo, hi) = match kind {
        NavaidKind::Ndb => (190, 1800),
        NavaidKind::Vor | NavaidKind::Vordme | NavaidKind::Vortac => (108_000, 118_000),
        NavaidKind::IlsLocalizer => (108_000, 112_000),
        NavaidKind::IlsGlidepath => (328_000, 336_000),
        NavaidKind::Dme => (108_000, 118_000),
        NavaidKind::Tacan => (108_000, 136_000),
    };
    freq_khz >= lo && freq_khz <= hi
}

fn valid_lon(l: Option<f64>) -> Option<f64> {
    l.filter(|v| (-180.0..=180.0).contains(v))
}

/// Network fetcher for the three OurAirports datasets.
pub struct OurAirportsProvider;

impl DataProvider for OurAirportsProvider {
    fn name(&self) -> &'static str {
        "OurAirports"
    }

    fn fetch(&self, dataset: &str) -> Result<FetchedDataset> {
        let url = match dataset {
            "airports" => AIRPORTS_URL,
            "runways" => RUNWAYS_URL,
            "navaids" => NAVAIDS_URL,
            other => return Err(anyhow!("Unknown OurAirports dataset: {other}")),
        };
        fetch_url("OurAirports", dataset, url, Utc::now())
    }

    fn parse_and_ingest(
        &self,
        dataset: &FetchedDataset,
        store: &mut WorldStore,
    ) -> Result<IngestReport> {
        OurAirportsImporter::ingest_dataset(dataset, store)
    }
}

pub struct OurAirportsImporter;

impl OurAirportsImporter {
    pub fn parse_airports(
        csv_content: &str,
        snapshot_id: &SourceSnapshotId,
        checksum: &str,
        valid_from: DateTime<Utc>,
    ) -> (Vec<CanonicalAirport>, IngestReport) {
        let mut report = IngestReport::new("OurAirports", "airports", checksum);
        let mut reader = csv::ReaderBuilder::new()
            .flexible(true)
            .from_reader(csv_content.as_bytes());

        let mut airports = Vec::new();
        for result in reader.deserialize::<OurAirportsAirportRecord>() {
            report.record_seen();
            let rec = match result {
                Ok(rec) => rec,
                Err(err) => {
                    report.record_rejected(format!("Failed to parse airport CSV row: {err}"));
                    continue;
                }
            };
            report.record_parsed();

            if rec.ident.trim().is_empty() {
                report.record_rejected(format!("Airport {}: empty identifier", rec.id));
                continue;
            }
            let Some(lat) = valid_lat(rec.latitude_deg) else {
                report.record_rejected(format!("Airport {}: invalid latitude", rec.ident));
                continue;
            };
            let Some(lon) = valid_lon(rec.longitude_deg) else {
                report.record_rejected(format!("Airport {}: invalid longitude", rec.ident));
                continue;
            };

            let airport = CanonicalAirport {
                id: AirportId(format!("ourairports:{}", rec.id)),
                ident: rec.ident,
                name: rec.name,
                airport_type: rec.airport_type.unwrap_or_default(),
                latitude: lat,
                longitude: lon,
                elevation_ft: rec.elevation_ft,
                iso_country: rec.iso_country,
                municipality: rec.municipality,
                runways: Vec::new(),
                temporal: TemporalValidity {
                    valid_from,
                    valid_until: None,
                    source_snapshot_id: snapshot_id.clone(),
                },
            };
            airports.push(airport);
        }

        (airports, report)
    }

    pub fn parse_runways(
        csv_content: &str,
        snapshot_id: &SourceSnapshotId,
        checksum: &str,
        year: f64,
        valid_from: DateTime<Utc>,
    ) -> (Vec<CanonicalRunway>, IngestReport) {
        let mut report = IngestReport::new("OurAirports", "runways", checksum);
        let mut reader = csv::ReaderBuilder::new()
            .flexible(true)
            .from_reader(csv_content.as_bytes());

        let mut runways = Vec::new();
        for result in reader.deserialize::<OurAirportsRunwayRecord>() {
            report.record_seen();
            let rec = match result {
                Ok(rec) => rec,
                Err(err) => {
                    report.record_rejected(format!("Failed to parse runway CSV row: {err}"));
                    continue;
                }
            };
            report.record_parsed();

            let Some(le_lat) = valid_lat(rec.le_latitude_deg) else {
                report.record_rejected(format!("Runway {}: invalid LE latitude", rec.id));
                continue;
            };
            let Some(le_lon) = valid_lon(rec.le_longitude_deg) else {
                report.record_rejected(format!("Runway {}: invalid LE longitude", rec.id));
                continue;
            };
            let Some(he_ident) = rec.he_ident.filter(|h| !h.trim().is_empty()) else {
                report.record_quarantined(format!("Runway {}: missing HE identifier", rec.id));
                continue;
            };
            let Some(he_lat) = valid_lat(rec.he_latitude_deg) else {
                report
                    .record_quarantined(format!("Runway {}: missing/invalid HE latitude", rec.id));
                continue;
            };
            let Some(he_lon) = valid_lon(rec.he_longitude_deg) else {
                report
                    .record_quarantined(format!("Runway {}: missing/invalid HE longitude", rec.id));
                continue;
            };
            let (Some(length_ft), Some(width_ft)) = (rec.length_ft, rec.width_ft) else {
                report.record_rejected(format!("Runway {}: missing length/width", rec.id));
                continue;
            };

            // Magnetic drift analysis requires a published true heading.
            // Without it we keep the runway (position/designator data is
            // still valid) and flag the missing analysis.
            let (computed_designator, heading) = match rec.le_heading_deg_t {
                Some(heading) if heading > 0.0 => match analyze_runway_magnetic_drift(
                    &rec.le_ident.clone(),
                    heading,
                    le_lat,
                    le_lon,
                    year,
                ) {
                    Ok(analysis) => (Some(analysis.computed_magnetic_designator), Some(heading)),
                    Err(err) => {
                        report
                            .warnings
                            .push(format!("Runway {}: WMM analysis failed ({err})", rec.id));
                        (None, Some(heading))
                    }
                },
                _ => {
                    report
                        .warnings
                        .push(format!("Runway {}: no published heading", rec.id));
                    (None, None)
                }
            };

            let runway = CanonicalRunway {
                id: RunwayId(format!("ourairports:{}", rec.id)),
                airport_id: AirportId(format!("ourairports:{}", rec.airport_ref)),
                airport_ident: rec.airport_ident,
                official_designator: rec.le_ident.clone(),
                computed_magnetic_designator: computed_designator,
                true_heading_deg: heading,
                length_ft,
                width_ft,
                surface: rec.surface,
                le_ident: rec.le_ident,
                le_lat,
                le_lon,
                le_elevation_ft: rec.le_elevation_ft,
                he_ident,
                he_lat,
                he_lon,
                he_elevation_ft: rec.he_elevation_ft,
                temporal: TemporalValidity {
                    valid_from,
                    valid_until: None,
                    source_snapshot_id: snapshot_id.clone(),
                },
            };
            runways.push(runway);
        }

        (runways, report)
    }

    pub fn parse_navaids(
        csv_content: &str,
        snapshot_id: &SourceSnapshotId,
        checksum: &str,
        valid_from: DateTime<Utc>,
    ) -> (Vec<CanonicalNavaid>, IngestReport) {
        let mut report = IngestReport::new("OurAirports", "navaids", checksum);
        let mut reader = csv::ReaderBuilder::new()
            .flexible(true)
            .from_reader(csv_content.as_bytes());

        let mut navaids = Vec::new();
        for result in reader.deserialize::<OurAirportsNavaidRecord>() {
            report.record_seen();
            let rec = match result {
                Ok(rec) => rec,
                Err(err) => {
                    report.record_rejected(format!("Failed to parse navaid CSV row: {err}"));
                    continue;
                }
            };
            report.record_parsed();

            if rec.ident.trim().is_empty() {
                report.record_rejected(format!("Navaid {}: empty identifier", rec.id));
                continue;
            }
            let Some(lat) = valid_lat(rec.latitude_deg) else {
                report.record_rejected(format!("Navaid {}: invalid latitude", rec.ident));
                continue;
            };
            let Some(lon) = valid_lon(rec.longitude_deg) else {
                report.record_rejected(format!("Navaid {}: invalid longitude", rec.ident));
                continue;
            };
            let Some(freq_khz) = rec.frequency_khz.filter(|f| *f > 0) else {
                report.record_rejected(format!(
                    "Navaid {}: missing or invalid frequency",
                    rec.ident
                ));
                continue;
            };

            let kind = match NavaidKind::parse(&rec.navaid_type) {
                Some(k) => k,
                None if rec.navaid_type.eq_ignore_ascii_case("NDB-DME") => {
                    // Composite facility: needs two X-Plane rows (NDB + DME).
                    // Not yet representable as a single canonical navaid.
                    report.record_quarantined(format!(
                        "Navaid {}: composite NDB-DME not yet representable",
                        rec.ident
                    ));
                    continue;
                }
                None => {
                    report.record_rejected(format!(
                        "Navaid {}: unknown navaid type '{}'",
                        rec.ident, rec.navaid_type
                    ));
                    continue;
                }
            };

            if !frequency_in_range(kind, freq_khz) {
                report.record_rejected(format!(
                    "Navaid {}: frequency {} kHz outside valid range for {}",
                    rec.ident,
                    freq_khz,
                    kind.as_str()
                ));
                continue;
            }

            let navaid = CanonicalNavaid {
                object_id: NavaidId(format!("ourairports:{}", rec.id)),
                ident: rec.ident,
                name: rec.name,
                kind,
                frequency: FrequencyKhz(freq_khz),
                latitude: lat,
                longitude: lon,
                elevation_ft: rec.elevation_ft,
                region_code: None,
                associated_airport: rec.associated_airport,
                magnetic_variation_deg: rec.magnetic_variation_deg,
                associated_runway: None,
                localizer_bearing_true_deg: None,
                localizer_bearing_mag_deg: None,
                glideslope_angle_deg: None,
                temporal: TemporalValidity {
                    valid_from,
                    valid_until: None,
                    source_snapshot_id: snapshot_id.clone(),
                },
            };
            navaids.push(navaid);
        }

        (navaids, report)
    }

    /// Parse and ingest one OurAirports dataset transactionally.
    pub fn ingest_dataset(
        dataset: &FetchedDataset,
        store: &mut WorldStore,
    ) -> Result<IngestReport> {
        let started = std::time::Instant::now();
        let short_hash: String = dataset.content_sha256.chars().take(8).collect();
        let snapshot_id = SourceSnapshotId(format!("snap-ourairports-{short_hash}"));
        let retrieved_at = dataset.retrieved_at;
        let valid_from = retrieved_at;

        let snapshot = SourceSnapshot {
            id: snapshot_id.clone(),
            provider: "OurAirports".to_string(),
            dataset: dataset.dataset_name.clone(),
            provider_revision: dataset.provider_revision.clone(),
            airac_cycle: None,
            effective_from: Some(retrieved_at),
            effective_until: None,
            retrieved_at,
            source_uri: dataset.source_uri.clone(),
            content_sha256: dataset.content_sha256.clone(),
            license_id: Some(LICENSE_ID.to_string()),
            license_notes: Some(
                "OurAirports data is released to the public domain; see \
                 https://ourairports.com/data/"
                    .to_string(),
            ),
            parser_version: env!("CARGO_PKG_VERSION").to_string(),
        };

        let revision = WorldRevision {
            id: WorldRevisionId(format!("rev-ourairports-{short_hash}")),
            created_at: retrieved_at,
            source_snapshot_id: snapshot_id.clone(),
            schema_version: "v2".to_string(),
            notes: Some(format!(
                "Ingested OurAirports dataset {}",
                dataset.dataset_name
            )),
        };

        let year = decimal_year(retrieved_at);
        let checksum = dataset.content_sha256.clone();
        let mut report = IngestReport::new("OurAirports", &dataset.dataset_name, &checksum);

        store.transact(|conn| {
            insert_source_snapshot_conn(conn, &snapshot)?;
            insert_world_revision_conn(conn, &revision)?;

            match dataset.dataset_name.as_str() {
                "airports" => {
                    let (airports, parse_report) = Self::parse_airports(
                        &dataset.raw_content,
                        &snapshot_id,
                        &checksum,
                        valid_from,
                    );
                    report = parse_report;
                    for airport in &airports {
                        // Nested runways are inserted by the importer via the
                        // airport writer, but OurAirports airports carry none.
                        report.record_write(insert_airport_conn(conn, airport)?);
                    }
                }
                "runways" => {
                    let (runways, parse_report) = Self::parse_runways(
                        &dataset.raw_content,
                        &snapshot_id,
                        &checksum,
                        year,
                        valid_from,
                    );
                    report = parse_report;
                    let mut known = std::collections::HashSet::new();
                    let mut stmt = conn.prepare("SELECT DISTINCT id FROM airports")?;
                    let ids = stmt.query_map([], |r| r.get::<_, String>(0))?;
                    for id in ids {
                        known.insert(id?);
                    }

                    for runway in runways {
                        if !known.contains(&runway.airport_id.0) {
                            report.record_quarantined(format!(
                                "Runway {}: airport {} not in store",
                                runway.id.0, runway.airport_id.0
                            ));
                            continue;
                        }
                        report.record_write(insert_runway_conn(conn, &runway)?);
                    }
                }
                "navaids" => {
                    let (navaids, parse_report) = Self::parse_navaids(
                        &dataset.raw_content,
                        &snapshot_id,
                        &checksum,
                        valid_from,
                    );
                    report = parse_report;
                    for navaid in &navaids {
                        report.record_write(insert_navaid_conn(conn, navaid)?);
                    }
                }
                other => return Err(anyhow!("Unknown OurAirports dataset: {other}")),
            }
            Ok(())
        })?;

        report.duration_ms = started.elapsed().as_millis() as u64;
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openairac_store::WorldStore;

    const AIRPORTS_CSV: &str = "\
id,ident,type,name,latitude_deg,longitude_deg,elevation_ft,iso_country,municipality
1,KSFO,large_airport,San Francisco International Airport,37.6188,-122.3750,13,US,San Francisco
2,KJFK,large_airport,John F Kennedy International Airport,40.6398,-73.7789,13,US,New York
3,BAD,,Airport With Bad Latitude,95.0,-122.3750,13,US,Nowhere
";

    const RUNWAYS_CSV: &str = "\
id,airport_ref,airport_ident,length_ft,width_ft,surface,le_ident,le_latitude_deg,le_longitude_deg,le_elevation_ft,le_heading_degT,he_ident,he_latitude_deg,he_longitude_deg,he_elevation_ft
101,1,KSFO,11870,200,ASP,28R,37.6188,-122.3750,13,284.0,10L,37.6140,-122.3900,11
102,2,KJFK,14511,200,ASP,13L,40.6398,-73.7789,13,,31R,40.6200,-73.7500,11
103,999,UNKNOWN,5000,100,ASP,09,10.0,20.0,10,90.0,27,10.1,20.1,10
104,1,KSFO,11870,200,ASP,28L,37.6190,-122.3770,13,284.0,10R,37.6141,-122.3901,11
";

    const NAVAIDS_CSV: &str = "\
id,filename,ident,name,type,frequency_khz,latitude_deg,longitude_deg,elevation_ft,associated_airport,magnetic_variation_deg
201,SFO.navaid,SFO,San Francisco VOR-DME,VOR-DME,115800,37.6195,-122.3739,13,KSFO,-13.0
202,XXX.navaid,XXX,Some NDB-DME,NDB-DME,350,37.0,-122.0,10,,5.0
203,BAD.navaid,BAD,Bad Navaid,VOR,999,37.0,-122.0,10,,
";

    fn fixture_dataset(name: &str, content: &str) -> FetchedDataset {
        FetchedDataset {
            provider_name: "OurAirports".to_string(),
            dataset_name: name.to_string(),
            source_uri: format!("https://ourairports.example/{name}.csv"),
            content_sha256: crate::provider::sha256_hex(content.as_bytes()),
            retrieved_at: Utc::now(),
            provider_revision: Some("2026-08-12".to_string()),
            raw_content: content.to_string(),
        }
    }

    #[test]
    fn test_parse_airports_rejects_bad_coords() {
        let snap = SourceSnapshotId("snap-test".to_string());
        let (airports, report) =
            OurAirportsImporter::parse_airports(AIRPORTS_CSV, &snap, "hash", Utc::now());
        assert_eq!(airports.len(), 2);
        assert_eq!(report.records_seen, 3);
        assert_eq!(report.records_parsed, 3);
        assert_eq!(report.records_rejected, 1);
        assert_eq!(airports[0].id, AirportId("ourairports:1".to_string()));
    }

    #[test]
    fn test_parse_runways_fail_closed() {
        let snap = SourceSnapshotId("snap-test".to_string());
        let (runways, report) =
            OurAirportsImporter::parse_runways(RUNWAYS_CSV, &snap, "hash", 2026.0, Utc::now());
        // Row 102: heading missing -> accepted with None.
        // Row 103: unknown airport -> parsed fine, quarantined later at ingest.
        assert_eq!(runways.len(), 4);
        let kjfk = runways.iter().find(|r| r.airport_ident == "KJFK").unwrap();
        assert_eq!(kjfk.true_heading_deg, None);
        assert_eq!(kjfk.computed_magnetic_designator, None);
        let ksfo = runways.iter().find(|r| r.airport_ident == "KSFO").unwrap();
        assert!(ksfo.computed_magnetic_designator.is_some());
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("no published heading"))
        );
    }

    #[test]
    fn test_parse_navaids_quarantines_composite() {
        let snap = SourceSnapshotId("snap-test".to_string());
        let (navaids, report) =
            OurAirportsImporter::parse_navaids(NAVAIDS_CSV, &snap, "hash", Utc::now());
        assert_eq!(navaids.len(), 1);
        assert_eq!(navaids[0].kind, NavaidKind::Vordme);
        assert_eq!(report.records_quarantined, 1);
        assert_eq!(report.records_rejected, 1);
    }

    #[test]
    fn test_ingest_dataset_transactional_and_referential() {
        let mut store = WorldStore::open_in_memory().unwrap();

        let ap = fixture_dataset("airports", AIRPORTS_CSV);
        let report = OurAirportsImporter::ingest_dataset(&ap, &mut store).unwrap();
        assert_eq!(report.records_accepted(), 2);
        assert_eq!(report.records_rejected, 1);

        let rwy = fixture_dataset("runways", RUNWAYS_CSV);
        let report = OurAirportsImporter::ingest_dataset(&rwy, &mut store).unwrap();
        // 101 and 104 accepted, 102 accepted (no heading), 103 quarantined.
        assert_eq!(report.records_accepted(), 3);
        assert_eq!(report.records_quarantined, 1);

        let nav = fixture_dataset("navaids", NAVAIDS_CSV);
        let report = OurAirportsImporter::ingest_dataset(&nav, &mut store).unwrap();
        assert_eq!(report.records_accepted(), 1);

        // The store is structurally clean after the whole ingestion.
        let issues = store.validate().unwrap();
        assert!(issues.is_empty(), "unexpected issues: {issues:?}");

        let status = store.status().unwrap();
        assert_eq!(status.total_airports, 2);
        assert_eq!(status.total_runways, 3);
        assert_eq!(status.total_navaids, 1);
        assert_eq!(status.total_snapshots, 3);
        assert_eq!(status.migration_version, 2);
    }
}
