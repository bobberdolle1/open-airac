use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use openairac_model::*;
use rusqlite::{Connection, Transaction, params};
use std::path::{Path, PathBuf};

pub struct WorldStore {
    conn: Connection,
    path: PathBuf,
}

impl WorldStore {
    /// Open a file-backed SQLite database connection
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_buf = path.as_ref().to_path_buf();
        if let Some(parent) = path_buf.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path_buf)
            .with_context(|| format!("Failed to open SQLite database at {:?}", path_buf))?;

        conn.execute("PRAGMA foreign_keys = ON;", [])?;
        conn.pragma_update(None, "journal_mode", "WAL")?;

        let mut store = Self {
            conn,
            path: path_buf,
        };
        store.migrate()?;
        Ok(store)
    }

    /// Open an in-memory SQLite database connection (for testing)
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute("PRAGMA foreign_keys = ON;", [])?;

        let mut store = Self {
            conn,
            path: PathBuf::from(":memory:"),
        };
        store.migrate()?;
        Ok(store)
    }

    /// Execute database schema migrations
    pub fn migrate(&mut self) -> Result<()> {
        let migration_sql = include_str!("../migrations/v1_init.sql");
        self.conn
            .execute_batch(migration_sql)
            .context("Failed to execute database migration v1_init.sql")?;
        Ok(())
    }

    /// Begin a database transaction
    pub fn transaction(&mut self) -> Result<Transaction<'_>> {
        Ok(self.conn.transaction()?)
    }

    /// Insert a source snapshot
    pub fn insert_source_snapshot(&self, snapshot: &SourceSnapshot) -> Result<()> {
        self.conn.execute(
            "INSERT INTO source_snapshots (
                id, provider, dataset, provider_revision, airac_cycle,
                effective_from, effective_until, retrieved_at, source_uri,
                content_sha256, license_notes, parser_version
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            ON CONFLICT(id) DO UPDATE SET
                retrieved_at = excluded.retrieved_at,
                content_sha256 = excluded.content_sha256;",
            params![
                snapshot.id.0,
                snapshot.provider,
                snapshot.dataset,
                snapshot.provider_revision,
                snapshot.airac_cycle,
                snapshot.effective_from.map(|d| d.to_rfc3339()),
                snapshot.effective_until.map(|d| d.to_rfc3339()),
                snapshot.retrieved_at.to_rfc3339(),
                snapshot.source_uri,
                snapshot.content_sha256,
                snapshot.license_notes,
                snapshot.parser_version,
            ],
        )?;
        Ok(())
    }

    /// Insert a world revision
    pub fn insert_world_revision(&self, revision: &WorldRevision) -> Result<()> {
        self.conn.execute(
            "INSERT INTO world_revisions (id, created_at, source_snapshot_id, schema_version, notes)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET created_at = excluded.created_at;",
            params![
                revision.id.0,
                revision.created_at.to_rfc3339(),
                revision.source_snapshot_id.0,
                revision.schema_version,
                revision.notes,
            ],
        )?;
        Ok(())
    }

    /// Insert an airport
    pub fn insert_airport(&self, airport: &CanonicalAirport) -> Result<()> {
        self.conn.execute(
            "INSERT INTO airports (
                id, ident, name, airport_type, latitude_deg, longitude_deg,
                elevation_ft, iso_country, municipality, source_snapshot_id,
                valid_from, valid_until
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            ON CONFLICT(id) DO UPDATE SET
                latitude_deg = excluded.latitude_deg,
                longitude_deg = excluded.longitude_deg,
                elevation_ft = excluded.elevation_ft;",
            params![
                airport.id.0,
                airport.ident,
                airport.name,
                airport.airport_type,
                airport.latitude,
                airport.longitude,
                airport.elevation_ft,
                airport.iso_country,
                airport.municipality,
                airport.temporal.source_snapshot_id.0,
                airport.temporal.valid_from.to_rfc3339(),
                airport.temporal.valid_until.map(|d| d.to_rfc3339()),
            ],
        )?;

        for rwy in &airport.runways {
            self.insert_runway(rwy)?;
        }
        Ok(())
    }

    /// Insert a runway
    pub fn insert_runway(&self, runway: &CanonicalRunway) -> Result<()> {
        self.conn.execute(
            "INSERT INTO runways (
                id, airport_ident, official_designator, computed_magnetic_designator,
                true_heading_deg, length_ft, width_ft, surface,
                le_ident, le_lat, le_lon, le_elevation_ft,
                he_ident, he_lat, he_lon, he_elevation_ft,
                source_snapshot_id, valid_from, valid_until
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
            ON CONFLICT(id) DO UPDATE SET
                true_heading_deg = excluded.true_heading_deg,
                computed_magnetic_designator = excluded.computed_magnetic_designator;",
            params![
                runway.id,
                runway.airport_ident,
                runway.official_designator,
                runway.computed_magnetic_designator,
                runway.true_heading_deg,
                runway.length_ft,
                runway.width_ft,
                runway.surface,
                runway.le_ident,
                runway.le_lat,
                runway.le_lon,
                runway.le_elevation_ft,
                runway.he_ident,
                runway.he_lat,
                runway.he_lon,
                runway.he_elevation_ft,
                runway.temporal.source_snapshot_id.0,
                runway.temporal.valid_from.to_rfc3339(),
                runway.temporal.valid_until.map(|d| d.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    /// Insert a navaid
    pub fn insert_navaid(&self, navaid: &CanonicalNavaid) -> Result<()> {
        self.conn.execute(
            "INSERT INTO navaids (
                id, ident, name, navaid_type, frequency_khz, latitude_deg,
                longitude_deg, elevation_ft, associated_airport, magnetic_variation_deg,
                source_snapshot_id, valid_from, valid_until
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            ON CONFLICT(id) DO UPDATE SET
                latitude_deg = excluded.latitude_deg,
                longitude_deg = excluded.longitude_deg,
                frequency_khz = excluded.frequency_khz;",
            params![
                navaid.object_id.0,
                navaid.ident,
                navaid.name,
                navaid.kind.as_str(),
                navaid.frequency.0,
                navaid.latitude,
                navaid.longitude,
                navaid.elevation_ft,
                navaid.associated_airport,
                navaid.magnetic_variation_deg,
                navaid.temporal.source_snapshot_id.0,
                navaid.temporal.valid_from.to_rfc3339(),
                navaid.temporal.valid_until.map(|d| d.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    /// Insert a waypoint
    pub fn insert_waypoint(&self, waypoint: &CanonicalWaypoint) -> Result<()> {
        self.conn.execute(
            "INSERT INTO waypoints (
                id, ident, name, latitude_deg, longitude_deg, datum, region,
                source_snapshot_id, valid_from, valid_until
            ) VALUES (?1, ?2, ?3, ?4, ?5, 'WGS84', ?6, ?7, ?8, ?9)
            ON CONFLICT(id) DO UPDATE SET
                latitude_deg = excluded.latitude_deg,
                longitude_deg = excluded.longitude_deg;",
            params![
                waypoint.object_id.0,
                waypoint.ident,
                waypoint.name,
                waypoint.latitude,
                waypoint.longitude,
                waypoint.region_code,
                waypoint.temporal.source_snapshot_id.0,
                waypoint.temporal.valid_from.to_rfc3339(),
                waypoint.temporal.valid_until.map(|d| d.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    /// Query airports valid at a given UTC date
    pub fn query_airports_at(&self, date: DateTime<Utc>) -> Result<Vec<CanonicalAirport>> {
        let date_str = date.to_rfc3339();
        let mut stmt = self.conn.prepare(
            "SELECT id, ident, name, airport_type, latitude_deg, longitude_deg,
                    elevation_ft, iso_country, municipality, source_snapshot_id, valid_from, valid_until
             FROM airports
             WHERE valid_from <= ?1 AND (valid_until IS NULL OR valid_until >= ?1);",
        )?;

        let rows = stmt.query_map(params![date_str], |row| {
            let id: String = row.get(0)?;
            let ident: String = row.get(1)?;
            let name: String = row.get(2)?;
            let airport_type: String = row.get(3)?;
            let latitude: f64 = row.get(4)?;
            let longitude: f64 = row.get(5)?;
            let elevation_ft: Option<f64> = row.get(6)?;
            let iso_country: Option<String> = row.get(7)?;
            let municipality: Option<String> = row.get(8)?;
            let source_snapshot_id: String = row.get(9)?;
            let valid_from_str: String = row.get(10)?;
            let valid_until_str: Option<String> = row.get(11)?;

            let valid_from = DateTime::parse_from_rfc3339(&valid_from_str)
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or(Utc::now());
            let valid_until = valid_until_str.and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .map(|d| d.with_timezone(&Utc))
                    .ok()
            });

            Ok(CanonicalAirport {
                id: AirportId(id),
                ident,
                name,
                airport_type,
                latitude,
                longitude,
                elevation_ft,
                iso_country,
                municipality,
                runways: Vec::new(),
                temporal: TemporalValidity {
                    valid_from,
                    valid_until,
                    source_snapshot_id: SourceSnapshotId(source_snapshot_id),
                },
            })
        })?;

        let mut airports = Vec::new();
        for airport_res in rows {
            let mut airport = airport_res?;
            airport.runways = self.query_runways_for_airport(&airport.ident)?;
            airports.push(airport);
        }
        Ok(airports)
    }

    fn query_runways_for_airport(&self, airport_ident: &str) -> Result<Vec<CanonicalRunway>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, airport_ident, official_designator, computed_magnetic_designator,
                    true_heading_deg, length_ft, width_ft, surface,
                    le_ident, le_lat, le_lon, le_elevation_ft,
                    he_ident, he_lat, he_lon, he_elevation_ft,
                    source_snapshot_id, valid_from, valid_until
             FROM runways
             WHERE airport_ident = ?1;",
        )?;

        let rows = stmt.query_map(params![airport_ident], |row| {
            let id: String = row.get(0)?;
            let airport_ident: String = row.get(1)?;
            let official_designator: String = row.get(2)?;
            let computed_magnetic_designator: String = row.get(3)?;
            let true_heading_deg: f64 = row.get(4)?;
            let length_ft: u32 = row.get(5)?;
            let width_ft: u32 = row.get(6)?;
            let surface: Option<String> = row.get(7)?;
            let le_ident: String = row.get(8)?;
            let le_lat: f64 = row.get(9)?;
            let le_lon: f64 = row.get(10)?;
            let le_elevation_ft: Option<f64> = row.get(11)?;
            let he_ident: String = row.get(12)?;
            let he_lat: f64 = row.get(13)?;
            let he_lon: f64 = row.get(14)?;
            let he_elevation_ft: Option<f64> = row.get(15)?;
            let source_snapshot_id: String = row.get(16)?;
            let valid_from_str: String = row.get(17)?;
            let valid_until_str: Option<String> = row.get(18)?;

            let valid_from = DateTime::parse_from_rfc3339(&valid_from_str)
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or(Utc::now());
            let valid_until = valid_until_str.and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .map(|d| d.with_timezone(&Utc))
                    .ok()
            });

            Ok(CanonicalRunway {
                id,
                airport_ident,
                official_designator,
                computed_magnetic_designator,
                true_heading_deg,
                length_ft,
                width_ft,
                surface,
                le_ident,
                le_lat,
                le_lon,
                le_elevation_ft,
                he_ident,
                he_lat,
                he_lon,
                he_elevation_ft,
                temporal: TemporalValidity {
                    valid_from,
                    valid_until,
                    source_snapshot_id: SourceSnapshotId(source_snapshot_id),
                },
            })
        })?;

        let mut runways = Vec::new();
        for rwy in rows {
            runways.push(rwy?);
        }
        Ok(runways)
    }

    /// Query navaids valid at a given UTC date
    pub fn query_navaids_at(&self, date: DateTime<Utc>) -> Result<Vec<CanonicalNavaid>> {
        let date_str = date.to_rfc3339();
        let mut stmt = self.conn.prepare(
            "SELECT id, ident, name, navaid_type, frequency_khz, latitude_deg,
                    longitude_deg, elevation_ft, associated_airport, magnetic_variation_deg,
                    source_snapshot_id, valid_from, valid_until
             FROM navaids
             WHERE valid_from <= ?1 AND (valid_until IS NULL OR valid_until >= ?1);",
        )?;

        let rows = stmt.query_map(params![date_str], |row| {
            let id: String = row.get(0)?;
            let ident: String = row.get(1)?;
            let name: String = row.get(2)?;
            let navaid_type_str: String = row.get(3)?;
            let frequency_khz: u32 = row.get(4)?;
            let latitude: f64 = row.get(5)?;
            let longitude: f64 = row.get(6)?;
            let elevation_ft: i32 = row
                .get::<_, Option<f64>>(7)?
                .map(|e| e.round() as i32)
                .unwrap_or(0);
            let associated_airport: Option<String> = row.get(8)?;
            let magnetic_variation_deg: Option<f64> = row.get(9)?;
            let source_snapshot_id: String = row.get(10)?;
            let valid_from_str: String = row.get(11)?;
            let valid_until_str: Option<String> = row.get(12)?;

            let kind = NavaidKind::parse(&navaid_type_str).unwrap_or(NavaidKind::Vor);
            let valid_from = DateTime::parse_from_rfc3339(&valid_from_str)
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or(Utc::now());
            let valid_until = valid_until_str.and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .map(|d| d.with_timezone(&Utc))
                    .ok()
            });

            Ok(CanonicalNavaid {
                object_id: NavaidId(id),
                ident,
                name,
                kind,
                frequency: FrequencyKhz(frequency_khz),
                latitude,
                longitude,
                elevation_ft,
                associated_airport,
                magnetic_variation_deg,
                temporal: TemporalValidity {
                    valid_from,
                    valid_until,
                    source_snapshot_id: SourceSnapshotId(source_snapshot_id),
                },
            })
        })?;

        let mut navaids = Vec::new();
        for nav in rows {
            navaids.push(nav?);
        }
        Ok(navaids)
    }

    /// Query waypoints valid at a given UTC date
    pub fn query_waypoints_at(&self, date: DateTime<Utc>) -> Result<Vec<CanonicalWaypoint>> {
        let date_str = date.to_rfc3339();
        let mut stmt = self.conn.prepare(
            "SELECT id, ident, name, latitude_deg, longitude_deg, region,
                    source_snapshot_id, valid_from, valid_until
             FROM waypoints
             WHERE valid_from <= ?1 AND (valid_until IS NULL OR valid_until >= ?1);",
        )?;

        let rows = stmt.query_map(params![date_str], |row| {
            let id: String = row.get(0)?;
            let ident: String = row.get(1)?;
            let name: String = row.get(2)?;
            let latitude: f64 = row.get(3)?;
            let longitude: f64 = row.get(4)?;
            let region: Option<String> = row.get(5)?;
            let source_snapshot_id: String = row.get(6)?;
            let valid_from_str: String = row.get(7)?;
            let valid_until_str: Option<String> = row.get(8)?;

            let valid_from = DateTime::parse_from_rfc3339(&valid_from_str)
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or(Utc::now());
            let valid_until = valid_until_str.and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .map(|d| d.with_timezone(&Utc))
                    .ok()
            });

            Ok(CanonicalWaypoint {
                object_id: WaypointId(id),
                ident,
                name,
                latitude,
                longitude,
                is_enroute: true,
                region_code: region.unwrap_or_default(),
                temporal: TemporalValidity {
                    valid_from,
                    valid_until,
                    source_snapshot_id: SourceSnapshotId(source_snapshot_id),
                },
            })
        })?;

        let mut waypoints = Vec::new();
        for wp in rows {
            waypoints.push(wp?);
        }
        Ok(waypoints)
    }

    /// Get current database status & integrity report
    pub fn status(&self) -> Result<StoreStatus> {
        let integrity_ok: String = self
            .conn
            .query_row("PRAGMA integrity_check;", [], |r| r.get(0))?;
        let integrity_is_ok = integrity_ok.to_lowercase() == "ok";

        let total_snapshots: usize =
            self.conn
                .query_row("SELECT COUNT(*) FROM source_snapshots;", [], |r| r.get(0))?;

        let latest_revision_id: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM world_revisions ORDER BY created_at DESC LIMIT 1;",
                [],
                |r| r.get(0),
            )
            .ok();

        let total_airports: usize =
            self.conn
                .query_row("SELECT COUNT(*) FROM airports;", [], |r| r.get(0))?;

        let total_runways: usize =
            self.conn
                .query_row("SELECT COUNT(*) FROM runways;", [], |r| r.get(0))?;

        let total_navaids: usize =
            self.conn
                .query_row("SELECT COUNT(*) FROM navaids;", [], |r| r.get(0))?;

        let total_waypoints: usize =
            self.conn
                .query_row("SELECT COUNT(*) FROM waypoints;", [], |r| r.get(0))?;

        Ok(StoreStatus {
            database_path: self.path.to_string_lossy().to_string(),
            is_open: true,
            integrity_ok: integrity_is_ok,
            migration_version: 1,
            total_snapshots,
            latest_revision_id,
            total_airports,
            total_runways,
            total_navaids,
            total_waypoints,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_memory_store_lifecycle() {
        let store = WorldStore::open_in_memory().unwrap();
        let status = store.status().unwrap();
        assert!(status.integrity_ok);
        assert_eq!(status.total_airports, 0);

        let snapshot = SourceSnapshot {
            id: SourceSnapshotId("snap-001".to_string()),
            provider: "OurAirports".to_string(),
            dataset: "airports".to_string(),
            provider_revision: Some("2026-08-12".to_string()),
            airac_cycle: None,
            effective_from: Some(Utc::now()),
            effective_until: None,
            retrieved_at: Utc::now(),
            source_uri: "https://davidmegginson.github.io/ourairports-data/airports.csv"
                .to_string(),
            content_sha256: "abc123hash".to_string(),
            license_notes: Some("Public Domain".to_string()),
            parser_version: "0.2.0".to_string(),
        };
        store.insert_source_snapshot(&snapshot).unwrap();

        let revision = WorldRevision {
            id: WorldRevisionId("rev-001".to_string()),
            created_at: Utc::now(),
            source_snapshot_id: snapshot.id.clone(),
            schema_version: "v1".to_string(),
            notes: Some("Test Ingest".to_string()),
        };
        store.insert_world_revision(&revision).unwrap();

        let airport = CanonicalAirport {
            id: AirportId("KSFO".to_string()),
            ident: "KSFO".to_string(),
            name: "San Francisco International Airport".to_string(),
            airport_type: "large_airport".to_string(),
            latitude: 37.6188,
            longitude: -122.3750,
            elevation_ft: Some(13.0),
            iso_country: Some("US".to_string()),
            municipality: Some("San Francisco".to_string()),
            runways: vec![CanonicalRunway {
                id: "KSFO-28R".to_string(),
                airport_ident: "KSFO".to_string(),
                official_designator: "28R".to_string(),
                computed_magnetic_designator: "28R".to_string(),
                true_heading_deg: 284.0,
                length_ft: 11870,
                width_ft: 200,
                surface: Some("ASP".to_string()),
                le_ident: "28R".to_string(),
                le_lat: 37.6188,
                le_lon: -122.3750,
                le_elevation_ft: Some(13.0),
                he_ident: "10L".to_string(),
                he_lat: 37.6140,
                he_lon: -122.3900,
                he_elevation_ft: Some(11.0),
                temporal: TemporalValidity {
                    valid_from: Utc::now(),
                    valid_until: None,
                    source_snapshot_id: snapshot.id.clone(),
                },
            }],
            temporal: TemporalValidity {
                valid_from: Utc::now(),
                valid_until: None,
                source_snapshot_id: snapshot.id.clone(),
            },
        };
        store.insert_airport(&airport).unwrap();

        let queried = store.query_airports_at(Utc::now()).unwrap();
        assert_eq!(queried.len(), 1);
        assert_eq!(queried[0].ident, "KSFO");
        assert_eq!(queried[0].runways.len(), 1);

        let status_after = store.status().unwrap();
        assert_eq!(status_after.total_airports, 1);
        assert_eq!(status_after.total_runways, 1);
        assert_eq!(status_after.latest_revision_id, Some("rev-001".to_string()));
    }
}
