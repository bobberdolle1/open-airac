use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use openairac_model::*;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Outcome of writing one canonical entity row into the temporal store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityWrite {
    /// New entity revision inserted.
    Created,
    /// Existing entity superseded: previous revision closed, new row written.
    Updated,
    /// Identical payload already present for this entity; nothing written.
    Unchanged,
}

/// Structural integrity finding reported by [`WorldStore::validate`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StoreIssue {
    pub severity: String,
    pub table: String,
    pub id: String,
    pub message: String,
}

pub struct WorldStore {
    conn: Connection,
    path: PathBuf,
}

const VALID_FROM_LE: &str = "valid_from <= ?1";
const VALID_UNTIL_GT: &str = "(valid_until IS NULL OR valid_until > ?1)";

fn rfc3339(d: DateTime<Utc>) -> String {
    d.to_rfc3339()
}

fn parse_utc(s: &str, what: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .with_context(|| format!("invalid stored {what} '{s}'"))
}

fn validate_coords(lat: f64, lon: f64) -> Result<()> {
    if !(-90.0..=90.0).contains(&lat) {
        return Err(anyhow!("latitude {lat:.6} out of range [-90, 90]"));
    }
    if !(-180.0..=180.0).contains(&lon) {
        return Err(anyhow!("longitude {lon:.6} out of range [-180, 180]"));
    }
    Ok(())
}

fn validate_temporal(t: &TemporalValidity) -> Result<()> {
    if let Some(until) = t.valid_until
        && until <= t.valid_from
    {
        return Err(anyhow!(
            "valid_until {} must be strictly after valid_from {}",
            until,
            t.valid_from
        ));
    }
    Ok(())
}

/// Close the open revision of entity `id` in `table` at `valid_from` so the
/// new revision becomes authoritative from that instant on.
fn close_open_revision(
    conn: &Connection,
    table: &str,
    id: &str,
    prev_valid_from: &str,
    new_valid_from: &str,
) -> Result<()> {
    // table names are hardcoded constants, never user input
    let sql = format!(
        "UPDATE {table} SET valid_until = ?1
         WHERE id = ?2 AND valid_from = ?3 AND (valid_until IS NULL OR valid_until > ?1)"
    );
    conn.execute(&sql, params![new_valid_from, id, prev_valid_from])
        .with_context(|| format!("closing previous {table} revision for '{id}'"))?;
    Ok(())
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

    /// Execute schema migrations gated by the SQLite `user_version` pragma.
    pub fn migrate(&mut self) -> Result<()> {
        let version: i64 = self
            .conn
            .query_row("PRAGMA user_version;", [], |r| r.get(0))
            .context("reading schema user_version")?;

        if version < 1 {
            self.conn
                .execute_batch(include_str!("../migrations/v1_init.sql"))
                .context("Failed to execute database migration v1_init.sql")?;
        }
        if version < 2 {
            self.conn
                .execute_batch(include_str!("../migrations/v2_temporal_revisions.sql"))
                .context("Failed to execute database migration v2_temporal_revisions.sql")?;
        }
        self.conn.pragma_update(None, "user_version", 2)?;
        Ok(())
    }

    /// Begin a database transaction
    pub fn transaction(&mut self) -> Result<Transaction<'_>> {
        Ok(self.conn.transaction()?)
    }

    /// Current schema version (from the `user_version` pragma).
    pub fn migration_version(&self) -> Result<u32> {
        let v: i64 = self
            .conn
            .query_row("PRAGMA user_version;", [], |r| r.get(0))?;
        Ok(v as u32)
    }

    pub fn insert_source_snapshot(&self, snapshot: &SourceSnapshot) -> Result<()> {
        insert_source_snapshot_conn(&self.conn, snapshot)
    }

    pub fn insert_world_revision(&self, revision: &WorldRevision) -> Result<()> {
        insert_world_revision_conn(&self.conn, revision)
    }

    /// Insert an airport (plus its nested runways) into the temporal store.
    pub fn insert_airport(&self, airport: &CanonicalAirport) -> Result<EntityWrite> {
        let write = insert_airport_conn(&self.conn, airport)?;
        for rwy in &airport.runways {
            insert_runway_conn(&self.conn, rwy)?;
        }
        Ok(write)
    }

    /// Run `f` inside one database transaction, committing only when `f`
    /// succeeds. Ingestion uses this so a failed run leaves no partial data.
    pub fn transact<T>(&mut self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let txn = self.conn.transaction()?;
        match f(&txn) {
            Ok(value) => {
                txn.commit()?;
                Ok(value)
            }
            Err(err) => {
                let _ = txn.rollback();
                Err(err)
            }
        }
    }

    pub fn insert_runway(&self, runway: &CanonicalRunway) -> Result<EntityWrite> {
        insert_runway_conn(&self.conn, runway)
    }

    pub fn insert_navaid(&self, navaid: &CanonicalNavaid) -> Result<EntityWrite> {
        insert_navaid_conn(&self.conn, navaid)
    }

    pub fn insert_waypoint(&self, waypoint: &CanonicalWaypoint) -> Result<EntityWrite> {
        insert_waypoint_conn(&self.conn, waypoint)
    }

    /// Query airports valid at a given UTC instant.
    pub fn query_airports_at(&self, date: DateTime<Utc>) -> Result<Vec<CanonicalAirport>> {
        let date_str = rfc3339(date);
        let mut stmt = self.conn.prepare(&format!(
            "SELECT id, ident, name, airport_type, latitude_deg, longitude_deg,
                    elevation_ft, iso_country, municipality, source_snapshot_id,
                    valid_from, valid_until
             FROM airports WHERE {VALID_FROM_LE} AND {VALID_UNTIL_GT};"
        ))?;

        let rows = stmt.query_and_then(params![date_str], |row| -> Result<CanonicalAirport> {
            let id: String = row.get(0).context("airports.id")?;
            let temporal = TemporalValidity {
                valid_from: parse_utc(
                    &row.get::<_, String>(10).context("valid_from")?,
                    "valid_from",
                )?,
                valid_until: row
                    .get::<_, Option<String>>(11)
                    .context("valid_until")?
                    .map(|s| parse_utc(&s, "valid_until"))
                    .transpose()?,
                source_snapshot_id: SourceSnapshotId(row.get(9).context("source_snapshot_id")?),
            };
            Ok(CanonicalAirport {
                id: AirportId(id),
                ident: row.get(1).context("ident")?,
                name: row.get(2).context("name")?,
                airport_type: row.get(3).context("airport_type")?,
                latitude: row.get(4).context("latitude")?,
                longitude: row.get(5).context("longitude")?,
                elevation_ft: row.get(6).context("elevation_ft")?,
                iso_country: row.get(7).context("iso_country")?,
                municipality: row.get(8).context("municipality")?,
                runways: Vec::new(),
                temporal,
            })
        })?;

        let mut airports = Vec::new();
        for row in rows {
            let mut airport = row?;
            airport.runways = self.query_runways_for_airport(&airport.id, date)?;
            airports.push(airport);
        }
        Ok(airports)
    }

    fn query_runways_for_airport(
        &self,
        airport_id: &AirportId,
        date: DateTime<Utc>,
    ) -> Result<Vec<CanonicalRunway>> {
        let date_str = rfc3339(date);
        let mut stmt = self.conn.prepare(
            "SELECT id, airport_id, airport_ident, official_designator,
                    computed_magnetic_designator, true_heading_deg, length_ft,
                    width_ft, surface, le_ident, le_lat, le_lon, le_elevation_ft,
                    he_ident, he_lat, he_lon, he_elevation_ft, source_snapshot_id,
                    valid_from, valid_until
             FROM runways
             WHERE airport_id = ?1 AND valid_from <= ?2
               AND (valid_until IS NULL OR valid_until > ?2);",
        )?;

        let rows = stmt.query_and_then(
            params![airport_id.0, date_str],
            |row| -> Result<CanonicalRunway> {
                let id: String = row.get(0).context("runways.id")?;
                Ok(CanonicalRunway {
                    id: RunwayId(id),
                    airport_id: AirportId(
                        row.get::<_, Option<String>>(1)
                            .context("airport_id")?
                            .unwrap_or_default(),
                    ),
                    airport_ident: row.get(2).context("airport_ident")?,
                    official_designator: row.get(3).context("official_designator")?,
                    computed_magnetic_designator: row
                        .get(4)
                        .context("computed_magnetic_designator")?,
                    true_heading_deg: row.get(5).context("true_heading_deg")?,
                    length_ft: row.get(6).context("length_ft")?,
                    width_ft: row.get(7).context("width_ft")?,
                    surface: row.get(8).context("surface")?,
                    le_ident: row.get(9).context("le_ident")?,
                    le_lat: row.get(10).context("le_lat")?,
                    le_lon: row.get(11).context("le_lon")?,
                    le_elevation_ft: row.get(12).context("le_elevation_ft")?,
                    he_ident: row.get(13).context("he_ident")?,
                    he_lat: row.get(14).context("he_lat")?,
                    he_lon: row.get(15).context("he_lon")?,
                    he_elevation_ft: row.get(16).context("he_elevation_ft")?,
                    temporal: TemporalValidity {
                        valid_from: parse_utc(
                            &row.get::<_, String>(18).context("valid_from")?,
                            "valid_from",
                        )?,
                        valid_until: row
                            .get::<_, Option<String>>(19)
                            .context("valid_until")?
                            .map(|s| parse_utc(&s, "valid_until"))
                            .transpose()?,
                        source_snapshot_id: SourceSnapshotId(
                            row.get(17).context("source_snapshot_id")?,
                        ),
                    },
                })
            },
        )?;

        let mut runways = Vec::new();
        for row in rows {
            runways.push(row?);
        }
        Ok(runways)
    }

    /// Query navaids valid at a given UTC instant.
    pub fn query_navaids_at(&self, date: DateTime<Utc>) -> Result<Vec<CanonicalNavaid>> {
        let date_str = rfc3339(date);
        let mut stmt = self.conn.prepare(&format!(
            "SELECT id, ident, name, navaid_type, frequency_khz, latitude_deg,
                    longitude_deg, elevation_ft, region, associated_airport,
                    magnetic_variation_deg, associated_runway,
                    localizer_bearing_true_deg, localizer_bearing_mag_deg,
                    glideslope_angle_deg, source_snapshot_id, valid_from,
                    valid_until
             FROM navaids WHERE {VALID_FROM_LE} AND {VALID_UNTIL_GT};"
        ))?;

        let rows = stmt.query_and_then(params![date_str], |row| -> Result<CanonicalNavaid> {
            let id: String = row.get(0).context("navaids.id")?;
            let type_str: String = row.get(3).context("navaid_type")?;
            // Fail closed: an unknown stored navaid type is a data defect,
            // never silently reinterpreted.
            let kind = NavaidKind::parse(&type_str)
                .ok_or_else(|| anyhow!("navaid '{id}' has unknown navaid_type '{type_str}'"))?;
            Ok(CanonicalNavaid {
                object_id: NavaidId(id),
                ident: row.get(1).context("ident")?,
                name: row.get(2).context("name")?,
                kind,
                frequency: FrequencyKhz(row.get(4).context("frequency_khz")?),
                latitude: row.get(5).context("latitude")?,
                longitude: row.get(6).context("longitude")?,
                elevation_ft: row
                    .get::<_, Option<f64>>(7)
                    .context("elevation_ft")?
                    .map(|e| e.round() as i32),
                region_code: row.get(8).context("region")?,
                associated_airport: row.get(9).context("associated_airport")?,
                magnetic_variation_deg: row.get(10).context("magnetic_variation_deg")?,
                associated_runway: row.get(11).context("associated_runway")?,
                localizer_bearing_true_deg: row.get(12).context("localizer_bearing_true_deg")?,
                localizer_bearing_mag_deg: row.get(13).context("localizer_bearing_mag_deg")?,
                glideslope_angle_deg: row.get(14).context("glideslope_angle_deg")?,
                temporal: TemporalValidity {
                    valid_from: parse_utc(
                        &row.get::<_, String>(16).context("valid_from")?,
                        "valid_from",
                    )?,
                    valid_until: row
                        .get::<_, Option<String>>(17)
                        .context("valid_until")?
                        .map(|s| parse_utc(&s, "valid_until"))
                        .transpose()?,
                    source_snapshot_id: SourceSnapshotId(
                        row.get(15).context("source_snapshot_id")?,
                    ),
                },
            })
        })?;

        let mut navaids: Vec<CanonicalNavaid> = Vec::new();

        for row in rows {
            navaids.push(row?);
        }
        Ok(navaids)
    }

    /// Query waypoints valid at a given UTC instant.
    pub fn query_waypoints_at(&self, date: DateTime<Utc>) -> Result<Vec<CanonicalWaypoint>> {
        let date_str = rfc3339(date);
        let mut stmt = self.conn.prepare(&format!(
            "SELECT id, ident, name, latitude_deg, longitude_deg, region, is_enroute,
                    waypoint_type, source_snapshot_id, valid_from, valid_until
             FROM waypoints WHERE {VALID_FROM_LE} AND {VALID_UNTIL_GT};"
        ))?;

        let rows = stmt.query_and_then(params![date_str], |row| -> Result<CanonicalWaypoint> {
            let id: String = row.get(0).context("waypoints.id")?;
            Ok(CanonicalWaypoint {
                object_id: WaypointId(id),
                ident: row.get(1).context("ident")?,
                name: row.get(2).context("name")?,
                latitude: row.get(3).context("latitude")?,
                longitude: row.get(4).context("longitude")?,
                region_code: row
                    .get::<_, Option<String>>(5)
                    .context("region")?
                    .unwrap_or_default(),
                is_enroute: row.get::<_, i64>(6).context("is_enroute")? != 0,
                waypoint_type: row
                    .get::<_, Option<i64>>(7)
                    .context("waypoint_type")?
                    .map(|v| v as u32),
                temporal: TemporalValidity {
                    valid_from: parse_utc(
                        &row.get::<_, String>(9).context("valid_from")?,
                        "valid_from",
                    )?,
                    valid_until: row
                        .get::<_, Option<String>>(10)
                        .context("valid_until")?
                        .map(|s| parse_utc(&s, "valid_until"))
                        .transpose()?,
                    source_snapshot_id: SourceSnapshotId(row.get(8).context("source_snapshot_id")?),
                },
            })
        })?;

        let mut waypoints = Vec::new();
        for row in rows {
            waypoints.push(row?);
        }
        Ok(waypoints)
    }

    /// Structural integrity validation of the canonical store. Returns every
    /// issue found (empty = clean). Deterministic ordering by (table, id).
    pub fn validate(&self) -> Result<Vec<StoreIssue>> {
        let mut issues = Vec::new();
        let mut push = |severity: &str, table: &str, id: String, message: String| {
            issues.push(StoreIssue {
                severity: severity.to_string(),
                table: table.to_string(),
                id,
                message,
            });
        };

        // 1. Provenance: every entity row must reference an existing snapshot.
        for table in ["airports", "runways", "navaids", "waypoints"] {
            let sql = format!(
                "SELECT t.id FROM {table} t
                 WHERE t.source_snapshot_id NOT IN (SELECT id FROM source_snapshots)
                 ORDER BY t.id LIMIT 20"
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let orphans: Vec<String> = stmt
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            for id in orphans {
                push(
                    "error",
                    table,
                    id.clone(),
                    "references missing source snapshot".into(),
                );
            }
        }

        // 2. Runways must reference an airport that exists in the store.
        {
            let mut stmt = self.conn.prepare(
                "SELECT r.id FROM runways r
                 WHERE (r.airport_id IS NULL OR r.airport_id = '')
                    OR r.airport_id NOT IN (SELECT id FROM airports)
                 ORDER BY r.id LIMIT 20",
            )?;
            let orphans: Vec<String> = stmt
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            for id in orphans {
                push("error", "runways", id, "references missing airport".into());
            }
        }

        // 3. Coordinate ranges.
        for table in ["airports", "navaids", "waypoints"] {
            let sql = format!(
                "SELECT id FROM {table}
                 WHERE latitude_deg < -90 OR latitude_deg > 90
                    OR longitude_deg < -180 OR longitude_deg > 180
                 ORDER BY id LIMIT 20"
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let bad: Vec<String> = stmt
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            for id in bad {
                push("error", table, id, "coordinates out of range".into());
            }
        }

        // 4. Impossible temporal ranges.
        for table in ["airports", "runways", "navaids", "waypoints"] {
            let sql = format!(
                "SELECT id FROM {table}
                 WHERE valid_until IS NOT NULL AND valid_until <= valid_from
                 ORDER BY id LIMIT 20"
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let bad: Vec<String> = stmt
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            for id in bad {
                push(
                    "error",
                    table,
                    id,
                    "valid_until not strictly after valid_from".into(),
                );
            }
        }

        // 5. Overlapping open revisions (more than one row without valid_until).
        for table in ["airports", "runways", "navaids", "waypoints"] {
            let sql = format!(
                "SELECT id FROM {table} WHERE valid_until IS NULL
                 GROUP BY id HAVING COUNT(*) > 1 ORDER BY id LIMIT 20"
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let dup: Vec<String> = stmt
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            for id in dup {
                push(
                    "error",
                    table,
                    id,
                    "multiple open temporal revisions".into(),
                );
            }
        }

        // 6. Navaid kind and frequency sanity.
        {
            let mut stmt = self
                .conn
                .prepare("SELECT id, navaid_type, frequency_khz FROM navaids ORDER BY id")?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, u32>(2)?,
                ))
            })?;
            for row in rows {
                let (id, type_str, freq) = row?;
                match NavaidKind::parse(&type_str) {
                    None => push(
                        "error",
                        "navaids",
                        id.clone(),
                        format!("unknown navaid_type '{type_str}'"),
                    ),
                    Some(kind) => {
                        let (lo, hi, what) = match kind {
                            NavaidKind::Ndb => (190, 1800, "NDB 190-1800 kHz"),
                            NavaidKind::Vor | NavaidKind::Vordme | NavaidKind::Vortac => {
                                (108_000, 118_000, "VHF 108-118 MHz")
                            }
                            NavaidKind::IlsLocalizer => (108_000, 112_000, "ILS-LOC 108-112 MHz"),
                            NavaidKind::IlsGlidepath => (328_000, 336_000, "ILS-GS 328-336 MHz"),
                            NavaidKind::Dme => (108_000, 118_000, "DME 108-118 MHz paired"),
                            NavaidKind::Tacan => (108_000, 136_000, "TACAN 108-136 MHz paired"),
                        };
                        if freq < lo || freq > hi {
                            push(
                                "error",
                                "navaids",
                                id.clone(),
                                format!("frequency {freq} kHz outside {what}"),
                            );
                        }
                    }
                }
            }
        }

        // 7. World revisions must reference an existing snapshot.
        {
            let mut stmt = self.conn.prepare(
                "SELECT id FROM world_revisions
                 WHERE source_snapshot_id NOT IN (SELECT id FROM source_snapshots)
                 ORDER BY id LIMIT 20",
            )?;
            let orphans: Vec<String> = stmt
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            for id in orphans {
                push(
                    "error",
                    "world_revisions",
                    id,
                    "references missing source snapshot".into(),
                );
            }
        }

        issues.sort_by_key(|i| (i.table.clone(), i.id.clone()));
        Ok(issues)
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
            .optional()?;

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
            migration_version: self.migration_version()?,
            total_snapshots,
            latest_revision_id,
            total_airports,
            total_runways,
            total_navaids,
            total_waypoints,
        })
    }
}

// ---------------------------------------------------------------------------
// Connection-level writers. These take `&Connection` so callers (e.g. ingest)
// can run a whole ingestion inside one transaction: `&Transaction` derefs to
// `&Connection`.
// ---------------------------------------------------------------------------

pub fn insert_source_snapshot_conn(conn: &Connection, snapshot: &SourceSnapshot) -> Result<()> {
    conn.execute(
        "INSERT INTO source_snapshots (
            id, provider, dataset, provider_revision, airac_cycle,
            effective_from, effective_until, retrieved_at, source_uri,
            content_sha256, license_id, license_notes, parser_version
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
        ON CONFLICT(id) DO UPDATE SET
            retrieved_at = excluded.retrieved_at,
            content_sha256 = excluded.content_sha256;",
        params![
            snapshot.id.0,
            snapshot.provider,
            snapshot.dataset,
            snapshot.provider_revision,
            snapshot.airac_cycle,
            snapshot.effective_from.map(rfc3339),
            snapshot.effective_until.map(rfc3339),
            rfc3339(snapshot.retrieved_at),
            snapshot.source_uri,
            snapshot.content_sha256,
            snapshot.license_id,
            snapshot.license_notes,
            snapshot.parser_version,
        ],
    )?;
    Ok(())
}

pub fn insert_world_revision_conn(conn: &Connection, revision: &WorldRevision) -> Result<()> {
    conn.execute(
        "INSERT INTO world_revisions (id, created_at, source_snapshot_id, schema_version, notes)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET created_at = excluded.created_at;",
        params![
            revision.id.0,
            rfc3339(revision.created_at),
            revision.source_snapshot_id.0,
            revision.schema_version,
            revision.notes,
        ],
    )?;
    Ok(())
}

pub fn insert_airport_conn(conn: &Connection, airport: &CanonicalAirport) -> Result<EntityWrite> {
    validate_coords(airport.latitude, airport.longitude)?;
    validate_temporal(&airport.temporal)?;
    let id = &airport.id.0;
    let vf = rfc3339(airport.temporal.valid_from);
    let vu = airport.temporal.valid_until.map(rfc3339);

    let existing = conn
        .query_row(
            "SELECT ident, name, airport_type, latitude_deg, longitude_deg,
                    elevation_ft, iso_country, municipality, source_snapshot_id,
                    valid_from
             FROM airports WHERE id = ?1 ORDER BY valid_from DESC LIMIT 1",
            params![id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, f64>(3)?,
                    row.get::<_, f64>(4)?,
                    row.get::<_, Option<f64>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                ))
            },
        )
        .optional()?;

    if let Some((ident, name, atype, lat, lon, elev, country, muni, snap, prev_vf)) = existing {
        let unchanged = ident == airport.ident
            && name == airport.name
            && atype == airport.airport_type
            && lat == airport.latitude
            && lon == airport.longitude
            && elev == airport.elevation_ft
            && country == airport.iso_country
            && muni == airport.municipality
            && snap == airport.temporal.source_snapshot_id.0;
        if unchanged {
            return Ok(EntityWrite::Unchanged);
        }
        if prev_vf >= vf {
            return Err(anyhow!(
                "airport '{id}': new valid_from {vf} must be strictly after existing revision {prev_vf}"
            ));
        }
        close_open_revision(conn, "airports", id, &prev_vf, &vf)?;
        insert_airport_row(conn, airport, &vf, &vu)?;
        return Ok(EntityWrite::Updated);
    }

    insert_airport_row(conn, airport, &vf, &vu)?;
    Ok(EntityWrite::Created)
}

fn insert_airport_row(
    conn: &Connection,
    airport: &CanonicalAirport,
    vf: &str,
    vu: &Option<String>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO airports (
            id, ident, name, airport_type, latitude_deg, longitude_deg,
            elevation_ft, iso_country, municipality, source_snapshot_id,
            valid_from, valid_until
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
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
            vf,
            vu,
        ],
    )?;
    Ok(())
}

pub fn insert_runway_conn(conn: &Connection, runway: &CanonicalRunway) -> Result<EntityWrite> {
    validate_coords(runway.le_lat, runway.le_lon)?;
    validate_coords(runway.he_lat, runway.he_lon)?;
    validate_temporal(&runway.temporal)?;
    let id = &runway.id.0;
    let vf = rfc3339(runway.temporal.valid_from);
    let vu = runway.temporal.valid_until.map(rfc3339);

    let existing = conn
        .query_row(
            "SELECT airport_id, airport_ident, official_designator,
                    computed_magnetic_designator, true_heading_deg, length_ft,
                    width_ft, surface, le_ident, le_lat, le_lon, le_elevation_ft,
                    he_ident, he_lat, he_lon, he_elevation_ft, source_snapshot_id,
                    valid_from
             FROM runways WHERE id = ?1 ORDER BY valid_from DESC LIMIT 1",
            params![id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<f64>>(4)?,
                    row.get::<_, u32>(5)?,
                    row.get::<_, u32>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, f64>(9)?,
                    row.get::<_, f64>(10)?,
                    row.get::<_, Option<f64>>(11)?,
                    row.get::<_, String>(12)?,
                    row.get::<_, f64>(13)?,
                    row.get::<_, f64>(14)?,
                    row.get::<_, Option<f64>>(15)?,
                    row.get::<_, String>(16)?,
                    row.get::<_, String>(17)?,
                ))
            },
        )
        .optional()?;

    if let Some((
        airport_id,
        airport_ident,
        official,
        computed,
        heading,
        len,
        width,
        surface,
        le_ident,
        le_lat,
        le_lon,
        le_elev,
        he_ident,
        he_lat,
        he_lon,
        he_elev,
        snap,
        prev_vf,
    )) = existing
    {
        let unchanged = airport_id.as_deref() == Some(runway.airport_id.0.as_str())
            && airport_ident == runway.airport_ident
            && official == runway.official_designator
            && computed == runway.computed_magnetic_designator
            && heading == runway.true_heading_deg
            && len == runway.length_ft
            && width == runway.width_ft
            && surface == runway.surface
            && le_ident == runway.le_ident
            && le_lat == runway.le_lat
            && le_lon == runway.le_lon
            && le_elev == runway.le_elevation_ft
            && he_ident == runway.he_ident
            && he_lat == runway.he_lat
            && he_lon == runway.he_lon
            && he_elev == runway.he_elevation_ft
            && snap == runway.temporal.source_snapshot_id.0;
        if unchanged {
            return Ok(EntityWrite::Unchanged);
        }
        if prev_vf >= vf {
            return Err(anyhow!(
                "runway '{id}': new valid_from {vf} must be strictly after existing revision {prev_vf}"
            ));
        }
        close_open_revision(conn, "runways", id, &prev_vf, &vf)?;
        insert_runway_row(conn, runway, &vf, &vu)?;
        return Ok(EntityWrite::Updated);
    }

    insert_runway_row(conn, runway, &vf, &vu)?;
    Ok(EntityWrite::Created)
}

fn insert_runway_row(
    conn: &Connection,
    runway: &CanonicalRunway,
    vf: &str,
    vu: &Option<String>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO runways (
            id, airport_id, airport_ident, official_designator,
            computed_magnetic_designator, true_heading_deg, length_ft, width_ft,
            surface, le_ident, le_lat, le_lon, le_elevation_ft,
            he_ident, he_lat, he_lon, he_elevation_ft,
            source_snapshot_id, valid_from, valid_until
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                  ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
        params![
            runway.id.0,
            runway.airport_id.0,
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
            vf,
            vu,
        ],
    )?;
    Ok(())
}

pub fn insert_navaid_conn(conn: &Connection, navaid: &CanonicalNavaid) -> Result<EntityWrite> {
    validate_coords(navaid.latitude, navaid.longitude)?;
    validate_temporal(&navaid.temporal)?;
    if navaid.frequency.0 == 0 {
        return Err(anyhow!("navaid '{}' has zero frequency", navaid.ident));
    }
    let id = &navaid.object_id.0;
    let vf = rfc3339(navaid.temporal.valid_from);
    let vu = navaid.temporal.valid_until.map(rfc3339);

    let existing = conn
        .query_row(
            "SELECT ident, name, navaid_type, frequency_khz, latitude_deg,
                    longitude_deg, elevation_ft, region, associated_airport,
                    magnetic_variation_deg, associated_runway,
                    localizer_bearing_true_deg, localizer_bearing_mag_deg,
                    glideslope_angle_deg, source_snapshot_id, valid_from
             FROM navaids WHERE id = ?1 ORDER BY valid_from DESC LIMIT 1",
            params![id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, u32>(3)?,
                    row.get::<_, f64>(4)?,
                    row.get::<_, f64>(5)?,
                    row.get::<_, Option<f64>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<f64>>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, Option<f64>>(11)?,
                    row.get::<_, Option<f64>>(12)?,
                    row.get::<_, Option<f64>>(13)?,
                    row.get::<_, String>(14)?,
                    row.get::<_, String>(15)?,
                ))
            },
        )
        .optional()?;

    if let Some((
        ident,
        name,
        type_str,
        freq,
        lat,
        lon,
        elev,
        region,
        assoc_ap,
        magvar,
        assoc_rwy,
        loc_true,
        loc_mag,
        gs,
        snap,
        prev_vf,
    )) = existing
    {
        let unchanged = ident == navaid.ident
            && name == navaid.name
            && type_str == navaid.kind.as_str()
            && freq == navaid.frequency.0
            && lat == navaid.latitude
            && lon == navaid.longitude
            && elev.map(|e| e.round() as i32) == navaid.elevation_ft
            && region == navaid.region_code
            && assoc_ap == navaid.associated_airport
            && magvar == navaid.magnetic_variation_deg
            && assoc_rwy == navaid.associated_runway
            && loc_true == navaid.localizer_bearing_true_deg
            && loc_mag == navaid.localizer_bearing_mag_deg
            && gs == navaid.glideslope_angle_deg
            && snap == navaid.temporal.source_snapshot_id.0;
        if unchanged {
            return Ok(EntityWrite::Unchanged);
        }
        if prev_vf >= vf {
            return Err(anyhow!(
                "navaid '{id}': new valid_from {vf} must be strictly after existing revision {prev_vf}"
            ));
        }
        close_open_revision(conn, "navaids", id, &prev_vf, &vf)?;
        insert_navaid_row(conn, navaid, &vf, &vu)?;
        return Ok(EntityWrite::Updated);
    }

    insert_navaid_row(conn, navaid, &vf, &vu)?;
    Ok(EntityWrite::Created)
}

fn insert_navaid_row(
    conn: &Connection,
    navaid: &CanonicalNavaid,
    vf: &str,
    vu: &Option<String>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO navaids (
            id, ident, name, navaid_type, frequency_khz, latitude_deg,
            longitude_deg, elevation_ft, region, associated_airport,
            magnetic_variation_deg, associated_runway, localizer_bearing_true_deg,
            localizer_bearing_mag_deg, glideslope_angle_deg,
            source_snapshot_id, valid_from, valid_until
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                  ?15, ?16, ?17, ?18)",
        params![
            navaid.object_id.0,
            navaid.ident,
            navaid.name,
            navaid.kind.as_str(),
            navaid.frequency.0,
            navaid.latitude,
            navaid.longitude,
            navaid.elevation_ft,
            navaid.region_code,
            navaid.associated_airport,
            navaid.magnetic_variation_deg,
            navaid.associated_runway,
            navaid.localizer_bearing_true_deg,
            navaid.localizer_bearing_mag_deg,
            navaid.glideslope_angle_deg,
            navaid.temporal.source_snapshot_id.0,
            vf,
            vu,
        ],
    )?;
    Ok(())
}

pub fn insert_waypoint_conn(
    conn: &Connection,
    waypoint: &CanonicalWaypoint,
) -> Result<EntityWrite> {
    validate_coords(waypoint.latitude, waypoint.longitude)?;
    validate_temporal(&waypoint.temporal)?;
    let id = &waypoint.object_id.0;
    let vf = rfc3339(waypoint.temporal.valid_from);
    let vu = waypoint.temporal.valid_until.map(rfc3339);

    let existing = conn
        .query_row(
            "SELECT ident, name, latitude_deg, longitude_deg, region, is_enroute,
                    waypoint_type, source_snapshot_id, valid_from
             FROM waypoints WHERE id = ?1 ORDER BY valid_from DESC LIMIT 1",
            params![id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, f64>(2)?,
                    row.get::<_, f64>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                ))
            },
        )
        .optional()?;

    if let Some((ident, name, lat, lon, region, enroute, wptype, snap, prev_vf)) = existing {
        let unchanged = ident == waypoint.ident
            && name == waypoint.name
            && lat == waypoint.latitude
            && lon == waypoint.longitude
            && region.as_deref().unwrap_or("") == waypoint.region_code
            && (enroute != 0) == waypoint.is_enroute
            && wptype.map(|v| v as u32) == waypoint.waypoint_type
            && snap == waypoint.temporal.source_snapshot_id.0;
        if unchanged {
            return Ok(EntityWrite::Unchanged);
        }
        if prev_vf >= vf {
            return Err(anyhow!(
                "waypoint '{id}': new valid_from {vf} must be strictly after existing revision {prev_vf}"
            ));
        }
        close_open_revision(conn, "waypoints", id, &prev_vf, &vf)?;
        insert_waypoint_row(conn, waypoint, &vf, &vu)?;
        return Ok(EntityWrite::Updated);
    }

    insert_waypoint_row(conn, waypoint, &vf, &vu)?;
    Ok(EntityWrite::Created)
}

fn insert_waypoint_row(
    conn: &Connection,
    waypoint: &CanonicalWaypoint,
    vf: &str,
    vu: &Option<String>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO waypoints (
            id, ident, name, latitude_deg, longitude_deg, datum, region,
            is_enroute, waypoint_type, source_snapshot_id, valid_from, valid_until
        ) VALUES (?1, ?2, ?3, ?4, ?5, 'WGS84', ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            waypoint.object_id.0,
            waypoint.ident,
            waypoint.name,
            waypoint.latitude,
            waypoint.longitude,
            waypoint.region_code,
            waypoint.is_enroute as i64,
            waypoint.waypoint_type.map(|v| v as i64),
            waypoint.temporal.source_snapshot_id.0,
            vf,
            vu,
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn snapshot(id: &str) -> SourceSnapshot {
        SourceSnapshot {
            id: SourceSnapshotId(id.to_string()),
            provider: "Test".to_string(),
            dataset: "airports".to_string(),
            provider_revision: Some("2026-08-12".to_string()),
            airac_cycle: None,
            effective_from: Some(Utc::now()),
            effective_until: None,
            retrieved_at: Utc::now(),
            source_uri: "http://test.invalid".to_string(),
            content_sha256: "abc123hash".to_string(),
            license_id: Some("CC0".to_string()),
            license_notes: Some("Public Domain".to_string()),
            parser_version: "0.2.0".to_string(),
        }
    }

    fn airport(id: &str, ident: &str, vf: DateTime<Utc>, snap: &str) -> CanonicalAirport {
        CanonicalAirport {
            id: AirportId(id.to_string()),
            ident: ident.to_string(),
            name: format!("{ident} Airport"),
            airport_type: "large_airport".to_string(),
            latitude: 37.6188,
            longitude: -122.3750,
            elevation_ft: Some(13.0),
            iso_country: Some("US".to_string()),
            municipality: None,
            runways: Vec::new(),
            temporal: TemporalValidity {
                valid_from: vf,
                valid_until: None,
                source_snapshot_id: SourceSnapshotId(snap.to_string()),
            },
        }
    }

    fn runway(ap_id: &str, rwy_id: &str, vf: DateTime<Utc>, snap: &str) -> CanonicalRunway {
        CanonicalRunway {
            id: RunwayId(rwy_id.to_string()),
            airport_id: AirportId(ap_id.to_string()),
            airport_ident: "KSFO".to_string(),
            official_designator: "28R".to_string(),
            computed_magnetic_designator: Some("28R".to_string()),
            true_heading_deg: Some(284.0),
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
                valid_from: vf,
                valid_until: None,
                source_snapshot_id: SourceSnapshotId(snap.to_string()),
            },
        }
    }

    #[test]
    fn test_in_memory_store_lifecycle() {
        let store = WorldStore::open_in_memory().unwrap();
        let status = store.status().unwrap();
        assert!(status.integrity_ok);
        assert_eq!(status.migration_version, 2);
        assert_eq!(status.total_airports, 0);

        let snap = snapshot("snap-001");
        store.insert_source_snapshot(&snap).unwrap();

        let revision = WorldRevision {
            id: WorldRevisionId("rev-001".to_string()),
            created_at: Utc::now(),
            source_snapshot_id: snap.id.clone(),
            schema_version: "v2".to_string(),
            notes: Some("Test Ingest".to_string()),
        };
        store.insert_world_revision(&revision).unwrap();

        let mut ap = airport("KSFO", "KSFO", Utc::now(), "snap-001");
        ap.runways = vec![runway("KSFO", "KSFO-28R", Utc::now(), "snap-001")];
        assert_eq!(store.insert_airport(&ap).unwrap(), EntityWrite::Created);

        let queried = store.query_airports_at(Utc::now()).unwrap();
        assert_eq!(queried.len(), 1);
        assert_eq!(queried[0].ident, "KSFO");
        assert_eq!(queried[0].runways.len(), 1);

        let status_after = store.status().unwrap();
        assert_eq!(status_after.total_airports, 1);
        assert_eq!(status_after.total_runways, 1);
        assert_eq!(status_after.latest_revision_id, Some("rev-001".to_string()));
    }

    #[test]
    fn test_temporal_revisioning_world_at() {
        let store = WorldStore::open_in_memory().unwrap();
        let snap = snapshot("snap-001");
        store.insert_source_snapshot(&snap).unwrap();

        let t0 = Utc::now();
        let t1 = t0 + Duration::from_secs(3600);
        let t2 = t0 + Duration::from_secs(7200);

        // Revision 1 valid from t0.
        let mut ap1 = airport("KSFO", "KSFO", t0, "snap-001");
        ap1.latitude = 37.6188;
        assert_eq!(store.insert_airport(&ap1).unwrap(), EntityWrite::Created);

        // Revision 2 valid from t1 changes the coordinates.
        let mut ap2 = airport("KSFO", "KSFO", t1, "snap-001");
        ap2.latitude = 37.6190;
        assert_eq!(store.insert_airport(&ap2).unwrap(), EntityWrite::Updated);

        let at_t0 = store.query_airports_at(t0).unwrap();
        assert_eq!(at_t0.len(), 1);
        assert_eq!(at_t0[0].latitude, 37.6188);

        let at_t1 = store.query_airports_at(t1).unwrap();
        assert_eq!(at_t1.len(), 1);
        assert_eq!(at_t1[0].latitude, 37.6190);

        let at_t2 = store.query_airports_at(t2).unwrap();
        assert_eq!(at_t2.len(), 1);
        assert_eq!(at_t2[0].latitude, 37.6190);

        // The old revision must be closed at exactly t1 (exclusive end).
        let old_until: Option<String> = store
            .conn
            .query_row(
                "SELECT valid_until FROM airports WHERE id = 'KSFO' AND valid_from = ?1",
                params![t0.to_rfc3339()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(old_until, Some(t1.to_rfc3339()));

        assert!(store.validate().unwrap().is_empty());
    }

    #[test]
    fn test_unchanged_detection() {
        let store = WorldStore::open_in_memory().unwrap();
        let snap = snapshot("snap-001");
        store.insert_source_snapshot(&snap).unwrap();

        let t0 = Utc::now();
        let t1 = t0 + Duration::from_secs(3600);

        let ap1 = airport("KSFO", "KSFO", t0, "snap-001");
        assert_eq!(store.insert_airport(&ap1).unwrap(), EntityWrite::Created);

        // Identical payload at a later valid_from must be reported unchanged
        // and must not write a new row.
        let ap2 = airport("KSFO", "KSFO", t1, "snap-001");
        assert_eq!(store.insert_airport(&ap2).unwrap(), EntityWrite::Unchanged);
        let count: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM airports WHERE id = 'KSFO'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_fail_closed_validation() {
        let store = WorldStore::open_in_memory().unwrap();
        let snap = snapshot("snap-001");
        store.insert_source_snapshot(&snap).unwrap();

        let mut bad = airport("BAD", "BAD", Utc::now(), "snap-001");
        bad.latitude = 95.0;
        assert!(store.insert_airport(&bad).is_err());

        let mut bad_temporal = airport("BADT", "BADT", Utc::now(), "snap-001");
        bad_temporal.temporal.valid_until = Some(Utc::now() - Duration::from_secs(10));
        assert!(store.insert_airport(&bad_temporal).is_err());

        // Inserting a row whose snapshot does not exist must fail (FK).
        let orphan = airport("ORPH", "ORPH", Utc::now(), "missing-snap");
        assert!(store.insert_airport(&orphan).is_err());
    }

    #[test]
    fn test_validate_reports_structural_issues() {
        let store = WorldStore::open_in_memory().unwrap();
        let snap = snapshot("snap-001");
        store.insert_source_snapshot(&snap).unwrap();

        let t = Utc::now();
        let ap = airport("KSFO", "KSFO", t, "snap-001");
        store.insert_airport(&ap).unwrap();

        // Runway referencing a nonexistent airport id.
        let rwy = runway("KJFK", "KSFO-28R", t, "snap-001");
        store.insert_runway(&rwy).unwrap();

        let issues = store.validate().unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].table, "runways");
        assert_eq!(issues[0].id, "KSFO-28R");
    }

    #[test]
    fn test_future_revision_preloading() {
        let store = WorldStore::open_in_memory().unwrap();
        let snap = snapshot("snap-001");
        store.insert_source_snapshot(&snap).unwrap();

        // Preload a revision that becomes valid in one hour.
        let t0 = Utc::now();
        let t_future = t0 + Duration::from_secs(3600);
        let ap = airport("KSFO", "KSFO", t_future, "snap-001");
        assert_eq!(store.insert_airport(&ap).unwrap(), EntityWrite::Created);

        assert_eq!(store.query_airports_at(t0).unwrap().len(), 0);
        assert_eq!(store.query_airports_at(t_future).unwrap().len(), 1);
    }
}
