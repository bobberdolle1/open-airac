use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use openairac_model::*;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;
use std::collections::HashMap;
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
        if version < 3 {
            self.conn
                .execute_batch(include_str!("../migrations/v3_source_observations.sql"))
                .context("Failed to execute database migration v3_source_observations.sql")?;
        }
        if version < 4 {
            self.conn
                .execute_batch(include_str!("../migrations/v4_procedure_legs.sql"))
                .context("Failed to execute database migration v4_procedure_legs.sql")?;
        }
        if version < 5 {
            self.conn
                .execute_batch(include_str!("../migrations/v5_airac_lifecycle.sql"))
                .context("Failed to execute database migration v5_airac_lifecycle.sql")?;
        }
        self.conn.pragma_update(None, "user_version", 5)?;
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
        query_airports_at_conn(&self.conn, date)
    }
}

/// Runway rows valid at `date`, optionally filtered to one airport.
/// Standalone form used by rollback re-publication.
pub fn query_runways_conn(
    conn: &Connection,
    date: DateTime<Utc>,
    airport_id: Option<&AirportId>,
) -> Result<Vec<CanonicalRunway>> {
    {
        let date_str = rfc3339(date);
        let mut stmt = conn.prepare(
            "SELECT id, airport_id, airport_ident, official_designator,
                    computed_magnetic_designator, true_heading_deg, length_ft,
                    width_ft, surface, le_ident, le_lat, le_lon, le_elevation_ft,
                    he_ident, he_lat, he_lon, he_elevation_ft, source_snapshot_id,
                    valid_from, valid_until
             FROM runways
             WHERE (?1 IS NULL OR airport_id = ?1) AND valid_from <= ?2
               AND (valid_until IS NULL OR valid_until > ?2);",
        )?;

        let rows = stmt.query_and_then(
            params![airport_id.map(|a| a.0.as_str()), date_str],
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
}

impl WorldStore {
    /// Query navaids valid at a given UTC instant.
    pub fn query_navaids_at(&self, date: DateTime<Utc>) -> Result<Vec<CanonicalNavaid>> {
        query_navaids_at_conn(&self.conn, date)
    }

    /// Query waypoints valid at a given UTC instant.
    pub fn query_waypoints_at(&self, date: DateTime<Utc>) -> Result<Vec<CanonicalWaypoint>> {
        query_waypoints_at_conn(&self.conn, date)
    }

    /// Query airway legs valid at a given UTC instant.
    pub fn query_airway_legs_at(&self, date: DateTime<Utc>) -> Result<Vec<CanonicalAirwayLeg>> {
        query_airway_legs_at(&self.conn, date)
    }

    /// Query procedure legs valid at a given UTC instant.
    pub fn query_procedure_legs_at(
        &self,
        date: DateTime<Utc>,
    ) -> Result<Vec<CanonicalProcedureLeg>> {
        query_procedure_legs_at(&self.conn, date)
    }

    // -------------------------------------------------------------------
    // AIRAC lifecycle (v5)
    // -------------------------------------------------------------------

    /// Insert a cycle into the catalog. Fails on duplicate id.
    pub fn insert_cycle(&self, cycle: &AiracCycle) -> Result<()> {
        insert_cycle_conn(&self.conn, cycle)
    }

    pub fn query_cycles(&self) -> Result<Vec<AiracCycle>> {
        query_cycles_conn(&self.conn)
    }

    pub fn query_cycle(&self, id: &CycleId) -> Result<Option<AiracCycle>> {
        query_cycle_conn(&self.conn, id)
    }

    /// Transition a cycle's status, validating the state machine.
    pub fn set_cycle_status(&self, id: &CycleId, status: CycleStatus) -> Result<()> {
        set_cycle_status_conn(&self.conn, id, status, Utc::now())
    }

    pub fn insert_cycle_snapshot(
        &self,
        cycle_id: &CycleId,
        snapshot_id: &SourceSnapshotId,
    ) -> Result<()> {
        insert_cycle_snapshot_conn(&self.conn, cycle_id, snapshot_id)
    }

    /// The source snapshots of one cycle — the provenance basis for
    /// ownership scoping (rollback diff, close_absent).
    pub fn cycle_snapshot_ids(&self, cycle_id: &CycleId) -> Result<Vec<SourceSnapshotId>> {
        cycle_snapshot_ids_conn(&self.conn, cycle_id)
    }

    /// Append an audit/journal entry; returns its id.
    pub fn record_cycle_event(&self, event: &CycleEvent) -> Result<i64> {
        record_cycle_event_conn(&self.conn, event)
    }

    pub fn query_cycle_events(&self) -> Result<Vec<CycleEvent>> {
        query_cycle_events_conn(&self.conn)
    }

    /// Whether an event of `kind` was already recorded for the cycle.
    pub fn has_cycle_event(&self, cycle_id: &CycleId, kind: CycleEventKind) -> Result<bool> {
        has_cycle_event_conn(&self.conn, cycle_id, kind)
    }

    /// Append an observed dataset publication (append-only).
    pub fn insert_dataset_version(&self, version: &DatasetVersion) -> Result<()> {
        insert_dataset_version_conn(&self.conn, version)
    }

    /// The latest observed publication of a dataset/cycle (max retrieved_at).
    pub fn latest_dataset_version(
        &self,
        provider: &str,
        dataset: &str,
        cycle: Option<&str>,
    ) -> Result<Option<DatasetVersion>> {
        latest_dataset_version_conn(&self.conn, provider, dataset, cycle)
    }

    pub fn insert_entity_alias(
        &self,
        table: &str,
        natural_key: &str,
        provider: &str,
        entity_id: &str,
    ) -> Result<()> {
        insert_entity_alias_conn(&self.conn, table, natural_key, provider, entity_id)
    }

    /// Roll an Active cycle back at `at` by re-publishing the pre-cycle
    /// state as new revisions (one transaction). Scope is the cycle's own
    /// provider/dataset/entity domain; other providers are never touched.
    pub fn rollback_cycle(
        &mut self,
        cycle_id: &CycleId,
        at: DateTime<Utc>,
    ) -> Result<RollbackReport> {
        let txn = self.conn.transaction()?;
        let report = rollback_cycle_conn(&txn, cycle_id, at)?;
        txn.commit()?;
        Ok(report)
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
        for table in [
            "airports",
            "runways",
            "navaids",
            "waypoints",
            "airway_legs",
            "procedure_legs",
        ] {
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
        for table in [
            "airports",
            "runways",
            "navaids",
            "waypoints",
            "airway_legs",
            "procedure_legs",
        ] {
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
        for table in [
            "airports",
            "runways",
            "navaids",
            "waypoints",
            "airway_legs",
            "procedure_legs",
        ] {
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

        // 8. Procedure legs: altitude descriptor membership and band
        // consistency (a 'B' band must carry both altitudes).
        {
            let mut stmt = self.conn.prepare(
                "SELECT id, altitude_descriptor, altitude_1_ft, altitude_2_ft
                 FROM procedure_legs ORDER BY id",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                ))
            })?;
            for row in rows {
                let (id, desc, a1, a2) = row?;
                let desc = desc.unwrap_or_default();
                if !desc.is_empty() && desc != "+" && desc != "-" && desc != "B" {
                    push(
                        "error",
                        "procedure_legs",
                        id.clone(),
                        format!("unknown altitude descriptor '{desc}'"),
                    );
                }
                if desc == "B" && (a1.is_none() || a2.is_none()) {
                    push(
                        "error",
                        "procedure_legs",
                        id.clone(),
                        "altitude band 'B' requires both altitudes".into(),
                    );
                }
                if a1.is_none() && a2.is_some() {
                    push(
                        "error",
                        "procedure_legs",
                        id.clone(),
                        "altitude 2 set without altitude 1".into(),
                    );
                }
            }
        }

        // 9. Procedure legs: known path terminators only.
        {
            let known = [
                "IF", "TF", "CF", "DF", "FA", "FC", "FD", "FM", "CA", "CD", "CI", "CR", "VA", "VD",
                "VI", "VM", "VR", "HA", "HF", "HM", "RF",
            ];
            let mut stmt = self
                .conn
                .prepare("SELECT id, path_terminator FROM procedure_legs ORDER BY id")?;
            let rows =
                stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            for row in rows {
                let (id, term) = row?;
                if !known.contains(&term.as_str()) {
                    push(
                        "warning",
                        "procedure_legs",
                        id.clone(),
                        format!("path terminator '{term}' is not in the ARINC 424 set"),
                    );
                }
            }
        }

        // 10. Procedure legs: duplicate sequence numbers within one
        // (airport, kind, procedure, transition, route type) at one
        // validity instant.
        {
            let mut stmt = self.conn.prepare(
                "SELECT airport_ident, procedure_kind, procedure_ident,
                        transition_ident, route_type, valid_from,
                        sequence_number, COUNT(*) AS n
                 FROM procedure_legs
                 GROUP BY airport_ident, procedure_kind, procedure_ident,
                          transition_ident, route_type, valid_from,
                          sequence_number
                 HAVING n > 1 ORDER BY airport_ident LIMIT 20",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, i64>(7)?,
                ))
            })?;
            for row in rows {
                let (airport, kind, ident, transition, seq, n) = row?;
                push(
                    "error",
                    "procedure_legs",
                    format!("{airport}/{kind}/{ident}/{transition}"),
                    format!("sequence {seq} duplicated {n} times"),
                );
            }
        }

        // 11. Cross-table: procedure fix references must resolve to a
        // waypoint or navaid in the store.
        {
            let mut stmt = self.conn.prepare(
                "SELECT p.id, p.fix_ident, p.fix_icao_code
                 FROM procedure_legs p
                 WHERE p.fix_ident NOT IN (SELECT ident FROM waypoints)
                   AND p.fix_ident NOT IN (SELECT ident FROM navaids)
                 ORDER BY p.id LIMIT 20",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?;
            for row in rows {
                let (id, fix, icao) = row?;
                push(
                    "warning",
                    "procedure_legs",
                    id,
                    format!("fix {fix} ({icao}) has no waypoint or navaid row"),
                );
            }
        }

        // 12. Cross-table: airway endpoints must resolve to a waypoint or
        // navaid.
        {
            let mut stmt = self.conn.prepare(
                "SELECT l.id, l.start_fix, l.end_fix
                 FROM airway_legs l
                 WHERE l.start_fix NOT IN (SELECT ident FROM waypoints)
                    AND l.start_fix NOT IN (SELECT ident FROM navaids)
                    OR l.end_fix NOT IN (SELECT ident FROM waypoints)
                    AND l.end_fix NOT IN (SELECT ident FROM navaids)
                 ORDER BY l.id LIMIT 20",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?;
            for row in rows {
                let (id, start, end) = row?;
                push(
                    "warning",
                    "airway_legs",
                    id,
                    format!("endpoint {start}/{end} has no waypoint row"),
                );
            }
        }

        // 14. Cycle catalog: status membership and window sanity.
        {
            let mut stmt = self.conn.prepare(
                "SELECT id, status, effective_from, effective_until
                 FROM airac_cycles ORDER BY id",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            })?;
            for row in rows {
                let (id, status, from, until) = row?;
                if CycleStatus::parse(&status).is_none() {
                    push(
                        "error",
                        "airac_cycles",
                        id.clone(),
                        format!("unknown status '{status}'"),
                    );
                }
                if let (Some(f), Some(u)) = (&from, &until)
                    && u <= f
                {
                    push(
                        "error",
                        "airac_cycles",
                        id.clone(),
                        "effective_until not strictly after effective_from".into(),
                    );
                }
            }
        }

        // 15. Cycle events: kind membership and rollback invariant
        // (Rollback MUST name the restored cycle; provenance of the
        // re-published rows lives in their own source_snapshot_id).
        {
            let mut stmt = self.conn.prepare(
                "SELECT id, kind, cycle_id, restored_cycle_id FROM cycle_events ORDER BY id",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            })?;
            for row in rows {
                let (id, kind, cycle_id, restored) = row?;
                if CycleEventKind::parse(&kind).is_none() {
                    push(
                        "error",
                        "cycle_events",
                        id.to_string(),
                        format!("unknown kind '{kind}'"),
                    );
                }
                if kind == "Rollback" && restored.is_none() {
                    push(
                        "error",
                        "cycle_events",
                        id.to_string(),
                        format!("rollback of '{cycle_id}' must name the restored cycle"),
                    );
                }
            }
        }

        // 16. Dataset versions: revision_kind and coverage membership.
        {
            let mut stmt = self.conn.prepare(
                "SELECT id, revision_kind, coverage, content_sha256
                 FROM dataset_versions ORDER BY id",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?;
            for row in rows {
                let (id, kind, coverage, sha) = row?;
                if RevisionKind::parse(&kind).is_none() {
                    push(
                        "error",
                        "dataset_versions",
                        id.to_string(),
                        format!("unknown revision_kind '{kind}'"),
                    );
                }
                if Coverage::parse(&coverage).is_none() {
                    push(
                        "error",
                        "dataset_versions",
                        id.to_string(),
                        format!("unknown coverage '{coverage}'"),
                    );
                }
                if sha.len() != 64 {
                    push(
                        "error",
                        "dataset_versions",
                        id.to_string(),
                        "content_sha256 must be a 64-char hex digest".into(),
                    );
                }
            }
        }

        // 17. Entity aliases: table membership.
        {
            let known = [
                "airports",
                "runways",
                "navaids",
                "waypoints",
                "airway_legs",
                "procedure_legs",
            ];
            let mut stmt = self.conn.prepare(
                "SELECT entity_table, entity_id FROM entity_aliases ORDER BY entity_table, entity_id",
            )?;
            let rows =
                stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            for row in rows {
                let (table, entity_id) = row?;
                if !known.contains(&table.as_str()) {
                    push(
                        "error",
                        "entity_aliases",
                        entity_id,
                        format!("unknown entity_table '{table}'"),
                    );
                }
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
            total_airway_legs: self.conn.query_row(
                "SELECT COUNT(*) FROM airway_legs;",
                [],
                |r| r.get(0),
            )?,
            total_procedure_legs: self.conn.query_row(
                "SELECT COUNT(*) FROM procedure_legs;",
                [],
                |r| r.get(0),
            )?,
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
                    elevation_ft, iso_country, municipality, valid_from
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
                ))
            },
        )
        .optional()?;

    let write = if let Some((ident, name, atype, lat, lon, elev, country, muni, prev_vf)) = existing
    {
        // Payload comparison excludes provenance (see entity_observations).
        let unchanged = ident == airport.ident
            && name == airport.name
            && atype == airport.airport_type
            && lat == airport.latitude
            && lon == airport.longitude
            && elev == airport.elevation_ft
            && country == airport.iso_country
            && muni == airport.municipality;
        if unchanged {
            EntityWrite::Unchanged
        } else {
            if prev_vf >= vf {
                return Err(anyhow!(
                    "airport '{id}': new valid_from {vf} must be strictly after existing revision {prev_vf}"
                ));
            }
            close_open_revision(conn, "airports", id, &prev_vf, &vf)?;
            insert_airport_row(conn, airport, &vf, &vu)?;
            EntityWrite::Updated
        }
    } else {
        insert_airport_row(conn, airport, &vf, &vu)?;
        EntityWrite::Created
    };

    record_observation(
        conn,
        "airports",
        id,
        &airport.temporal.source_snapshot_id.0,
        &vf,
    )?;
    Ok(write)
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
                    he_ident, he_lat, he_lon, he_elevation_ft, valid_from
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
                ))
            },
        )
        .optional()?;

    let write = if let Some((
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
        prev_vf,
    )) = existing
    {
        // Payload comparison excludes provenance (see entity_observations).
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
            && he_elev == runway.he_elevation_ft;
        if unchanged {
            EntityWrite::Unchanged
        } else {
            if prev_vf >= vf {
                return Err(anyhow!(
                    "runway '{id}': new valid_from {vf} must be strictly after existing revision {prev_vf}"
                ));
            }
            close_open_revision(conn, "runways", id, &prev_vf, &vf)?;
            insert_runway_row(conn, runway, &vf, &vu)?;
            EntityWrite::Updated
        }
    } else {
        insert_runway_row(conn, runway, &vf, &vu)?;
        EntityWrite::Created
    };

    record_observation(
        conn,
        "runways",
        id,
        &runway.temporal.source_snapshot_id.0,
        &vf,
    )?;
    Ok(write)
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
                    magnetic_variation_deg, slaved_variation_deg,
                    service_volume_nm, dme_paired, associated_runway,
                    localizer_bearing_true_deg, localizer_bearing_mag_deg,
                    glideslope_angle_deg, valid_from
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
                    row.get::<_, Option<f64>>(10)?,
                    row.get::<_, Option<i64>>(11)?,
                    row.get::<_, i64>(12)?,
                    row.get::<_, Option<String>>(13)?,
                    row.get::<_, Option<f64>>(14)?,
                    row.get::<_, Option<f64>>(15)?,
                    row.get::<_, Option<f64>>(16)?,
                    row.get::<_, String>(17)?,
                ))
            },
        )
        .optional()?;

    let write = if let Some((
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
        slaved,
        volume,
        paired,
        assoc_rwy,
        loc_true,
        loc_mag,
        gs,
        prev_vf,
    )) = existing
    {
        // Payload comparison deliberately excludes the source snapshot id:
        // provenance is tracked separately in entity_observations, so a new
        // dataset snapshot does not create semantic revisions for unchanged
        // entities.
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
            && slaved == navaid.slaved_variation_deg
            && volume.map(|v| v as u16) == navaid.service_volume_nm
            && (paired != 0) == navaid.dme_paired
            && assoc_rwy == navaid.associated_runway
            && loc_true == navaid.localizer_bearing_true_deg
            && loc_mag == navaid.localizer_bearing_mag_deg
            && gs == navaid.glideslope_angle_deg;
        if unchanged {
            EntityWrite::Unchanged
        } else {
            if prev_vf >= vf {
                return Err(anyhow!(
                    "navaid '{id}': new valid_from {vf} must be strictly after existing revision {prev_vf}"
                ));
            }
            close_open_revision(conn, "navaids", id, &prev_vf, &vf)?;
            insert_navaid_row(conn, navaid, &vf, &vu)?;
            EntityWrite::Updated
        }
    } else {
        insert_navaid_row(conn, navaid, &vf, &vu)?;
        EntityWrite::Created
    };

    record_observation(
        conn,
        "navaids",
        id,
        &navaid.temporal.source_snapshot_id.0,
        &vf,
    )?;
    Ok(write)
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
            magnetic_variation_deg, slaved_variation_deg, service_volume_nm,
            dme_paired, associated_runway, localizer_bearing_true_deg,
            localizer_bearing_mag_deg, glideslope_angle_deg,
            source_snapshot_id, valid_from, valid_until
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                  ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
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
            navaid.slaved_variation_deg,
            navaid.service_volume_nm,
            navaid.dme_paired as i64,
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

/// Log that `snapshot_id` observed entity `id` of `table` with this
/// `valid_from`. Provenance observations are separate from payload
/// revisions: re-observing an unchanged entity does not re-revise it.
pub fn record_observation(
    conn: &Connection,
    table: &str,
    entity_id: &str,
    snapshot_id: &str,
    valid_from: &str,
) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO entity_observations
            (source_snapshot_id, entity_table, entity_id, valid_from)
         VALUES (?1, ?2, ?3, ?4)",
        params![snapshot_id, table, entity_id, valid_from],
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
                    waypoint_type, terminal_area_ident, valid_from
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
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, String>(8)?,
                ))
            },
        )
        .optional()?;

    let write = if let Some((ident, name, lat, lon, region, enroute, wptype, term_area, prev_vf)) =
        existing
    {
        // Payload comparison excludes provenance (see entity_observations).
        let unchanged = ident == waypoint.ident
            && name == waypoint.name
            && lat == waypoint.latitude
            && lon == waypoint.longitude
            && region.as_deref().unwrap_or("") == waypoint.region_code
            && (enroute != 0) == waypoint.is_enroute
            && wptype.map(|v| v as u32) == waypoint.waypoint_type
            && term_area == waypoint.terminal_area_ident;
        if unchanged {
            EntityWrite::Unchanged
        } else {
            if prev_vf >= vf {
                return Err(anyhow!(
                    "waypoint '{id}': new valid_from {vf} must be strictly after existing revision {prev_vf}"
                ));
            }
            close_open_revision(conn, "waypoints", id, &prev_vf, &vf)?;
            insert_waypoint_row(conn, waypoint, &vf, &vu)?;
            EntityWrite::Updated
        }
    } else {
        insert_waypoint_row(conn, waypoint, &vf, &vu)?;
        EntityWrite::Created
    };

    record_observation(
        conn,
        "waypoints",
        id,
        &waypoint.temporal.source_snapshot_id.0,
        &vf,
    )?;
    Ok(write)
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
            is_enroute, waypoint_type, terminal_area_ident, source_snapshot_id,
            valid_from, valid_until
        ) VALUES (?1, ?2, ?3, ?4, ?5, 'WGS84', ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            waypoint.object_id.0,
            waypoint.ident,
            waypoint.name,
            waypoint.latitude,
            waypoint.longitude,
            waypoint.region_code,
            waypoint.is_enroute as i64,
            waypoint.waypoint_type.map(|v| v as i64),
            waypoint.terminal_area_ident,
            waypoint.temporal.source_snapshot_id.0,
            vf,
            vu,
        ],
    )?;
    Ok(())
}

pub fn insert_airway_leg_conn(conn: &Connection, leg: &CanonicalAirwayLeg) -> Result<EntityWrite> {
    validate_temporal(&leg.temporal)?;
    let id = &leg.object_id.0;
    let vf = rfc3339(leg.temporal.valid_from);
    let vu = leg.temporal.valid_until.map(rfc3339);

    let existing = conn
        .query_row(
            "SELECT route_ident, route_type, level, sequence_number,
                    start_fix, start_icao_code, end_fix, end_icao_code,
                    direction, minimum_altitude_ft, maximum_altitude_ft,
                    valid_from
             FROM airway_legs WHERE id = ?1 ORDER BY valid_from DESC LIMIT 1",
            params![id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, u32>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, Option<i64>>(9)?,
                    row.get::<_, Option<i64>>(10)?,
                    row.get::<_, String>(11)?,
                ))
            },
        )
        .optional()?;

    let write = if let Some((
        route_ident,
        route_type,
        level,
        sequence_number,
        start_fix,
        start_icao_code,
        end_fix,
        end_icao_code,
        direction,
        min_alt,
        max_alt,
        prev_vf,
    )) = existing
    {
        let unchanged = route_ident == leg.route_ident
            && route_type == leg.route_type
            && level.as_deref() == leg.level.map(|c| c.to_string()).as_deref()
            && sequence_number == leg.sequence_number
            && start_fix == leg.start_fix
            && start_icao_code == leg.start_icao_code
            && end_fix == leg.end_fix
            && end_icao_code == leg.end_icao_code
            && direction == leg.direction.to_string()
            && min_alt.map(|v| v as u32) == leg.minimum_altitude_ft
            && max_alt.map(|v| v as u32) == leg.maximum_altitude_ft;
        if unchanged {
            EntityWrite::Unchanged
        } else {
            if prev_vf >= vf {
                return Err(anyhow!(
                    "airway leg '{id}': new valid_from {vf} must be strictly after existing revision {prev_vf}"
                ));
            }
            close_open_revision(conn, "airway_legs", id, &prev_vf, &vf)?;
            insert_airway_leg_row(conn, leg, &vf, &vu)?;
            EntityWrite::Updated
        }
    } else {
        insert_airway_leg_row(conn, leg, &vf, &vu)?;
        EntityWrite::Created
    };

    record_observation(
        conn,
        "airway_legs",
        id,
        &leg.temporal.source_snapshot_id.0,
        &vf,
    )?;
    Ok(write)
}

fn insert_airway_leg_row(
    conn: &Connection,
    leg: &CanonicalAirwayLeg,
    vf: &str,
    vu: &Option<String>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO airway_legs (
            id, route_ident, route_type, level, sequence_number,
            start_fix, start_icao_code, end_fix, end_icao_code,
            direction, minimum_altitude_ft, maximum_altitude_ft,
            source_snapshot_id, valid_from, valid_until
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            leg.object_id.0,
            leg.route_ident,
            leg.route_type,
            leg.level.map(|c| c.to_string()),
            leg.sequence_number,
            leg.start_fix,
            leg.start_icao_code,
            leg.end_fix,
            leg.end_icao_code,
            leg.direction.to_string(),
            leg.minimum_altitude_ft,
            leg.maximum_altitude_ft,
            leg.temporal.source_snapshot_id.0,
            vf,
            vu,
        ],
    )?;
    Ok(())
}

/// Insert an entity revision, replacing a preloaded FUTURE revision with the
/// same `valid_from` (a correction made before the data became effective).
/// Once a revision is effective (valid_from <= now) it is immutable: a
/// conflicting write returns an error from the underlying insert.
pub fn insert_with_future_correction(
    conn: &Connection,
    table: &str,
    id: &str,
    valid_from: DateTime<Utc>,
    now: DateTime<Utc>,
    insert: impl FnOnce(&Connection) -> Result<EntityWrite>,
) -> Result<EntityWrite> {
    if valid_from > now {
        let vf = rfc3339(valid_from);
        let exists: bool = conn
            .query_row(
                &format!("SELECT 1 FROM {table} WHERE id = ?1 AND valid_from = ?2 LIMIT 1"),
                params![id, vf],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if exists {
            conn.execute(
                &format!("DELETE FROM {table} WHERE id = ?1 AND valid_from = ?2"),
                params![id, vf],
            )?;
        }
    }
    insert(conn)
}

pub fn insert_procedure_leg_conn(
    conn: &Connection,
    leg: &CanonicalProcedureLeg,
) -> Result<EntityWrite> {
    validate_temporal(&leg.temporal)?;
    let id = &leg.object_id.0;
    let vf = rfc3339(leg.temporal.valid_from);
    let vu = leg.temporal.valid_until.map(rfc3339);

    let existing = conn
        .query_row(
            "SELECT airport_ident, icao_code, procedure_kind, procedure_ident,
                    route_type, transition_ident, sequence_number, fix_ident,
                    fix_icao_code, fix_section, waypoint_description,
                    turn_direction, rnp_nm, path_terminator, recommended_navaid,
                    arc_radius_nm, course_a_deg, distance_a_nm, course_b_deg,
                    distance_b_nm, altitude_descriptor, altitude_1_ft,
                    altitude_2_ft, speed_limit_kts, course_c_deg,
                    vertical_angle_deg, msa_center_fix, route_qualifiers, raw,
                    valid_from
             FROM procedure_legs WHERE id = ?1 ORDER BY valid_from DESC LIMIT 1",
            params![id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, u32>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, Option<f64>>(12)?,
                    row.get::<_, String>(13)?,
                    row.get::<_, Option<String>>(14)?,
                    row.get::<_, Option<f64>>(15)?,
                    row.get::<_, Option<f64>>(16)?,
                    row.get::<_, Option<f64>>(17)?,
                    row.get::<_, Option<f64>>(18)?,
                    row.get::<_, Option<f64>>(19)?,
                    row.get::<_, Option<String>>(20)?,
                    row.get::<_, Option<i64>>(21)?,
                    row.get::<_, Option<i64>>(22)?,
                    row.get::<_, Option<i64>>(23)?,
                    row.get::<_, Option<i64>>(24)?,
                    row.get::<_, Option<f64>>(25)?,
                    row.get::<_, Option<String>>(26)?,
                    row.get::<_, String>(27)?,
                    row.get::<_, String>(28)?,
                    row.get::<_, String>(29)?,
                ))
            },
        )
        .optional()?;

    let write = if let Some((
        airport_ident,
        icao_code,
        procedure_kind,
        procedure_ident,
        route_type,
        transition_ident,
        sequence_number,
        fix_ident,
        fix_icao_code,
        fix_section,
        waypoint_description,
        turn_direction,
        rnp_nm,
        path_terminator,
        recommended_navaid,
        arc_radius_nm,
        course_a_deg,
        distance_a_nm,
        course_b_deg,
        distance_b_nm,
        altitude_descriptor,
        altitude_1_ft,
        altitude_2_ft,
        speed_limit_kts,
        course_c_deg,
        vertical_angle_deg,
        msa_center_fix,
        route_qualifiers,
        raw,
        prev_vf,
    )) = existing
    {
        // Payload comparison excludes provenance (see entity_observations).
        let unchanged = airport_ident == leg.airport_ident
            && icao_code == leg.icao_code
            && procedure_kind == leg.procedure_kind.to_string()
            && procedure_ident == leg.procedure_ident
            && route_type == leg.route_type
            && transition_ident == leg.transition_ident
            && sequence_number == leg.sequence_number
            && fix_ident == leg.fix_ident
            && fix_icao_code == leg.fix_icao_code
            && fix_section == leg.fix_section
            && waypoint_description == leg.waypoint_description
            && turn_direction.as_deref() == leg.turn_direction.map(|c| c.to_string()).as_deref()
            && rnp_nm == leg.rnp_nm
            && path_terminator == leg.path_terminator
            && recommended_navaid == leg.recommended_navaid
            && arc_radius_nm == leg.arc_radius_nm
            && course_a_deg == leg.course_a_deg
            && distance_a_nm == leg.distance_a_nm
            && course_b_deg == leg.course_b_deg
            && distance_b_nm == leg.distance_b_nm
            && altitude_descriptor.as_deref()
                == leg.altitude_descriptor.map(|c| c.to_string()).as_deref()
            && altitude_1_ft.map(|v| v as u32) == leg.altitude_1_ft
            && altitude_2_ft.map(|v| v as u32) == leg.altitude_2_ft
            && speed_limit_kts.map(|v| v as u32) == leg.speed_limit_kts
            && course_c_deg.map(|v| v as u32) == leg.course_c_deg
            && vertical_angle_deg == leg.vertical_angle_deg
            && msa_center_fix == leg.msa_center_fix
            && route_qualifiers == leg.route_qualifiers
            && raw == leg.raw;
        if unchanged {
            EntityWrite::Unchanged
        } else {
            if prev_vf >= vf {
                return Err(anyhow!(
                    "procedure leg '{id}': new valid_from {vf} must be strictly after existing revision {prev_vf}"
                ));
            }
            close_open_revision(conn, "procedure_legs", id, &prev_vf, &vf)?;
            insert_procedure_leg_row(conn, leg, &vf, &vu)?;
            EntityWrite::Updated
        }
    } else {
        insert_procedure_leg_row(conn, leg, &vf, &vu)?;
        EntityWrite::Created
    };

    record_observation(
        conn,
        "procedure_legs",
        id,
        &leg.temporal.source_snapshot_id.0,
        &vf,
    )?;
    Ok(write)
}

fn insert_procedure_leg_row(
    conn: &Connection,
    leg: &CanonicalProcedureLeg,
    vf: &str,
    vu: &Option<String>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO procedure_legs (
            id, airport_ident, icao_code, procedure_kind, procedure_ident,
            route_type, transition_ident, sequence_number, fix_ident,
            fix_icao_code, fix_section, waypoint_description, turn_direction,
            rnp_nm, path_terminator, recommended_navaid, arc_radius_nm,
            course_a_deg, distance_a_nm, course_b_deg, distance_b_nm,
            altitude_descriptor, altitude_1_ft, altitude_2_ft, speed_limit_kts,
            course_c_deg, vertical_angle_deg, msa_center_fix, route_qualifiers,
            raw, source_snapshot_id, valid_from, valid_until
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                  ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26,
                  ?27, ?28, ?29, ?30, ?31, ?32, ?33)",
        params![
            leg.object_id.0,
            leg.airport_ident,
            leg.icao_code,
            leg.procedure_kind.to_string(),
            leg.procedure_ident,
            leg.route_type,
            leg.transition_ident,
            leg.sequence_number,
            leg.fix_ident,
            leg.fix_icao_code,
            leg.fix_section,
            leg.waypoint_description,
            leg.turn_direction.map(|c| c.to_string()),
            leg.rnp_nm,
            leg.path_terminator,
            leg.recommended_navaid,
            leg.arc_radius_nm,
            leg.course_a_deg,
            leg.distance_a_nm,
            leg.course_b_deg,
            leg.distance_b_nm,
            leg.altitude_descriptor.map(|c| c.to_string()),
            leg.altitude_1_ft,
            leg.altitude_2_ft,
            leg.speed_limit_kts,
            leg.course_c_deg,
            leg.vertical_angle_deg,
            leg.msa_center_fix,
            leg.route_qualifiers,
            leg.raw,
            leg.temporal.source_snapshot_id.0,
            vf,
            vu,
        ],
    )?;
    Ok(())
}

/// Query procedure legs valid at a given UTC instant.
pub fn query_procedure_legs_at(
    conn: &Connection,
    date: DateTime<Utc>,
) -> Result<Vec<CanonicalProcedureLeg>> {
    let date_str = rfc3339(date);
    let mut stmt = conn.prepare(&format!(
        "SELECT id, airport_ident, icao_code, procedure_kind, procedure_ident,
                route_type, transition_ident, sequence_number, fix_ident,
                fix_icao_code, fix_section, waypoint_description,
                turn_direction, rnp_nm, path_terminator, recommended_navaid,
                arc_radius_nm, course_a_deg, distance_a_nm, course_b_deg,
                distance_b_nm, altitude_descriptor, altitude_1_ft,
                altitude_2_ft, speed_limit_kts, course_c_deg,
                vertical_angle_deg, msa_center_fix, route_qualifiers, raw,
                source_snapshot_id, valid_from, valid_until
         FROM procedure_legs WHERE {VALID_FROM_LE} AND {VALID_UNTIL_GT}
         ORDER BY airport_ident, procedure_ident, transition_ident, sequence_number;"
    ))?;

    let rows = stmt.query_and_then(params![date_str], |row| -> Result<CanonicalProcedureLeg> {
        Ok(CanonicalProcedureLeg {
            object_id: ProcedureLegId(row.get(0).context("procedure_legs.id")?),
            airport_ident: row.get(1).context("airport_ident")?,
            icao_code: row.get(2).context("icao_code")?,
            procedure_kind: row
                .get::<_, String>(3)
                .context("procedure_kind")?
                .chars()
                .next()
                .unwrap_or(' '),
            procedure_ident: row.get(4).context("procedure_ident")?,
            route_type: row.get(5).context("route_type")?,
            transition_ident: row.get(6).context("transition_ident")?,
            sequence_number: row.get(7).context("sequence_number")?,
            fix_ident: row.get(8).context("fix_ident")?,
            fix_icao_code: row.get(9).context("fix_icao_code")?,
            fix_section: row.get(10).context("fix_section")?,
            waypoint_description: row.get(11).context("waypoint_description")?,
            turn_direction: row
                .get::<_, Option<String>>(12)
                .context("turn_direction")?
                .and_then(|s| s.chars().next()),
            rnp_nm: row.get(13).context("rnp_nm")?,
            path_terminator: row.get(14).context("path_terminator")?,
            recommended_navaid: row.get(15).context("recommended_navaid")?,
            arc_radius_nm: row.get(16).context("arc_radius_nm")?,
            course_a_deg: row.get(17).context("course_a_deg")?,
            distance_a_nm: row.get(18).context("distance_a_nm")?,
            course_b_deg: row.get(19).context("course_b_deg")?,
            distance_b_nm: row.get(20).context("distance_b_nm")?,
            altitude_descriptor: row
                .get::<_, Option<String>>(21)
                .context("altitude_descriptor")?
                .and_then(|s| s.chars().next()),
            altitude_1_ft: row.get(22).context("altitude_1_ft")?,
            altitude_2_ft: row.get(23).context("altitude_2_ft")?,
            speed_limit_kts: row.get(24).context("speed_limit_kts")?,
            course_c_deg: row.get(25).context("course_c_deg")?,
            vertical_angle_deg: row.get(26).context("vertical_angle_deg")?,
            msa_center_fix: row.get(27).context("msa_center_fix")?,
            route_qualifiers: row.get(28).context("route_qualifiers")?,
            raw: row.get(29).context("raw")?,
            temporal: TemporalValidity {
                valid_from: parse_utc(
                    &row.get::<_, String>(31).context("valid_from")?,
                    "valid_from",
                )?,
                valid_until: row
                    .get::<_, Option<String>>(32)
                    .context("valid_until")?
                    .map(|s| parse_utc(&s, "valid_until"))
                    .transpose()?,
                source_snapshot_id: SourceSnapshotId(row.get(30).context("source_snapshot_id")?),
            },
        })
    })?;

    let mut legs = Vec::new();
    for row in rows {
        legs.push(row?);
    }
    Ok(legs)
}

// ---------------------------------------------------------------------------
// AIRAC lifecycle (v5) — connection-level implementation
// ---------------------------------------------------------------------------

/// The instant strictly before `t` in the store's RFC3339 representation.
///
/// One nanosecond — the smallest representable temporal step — so
/// `just_before(t)` is the latest instant that still compares strictly
/// less than `t` in the store's string serialization. Correct even at
/// sub-second boundaries (e.g. a cycle effective at
/// `09:00:00.000000001` probes the world at `09:00:00.000000000`).
/// Use this for every "world at effective_from − ε" probe; do NOT
/// hand-roll epsilon arithmetic elsewhere.
pub fn just_before(t: DateTime<Utc>) -> DateTime<Utc> {
    t - chrono::TimeDelta::nanoseconds(1)
}

pub fn insert_cycle_conn(conn: &Connection, cycle: &AiracCycle) -> Result<()> {
    conn.execute(
        "INSERT INTO airac_cycles
            (id, effective_from, effective_until, status, source_uri,
             created_at, updated_at, notes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            cycle.id.0,
            cycle.effective_from.map(rfc3339),
            cycle.effective_until.map(rfc3339),
            cycle.status.as_str(),
            cycle.source_uri,
            rfc3339(cycle.created_at),
            rfc3339(cycle.updated_at),
            cycle.notes,
        ],
    )
    .with_context(|| format!("inserting cycle '{}'", cycle.id.0))?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cycle_from_parts(
    id: String,
    effective_from: Option<String>,
    effective_until: Option<String>,
    status: String,
    source_uri: Option<String>,
    created_at: String,
    updated_at: String,
    notes: Option<String>,
) -> Result<AiracCycle> {
    Ok(AiracCycle {
        id: CycleId(id),
        effective_from: effective_from
            .map(|v| parse_utc(&v, "airac_cycles.effective_from"))
            .transpose()?,
        effective_until: effective_until
            .map(|v| parse_utc(&v, "airac_cycles.effective_until"))
            .transpose()?,
        status: CycleStatus::parse(&status)
            .ok_or_else(|| anyhow::anyhow!("unknown cycle status '{status}'"))?,
        source_uri,
        created_at: parse_utc(&created_at, "airac_cycles.created_at")?,
        updated_at: parse_utc(&updated_at, "airac_cycles.updated_at")?,
        notes,
    })
}

pub fn query_cycles_conn(conn: &Connection) -> Result<Vec<AiracCycle>> {
    let mut stmt = conn.prepare(
        "SELECT id, effective_from, effective_until, status, source_uri,
                created_at, updated_at, notes
         FROM airac_cycles ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, String>(6)?,
            r.get::<_, Option<String>>(7)?,
        ))
    })?;
    let mut cycles = Vec::new();
    for row in rows {
        let (id, ef, eu, st, uri, ca, ua, notes) = row?;
        cycles.push(cycle_from_parts(id, ef, eu, st, uri, ca, ua, notes)?);
    }
    Ok(cycles)
}

pub fn query_cycle_conn(conn: &Connection, id: &CycleId) -> Result<Option<AiracCycle>> {
    let mut stmt = conn.prepare(
        "SELECT id, effective_from, effective_until, status, source_uri,
                created_at, updated_at, notes
         FROM airac_cycles WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map(params![id.0], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, String>(6)?,
            r.get::<_, Option<String>>(7)?,
        ))
    })?;
    if let Some(row) = rows.next() {
        let (id, ef, eu, st, uri, ca, ua, notes) = row?;
        return Ok(Some(cycle_from_parts(id, ef, eu, st, uri, ca, ua, notes)?));
    }
    Ok(None)
}

pub fn set_cycle_status_conn(
    conn: &Connection,
    id: &CycleId,
    status: CycleStatus,
    now: DateTime<Utc>,
) -> Result<()> {
    let current = query_cycle_conn(conn, id)?
        .ok_or_else(|| anyhow::anyhow!("cycle '{}' not in catalog", id.0))?;
    if current.status == status {
        return Ok(());
    }
    if !CycleStatus::legal_transition(current.status, status) {
        anyhow::bail!(
            "illegal cycle transition {} -> {} for '{}'",
            current.status.as_str(),
            status.as_str(),
            id.0
        );
    }
    conn.execute(
        "UPDATE airac_cycles SET status = ?1, updated_at = ?2 WHERE id = ?3",
        params![status.as_str(), rfc3339(now), id.0],
    )
    .with_context(|| format!("updating cycle '{}' status", id.0))?;
    Ok(())
}

pub fn insert_cycle_snapshot_conn(
    conn: &Connection,
    cycle_id: &CycleId,
    snapshot_id: &SourceSnapshotId,
) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO cycle_snapshots (cycle_id, source_snapshot_id) VALUES (?1, ?2)",
        params![cycle_id.0, snapshot_id.0],
    )
    .with_context(|| {
        format!(
            "linking cycle '{}' to snapshot '{}'",
            cycle_id.0, snapshot_id.0
        )
    })?;
    Ok(())
}

pub fn cycle_snapshot_ids_conn(
    conn: &Connection,
    cycle_id: &CycleId,
) -> Result<Vec<SourceSnapshotId>> {
    let mut stmt = conn.prepare(
        "SELECT source_snapshot_id FROM cycle_snapshots WHERE cycle_id = ?1 ORDER BY source_snapshot_id",
    )?;
    let rows = stmt.query_map(params![cycle_id.0], |r| r.get::<_, String>(0))?;
    let mut ids = Vec::new();
    for row in rows {
        ids.push(SourceSnapshotId(row?));
    }
    Ok(ids)
}

/// Whether an event of `kind` was already recorded for the cycle
/// (idempotency check for Scheduled/Observed bookkeeping).
pub fn has_cycle_event_conn(
    conn: &Connection,
    cycle_id: &CycleId,
    kind: CycleEventKind,
) -> Result<bool> {
    let found = conn
        .query_row(
            "SELECT 1 FROM cycle_events WHERE cycle_id = ?1 AND kind = ?2 LIMIT 1",
            params![cycle_id.0, kind.as_str()],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    Ok(found)
}

pub fn record_cycle_event_conn(conn: &Connection, event: &CycleEvent) -> Result<i64> {
    conn.execute(
        "INSERT INTO cycle_events (at, kind, cycle_id, restored_cycle_id, notes)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            rfc3339(event.at),
            event.kind.as_str(),
            event.cycle_id.0,
            event.restored_cycle_id.as_ref().map(|c| c.0.as_str()),
            event.notes,
        ],
    )
    .with_context(|| format!("recording cycle event for '{}'", event.cycle_id.0))?;
    Ok(conn.last_insert_rowid())
}

pub fn query_cycle_events_conn(conn: &Connection) -> Result<Vec<CycleEvent>> {
    let mut stmt = conn.prepare(
        "SELECT id, at, kind, cycle_id, restored_cycle_id, notes
         FROM cycle_events ORDER BY at, id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, Option<String>>(5)?,
        ))
    })?;
    let mut events = Vec::new();
    for row in rows {
        let (id, at, kind, cycle_id, restored, notes) = row?;
        events.push(CycleEvent {
            id,
            at: parse_utc(&at, "cycle_events.at")?,
            kind: CycleEventKind::parse(&kind).unwrap_or(CycleEventKind::Scheduled),
            cycle_id: CycleId(cycle_id),
            restored_cycle_id: restored.map(CycleId),
            notes,
        });
    }
    Ok(events)
}

pub fn insert_dataset_version_conn(conn: &Connection, version: &DatasetVersion) -> Result<()> {
    conn.execute(
        "INSERT INTO dataset_versions
            (provider, dataset, airac_cycle, content_sha256, retrieved_at,
             revision_kind, coverage, notes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            version.provider,
            version.dataset,
            version.airac_cycle,
            version.content_sha256,
            rfc3339(version.retrieved_at),
            version.revision_kind.as_str(),
            version.coverage.as_str(),
            version.notes,
        ],
    )
    .context("inserting dataset version")?;
    Ok(())
}

pub fn latest_dataset_version_conn(
    conn: &Connection,
    provider: &str,
    dataset: &str,
    cycle: Option<&str>,
) -> Result<Option<DatasetVersion>> {
    let mut stmt = conn.prepare(
        "SELECT id, provider, dataset, airac_cycle, content_sha256, retrieved_at,
                revision_kind, coverage, notes
         FROM dataset_versions
         WHERE provider = ?1 AND dataset = ?2 AND airac_cycle IS ?3
         ORDER BY retrieved_at DESC, id DESC LIMIT 1",
    )?;
    let mut rows = stmt.query_map(params![provider, dataset, cycle], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, String>(6)?,
            r.get::<_, String>(7)?,
            r.get::<_, Option<String>>(8)?,
        ))
    })?;
    if let Some(row) = rows.next() {
        let (id, prov, ds, cycle_s, sha, retrieved, kind, cov, notes) = row?;
        return Ok(Some(DatasetVersion {
            id,
            provider: prov,
            dataset: ds,
            airac_cycle: cycle_s,
            content_sha256: sha,
            retrieved_at: parse_utc(&retrieved, "dataset_versions.retrieved_at")?,
            revision_kind: RevisionKind::parse(&kind)
                .ok_or_else(|| anyhow::anyhow!("unknown revision_kind '{kind}'"))?,
            coverage: Coverage::parse(&cov)
                .ok_or_else(|| anyhow::anyhow!("unknown coverage '{cov}'"))?,
            notes,
        }));
    }
    Ok(None)
}

/// Entity tables eligible for full-snapshot close semantics. This is the
/// closed set of names `close_absent_at`/`close_entity_at` accept — an
/// arbitrary string can never reach SQL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntityTable {
    Airports,
    Runways,
    Navaids,
    Waypoints,
    AirwayLegs,
    ProcedureLegs,
}

impl EntityTable {
    pub fn as_str(&self) -> &'static str {
        match self {
            EntityTable::Airports => "airports",
            EntityTable::Runways => "runways",
            EntityTable::Navaids => "navaids",
            EntityTable::Waypoints => "waypoints",
            EntityTable::AirwayLegs => "airway_legs",
            EntityTable::ProcedureLegs => "procedure_legs",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "airports" => Some(EntityTable::Airports),
            "runways" => Some(EntityTable::Runways),
            "navaids" => Some(EntityTable::Navaids),
            "waypoints" => Some(EntityTable::Waypoints),
            "airway_legs" => Some(EntityTable::AirwayLegs),
            "procedure_legs" => Some(EntityTable::ProcedureLegs),
            _ => None,
        }
    }

    pub fn all() -> &'static [EntityTable] {
        &[
            EntityTable::Airports,
            EntityTable::Runways,
            EntityTable::Navaids,
            EntityTable::Waypoints,
            EntityTable::AirwayLegs,
            EntityTable::ProcedureLegs,
        ]
    }
}

pub fn insert_entity_alias_conn(
    conn: &Connection,
    table: &str,
    natural_key: &str,
    provider: &str,
    entity_id: &str,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO entity_aliases (entity_table, natural_key, provider, entity_id)
         VALUES (?1, ?2, ?3, ?4)",
        params![table, natural_key, provider, entity_id],
    )
    .context("inserting entity alias")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Full-snapshot semantics (v0.4)
// ---------------------------------------------------------------------------

/// Close every open row of `table` in `namespace` that was valid before
/// `valid_from` and is NOT in `seen_ids`: a full-snapshot publication
/// ("here is the whole dataset") implicitly removes absent entities.
///
/// Semantics:
/// * Ownership scope is the object-id namespace (`<namespace>:%`); rows
///   of other providers are never touched.
/// * Only rows with `valid_until IS NULL AND valid_from < valid_from`
///   are candidates — future revisions and already-closed history are
///   never modified.
/// * `seen_ids` must contain every entity id the publication carries,
///   INCLUDING records the parser rejected/quarantined but could
///   identify. If the caller cannot guarantee that (unidentifiable
///   rejections), it MUST NOT call this function: a parser failure must
///   never silently become a source deletion.
/// * Idempotent: a second call with the same inputs closes nothing.
///
/// Uses a TEMP table (per-connection, dropped at end) instead of a
/// giant NOT IN list so large datasets stay scalable.
pub fn close_absent_at(
    conn: &Connection,
    table: EntityTable,
    namespace: &str,
    valid_from: DateTime<Utc>,
    seen_ids: &[String],
) -> Result<usize> {
    if !namespace
        .chars()
        .all(|c| c.is_ascii_lowercase() || c == '_')
    {
        anyhow::bail!("namespace '{namespace}' must be [a-z_]+");
    }
    let table = table.as_str();
    conn.execute_batch("CREATE TEMP TABLE ingest_seen (id TEXT PRIMARY KEY)")?;
    {
        let mut insert = conn.prepare("INSERT OR IGNORE INTO ingest_seen (id) VALUES (?1)")?;
        for chunk in seen_ids.chunks(512) {
            for id in chunk {
                insert.execute(params![id])?;
            }
        }
    }
    let vf = rfc3339(valid_from);
    let prefix = format!("{namespace}:%");
    let closed: usize = conn
        .execute(
            &format!(
                "UPDATE {table} SET valid_until = ?1
                 WHERE id LIKE ?2
                   AND valid_until IS NULL
                   AND valid_from < ?1
                   AND id NOT IN (SELECT id FROM ingest_seen)"
            ),
            params![vf, prefix],
        )
        .with_context(|| format!("closing absent rows of {table} for '{namespace}'"))?;
    conn.execute_batch("DROP TABLE ingest_seen")?;
    Ok(closed)
}

/// Tombstone-close one open row of `table` at `at` (a removal carried by
/// a Partial correction). Returns true when a row was closed. Idempotent:
/// a second call returns false.
pub fn close_entity_at(
    conn: &Connection,
    table: EntityTable,
    id: &str,
    at: DateTime<Utc>,
) -> Result<bool> {
    let table = table.as_str();
    let closed = conn
        .execute(
            &format!(
                "UPDATE {table} SET valid_until = ?1
                 WHERE id = ?2 AND valid_until IS NULL AND valid_from < ?1"
            ),
            params![rfc3339(at), id],
        )
        .with_context(|| format!("tombstone-closing '{id}' in {table}"))?;
    Ok(closed > 0)
}

// ---------------------------------------------------------------------------
// Cycle rollback (v0.4): re-publication of the pre-cycle state
// ---------------------------------------------------------------------------

/// Result of a rollback: how the pre-cycle world was re-published.
#[derive(Debug, Clone, PartialEq)]
pub struct RollbackReport {
    pub cycle_id: CycleId,
    /// The cycle whose state was re-published (max effective_from < the
    /// rolled-back cycle's effective_from). None when no earlier cycle
    /// exists (rolling back the first cycle).
    pub restored_cycle_id: Option<CycleId>,
    /// Entities that existed only in the rolled-back cycle: closed.
    pub added_closed: usize,
    /// Entities the cycle revised: closed + pre-cycle row re-published.
    pub changed_republished: usize,
    /// Entities the cycle removed: pre-cycle row re-published.
    pub removed_republished: usize,
    pub at: DateTime<Utc>,
    /// True when the cycle was already rolled back (idempotent no-op).
    pub noop: bool,
}

/// Roll one Active cycle back at instant `at`, restoring the world state
/// from immediately before the cycle became effective
/// (`world_at(just_before(effective_from))`), re-published as NEW
/// revisions with `valid_from = at`. History is never rewritten: rows
/// valid before `at` are untouched, and re-published rows carry the
/// historical row's exact provenance (`source_snapshot_id`).
///
/// Scope: ONLY the provider/dataset/entity domain owned by the cycle
/// (derived from `cycle_snapshots -> source_snapshots.provider` +
/// the provider manifest registry). Changes by other providers during
/// the cycle's window are never reverted.
pub fn rollback_cycle_conn(
    conn: &Connection,
    cycle_id: &CycleId,
    at: DateTime<Utc>,
) -> Result<RollbackReport> {
    let cycle = query_cycle_conn(conn, cycle_id)?
        .ok_or_else(|| anyhow::anyhow!("cycle '{}' not in catalog", cycle_id.0))?;

    if cycle.status == CycleStatus::RolledBack {
        let restored = query_cycle_events_conn(conn)?
            .into_iter()
            .rev()
            .find(|e| e.cycle_id == *cycle_id && e.kind == CycleEventKind::Rollback)
            .and_then(|e| e.restored_cycle_id);
        return Ok(RollbackReport {
            cycle_id: cycle_id.clone(),
            restored_cycle_id: restored,
            added_closed: 0,
            changed_republished: 0,
            removed_republished: 0,
            at,
            noop: true,
        });
    }
    if cycle.status != CycleStatus::Active {
        anyhow::bail!(
            "cycle '{}' is {}, only an Active cycle can be rolled back",
            cycle_id.0,
            cycle.status.as_str()
        );
    }
    let eff = cycle.effective_from.ok_or_else(|| {
        anyhow::anyhow!(
            "cycle '{}' has unconfirmed effective dates; cannot roll back",
            cycle_id.0
        )
    })?;

    // Ownership scope: provider/dataset/entity domain of the cycle.
    let mut ownership: Vec<(EntityTable, String)> = Vec::new();
    for snapshot_id in cycle_snapshot_ids_conn(conn, cycle_id)? {
        let provider: Option<String> = conn
            .query_row(
                "SELECT provider FROM source_snapshots WHERE id = ?1",
                params![snapshot_id.0],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(provider) = provider
            && let Some(manifest) = openairac_model::manifest_for_provider(&provider)
        {
            for table_name in manifest
                .datasets
                .iter()
                .flat_map(|d| d.entity_tables.iter().copied())
            {
                let table = EntityTable::parse(table_name).ok_or_else(|| {
                    anyhow::anyhow!(
                        "manifest table '{table_name}' of '{provider}' cannot be rolled back"
                    )
                })?;
                ownership.push((table, manifest.namespace.to_string()));
            }
        }
    }
    ownership.sort_by_key(|(t, n)| (t.as_str().to_string(), n.clone()));
    ownership.dedup();
    if ownership.is_empty() {
        anyhow::bail!(
            "cycle '{}' has no derivable ownership scope (no linked snapshots)",
            cycle_id.0
        );
    }

    // The cycle whose world is restored: latest earlier effective cycle.
    let restored_cycle_id = query_cycles_conn(conn)?
        .iter()
        .filter(|c| c.effective_from.map(|e| e < eff).unwrap_or(false))
        .max_by_key(|c| c.effective_from)
        .map(|c| c.id.clone());

    let pre_instant = just_before(eff);
    let cur_instant = just_before(at);

    let mut added = 0usize;
    let mut changed = 0usize;
    let mut removed = 0usize;
    for (table, namespace) in &ownership {
        let (a, c, r) = match table {
            EntityTable::Airports => rollback_table(
                conn,
                *table,
                namespace,
                eff,
                at,
                pre_instant,
                cur_instant,
                query_airports_at_conn,
                |a: &CanonicalAirport| a.id.0.clone(),
                |a: &CanonicalAirport| a.temporal.valid_from,
                |a: &CanonicalAirport, vf: DateTime<Utc>| {
                    let mut c = a.clone();
                    c.temporal.valid_from = vf;
                    c.temporal.valid_until = None;
                    c
                },
                insert_airport_row,
            )?,
            EntityTable::Runways => rollback_table(
                conn,
                *table,
                namespace,
                eff,
                at,
                pre_instant,
                cur_instant,
                |conn: &Connection, t: DateTime<Utc>| query_runways_conn(conn, t, None),
                |r: &CanonicalRunway| r.id.0.clone(),
                |r: &CanonicalRunway| r.temporal.valid_from,
                |r: &CanonicalRunway, vf: DateTime<Utc>| {
                    let mut c = r.clone();
                    c.temporal.valid_from = vf;
                    c.temporal.valid_until = None;
                    c
                },
                insert_runway_row,
            )?,
            EntityTable::Navaids => rollback_table(
                conn,
                *table,
                namespace,
                eff,
                at,
                pre_instant,
                cur_instant,
                query_navaids_at_conn,
                |n: &CanonicalNavaid| n.object_id.0.clone(),
                |n: &CanonicalNavaid| n.temporal.valid_from,
                |n: &CanonicalNavaid, vf: DateTime<Utc>| {
                    let mut c = n.clone();
                    c.temporal.valid_from = vf;
                    c.temporal.valid_until = None;
                    c
                },
                insert_navaid_row,
            )?,
            EntityTable::Waypoints => rollback_table(
                conn,
                *table,
                namespace,
                eff,
                at,
                pre_instant,
                cur_instant,
                query_waypoints_at_conn,
                |w: &CanonicalWaypoint| w.object_id.0.clone(),
                |w: &CanonicalWaypoint| w.temporal.valid_from,
                |w: &CanonicalWaypoint, vf: DateTime<Utc>| {
                    let mut c = w.clone();
                    c.temporal.valid_from = vf;
                    c.temporal.valid_until = None;
                    c
                },
                insert_waypoint_row,
            )?,
            EntityTable::AirwayLegs => rollback_table(
                conn,
                *table,
                namespace,
                eff,
                at,
                pre_instant,
                cur_instant,
                query_airway_legs_at,
                |l: &CanonicalAirwayLeg| l.object_id.0.clone(),
                |l: &CanonicalAirwayLeg| l.temporal.valid_from,
                |l: &CanonicalAirwayLeg, vf: DateTime<Utc>| {
                    let mut c = l.clone();
                    c.temporal.valid_from = vf;
                    c.temporal.valid_until = None;
                    c
                },
                insert_airway_leg_row,
            )?,
            EntityTable::ProcedureLegs => rollback_table(
                conn,
                *table,
                namespace,
                eff,
                at,
                pre_instant,
                cur_instant,
                query_procedure_legs_at,
                |l: &CanonicalProcedureLeg| l.object_id.0.clone(),
                |l: &CanonicalProcedureLeg| l.temporal.valid_from,
                |l: &CanonicalProcedureLeg, vf: DateTime<Utc>| {
                    let mut c = l.clone();
                    c.temporal.valid_from = vf;
                    c.temporal.valid_until = None;
                    c
                },
                insert_procedure_leg_row,
            )?,
        };
        added += a;
        changed += c;
        removed += r;
    }

    record_cycle_event_conn(
        conn,
        &CycleEvent {
            id: 0,
            at,
            kind: CycleEventKind::Rollback,
            cycle_id: cycle_id.clone(),
            restored_cycle_id: restored_cycle_id.clone(),
            notes: Some("re-published pre-cycle state".to_string()),
        },
    )?;
    set_cycle_status_conn(conn, cycle_id, CycleStatus::RolledBack, at)?;

    Ok(RollbackReport {
        cycle_id: cycle_id.clone(),
        restored_cycle_id,
        added_closed: added,
        changed_republished: changed,
        removed_republished: removed,
        at,
        noop: false,
    })
}

/// Generic re-publication over one entity table.
///
/// Classes per entity id (cur = row valid at `cur_instant`, pre = row
/// valid at `pre_instant`):
/// * Added   (cur owned by the cycle, no pre row): close cur at `at`.
/// * Changed (cur owned by the cycle, pre row exists): close cur via the
///   normal revision close + re-publish pre at `at` (exact provenance).
/// * Removed (pre row exists, no cur row): re-publish pre at `at`.
/// * Unchanged: untouched.
///
/// Rows owned by other providers (namespace mismatch) are never seen:
/// `load` results are filtered by the id prefix before diffing.
///
/// Re-publication uses the raw row-insert primitives (NOT the payload-
/// comparing insert_*_conn): a Removed entity re-publishes a payload
/// identical to its closed historical row, which the payload-comparison
/// would classify as Unchanged and skip — the temporal presence change
/// would be lost. Raw inserts also record no observations, matching the
/// design rule that rollback is not an observation of any source file.
#[allow(clippy::too_many_arguments)]
fn rollback_table<T: Clone>(
    conn: &Connection,
    table: EntityTable,
    namespace: &str,
    _eff: DateTime<Utc>,
    at: DateTime<Utc>,
    pre_instant: DateTime<Utc>,
    cur_instant: DateTime<Utc>,
    load: impl Fn(&Connection, DateTime<Utc>) -> Result<Vec<T>>,
    id_of: impl Fn(&T) -> String,
    vf_of: impl Fn(&T) -> DateTime<Utc>,
    republish: impl Fn(&T, DateTime<Utc>) -> T,
    force_insert: impl Fn(&Connection, &T, &str, &Option<String>) -> Result<()>,
) -> Result<(usize, usize, usize)> {
    let prefix = format!("{namespace}:");
    let pre: HashMap<String, T> = load(conn, pre_instant)?
        .into_iter()
        .filter(|e| id_of(e).starts_with(&prefix))
        .map(|e| (id_of(&e), e))
        .collect();
    let cur: HashMap<String, T> = load(conn, cur_instant)?
        .into_iter()
        .filter(|e| id_of(e).starts_with(&prefix))
        .map(|e| (id_of(&e), e))
        .collect();

    let eff = _eff;
    let mut added = 0usize;
    let mut changed = 0usize;
    let mut removed = 0usize;
    let at_str = rfc3339(at);

    for (id, c) in &cur {
        if vf_of(c) < eff {
            continue; // not owned by the rolled-back cycle
        }
        if let Some(p) = pre.get(id) {
            changed += 1;
            // Close the cycle's revision, then re-publish the pre-cycle
            // row as a brand-new revision at `at`.
            close_entity_at(conn, table, id, at)?;
            let row = republish(p, at);
            force_insert(conn, &row, &at_str, &None)?;
        } else {
            added += 1;
            close_entity_at(conn, table, id, at)?;
        }
    }
    for id in pre.keys() {
        if !cur.contains_key(id) {
            removed += 1;
            let row = republish(&pre[id], at);
            force_insert(conn, &row, &at_str, &None)?;
        }
    }

    Ok((added, changed, removed))
}

// ---------------------------------------------------------------------------
// Conn-level entity loaders (delegates for the WorldStore query methods)
// ---------------------------------------------------------------------------

/// Airports valid at `date`, with nested runways attached.
pub fn query_airports_at_conn(
    conn: &Connection,
    date: DateTime<Utc>,
) -> Result<Vec<CanonicalAirport>> {
    let date_str = rfc3339(date);
    let mut stmt = conn.prepare(&format!(
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
        airport.runways = query_runways_conn(conn, date, Some(&airport.id))?;
        airports.push(airport);
    }
    Ok(airports)
}

/// Navaids valid at `date`.
pub fn query_navaids_at_conn(
    conn: &Connection,
    date: DateTime<Utc>,
) -> Result<Vec<CanonicalNavaid>> {
    let date_str = rfc3339(date);
    let mut stmt = conn.prepare(&format!(
        "SELECT id, ident, name, navaid_type, frequency_khz, latitude_deg,
                longitude_deg, elevation_ft, region, associated_airport,
                magnetic_variation_deg, slaved_variation_deg,
                service_volume_nm, dme_paired, associated_runway,
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
            slaved_variation_deg: row.get(11).context("slaved_variation_deg")?,
            service_volume_nm: row.get(12).context("service_volume_nm")?,
            dme_paired: row.get::<_, i64>(13).context("dme_paired")? != 0,
            associated_runway: row.get(14).context("associated_runway")?,
            localizer_bearing_true_deg: row.get(15).context("localizer_bearing_true_deg")?,
            localizer_bearing_mag_deg: row.get(16).context("localizer_bearing_mag_deg")?,
            glideslope_angle_deg: row.get(17).context("glideslope_angle_deg")?,
            temporal: TemporalValidity {
                valid_from: parse_utc(
                    &row.get::<_, String>(19).context("valid_from")?,
                    "valid_from",
                )?,
                valid_until: row
                    .get::<_, Option<String>>(20)
                    .context("valid_until")?
                    .map(|s| parse_utc(&s, "valid_until"))
                    .transpose()?,
                source_snapshot_id: SourceSnapshotId(row.get(18).context("source_snapshot_id")?),
            },
        })
    })?;

    let mut navaids: Vec<CanonicalNavaid> = Vec::new();
    for row in rows {
        navaids.push(row?);
    }
    Ok(navaids)
}

/// Waypoints valid at `date`.
pub fn query_waypoints_at_conn(
    conn: &Connection,
    date: DateTime<Utc>,
) -> Result<Vec<CanonicalWaypoint>> {
    let date_str = rfc3339(date);
    let mut stmt = conn.prepare(&format!(
        "SELECT id, ident, name, latitude_deg, longitude_deg, region, is_enroute,
                waypoint_type, terminal_area_ident, source_snapshot_id,
                valid_from, valid_until
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
            terminal_area_ident: row.get(8).context("terminal_area_ident")?,
            temporal: TemporalValidity {
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
            },
        })
    })?;

    let mut waypoints = Vec::new();
    for row in rows {
        waypoints.push(row?);
    }
    Ok(waypoints)
}

/// Query airway legs valid at a given UTC instant, ordered by route.
pub fn query_airway_legs_at(
    conn: &Connection,
    date: DateTime<Utc>,
) -> Result<Vec<CanonicalAirwayLeg>> {
    let date_str = rfc3339(date);
    let mut stmt = conn.prepare(&format!(
        "SELECT id, route_ident, route_type, level, sequence_number,
                start_fix, start_icao_code, end_fix, end_icao_code,
                direction, minimum_altitude_ft, maximum_altitude_ft,
                source_snapshot_id, valid_from, valid_until
         FROM airway_legs WHERE {VALID_FROM_LE} AND {VALID_UNTIL_GT}
         ORDER BY route_ident, sequence_number;"
    ))?;

    let rows = stmt.query_and_then(params![date_str], |row| -> Result<CanonicalAirwayLeg> {
        Ok(CanonicalAirwayLeg {
            object_id: AirwayLegId(row.get(0).context("airway_legs.id")?),
            route_ident: row.get(1).context("route_ident")?,
            route_type: row.get(2).context("route_type")?,
            level: row
                .get::<_, Option<String>>(3)
                .context("level")?
                .and_then(|s| s.chars().next()),
            sequence_number: row.get(4).context("sequence_number")?,
            start_fix: row.get(5).context("start_fix")?,
            start_icao_code: row.get(6).context("start_icao_code")?,
            end_fix: row.get(7).context("end_fix")?,
            end_icao_code: row.get(8).context("end_icao_code")?,
            direction: row
                .get::<_, String>(9)
                .context("direction")?
                .chars()
                .next()
                .unwrap_or('N'),
            minimum_altitude_ft: row.get(10).context("minimum_altitude_ft")?,
            maximum_altitude_ft: row.get(11).context("maximum_altitude_ft")?,
            temporal: TemporalValidity {
                valid_from: parse_utc(
                    &row.get::<_, String>(13).context("valid_from")?,
                    "valid_from",
                )?,
                valid_until: row
                    .get::<_, Option<String>>(14)
                    .context("valid_until")?
                    .map(|s| parse_utc(&s, "valid_until"))
                    .transpose()?,
                source_snapshot_id: SourceSnapshotId(row.get(12).context("source_snapshot_id")?),
            },
        })
    })?;

    let mut legs = Vec::new();
    for row in rows {
        legs.push(row?);
    }
    Ok(legs)
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
        assert_eq!(status.migration_version, 5);
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
    fn test_validate_procedure_and_airway_checks() {
        let mut store = WorldStore::open_in_memory().unwrap();
        store.insert_source_snapshot(&snapshot("snap-001")).unwrap();
        let t = Utc::now();

        let mut counter = 0u32;
        let mut ple = |seq: u32, term: &str| {
            counter += 1;
            let id = format!("faa:PD:KSFO:D:CIITY3:{seq}:{counter}");
            CanonicalProcedureLeg {
                object_id: ProcedureLegId(id),
                airport_ident: "KSFO".to_string(),
                icao_code: "K2".to_string(),
                procedure_kind: 'D',
                procedure_ident: "CIITY3".to_string(),
                route_type: String::new(),
                transition_ident: String::new(),
                sequence_number: seq,
                fix_ident: "NOFIX".to_string(),
                fix_icao_code: "K2".to_string(),
                fix_section: " ".to_string(),
                waypoint_description: "E ".to_string(),
                turn_direction: None,
                rnp_nm: None,
                path_terminator: term.to_string(),
                recommended_navaid: None,
                arc_radius_nm: None,
                course_a_deg: None,
                distance_a_nm: None,
                course_b_deg: None,
                distance_b_nm: None,
                altitude_descriptor: Some('B'),
                altitude_1_ft: Some(5000),
                altitude_2_ft: None,
                speed_limit_kts: None,
                course_c_deg: None,
                vertical_angle_deg: None,
                msa_center_fix: None,
                route_qualifiers: String::new(),
                raw: String::new(),
                temporal: TemporalValidity {
                    valid_from: t,
                    valid_until: None,
                    source_snapshot_id: SourceSnapshotId("snap-001".to_string()),
                },
            }
        };
        // Unknown terminator + incomplete band + missing fix row.
        store
            .transact(|conn| insert_procedure_leg_conn(conn, &ple(10, "ZZ")))
            .unwrap();
        // Duplicate sequence in the same procedure group.
        store
            .transact(|conn| insert_procedure_leg_conn(conn, &ple(10, "TF")))
            .unwrap();
        // Airway leg whose endpoints have no waypoint rows.
        let leg = CanonicalAirwayLeg {
            object_id: AirwayLegId("faa:ER:V9:2".to_string()),
            route_ident: "V9".to_string(),
            route_type: "O".to_string(),
            level: Some('L'),
            sequence_number: 2,
            start_fix: "GHOST".to_string(),
            start_icao_code: "K2".to_string(),
            end_fix: "NOWHERE".to_string(),
            end_icao_code: "K2".to_string(),
            direction: 'N',
            minimum_altitude_ft: None,
            maximum_altitude_ft: None,
            temporal: TemporalValidity {
                valid_from: t,
                valid_until: None,
                source_snapshot_id: SourceSnapshotId("snap-001".to_string()),
            },
        };
        store
            .transact(|conn| insert_airway_leg_conn(conn, &leg))
            .unwrap();

        let issues = store.validate().unwrap();
        let mut tables: Vec<(&str, String)> = issues
            .iter()
            .map(|i| (i.table.as_str(), i.message.clone()))
            .collect();
        tables.sort();
        assert!(
            tables
                .iter()
                .any(|(tb, m)| *tb == "procedure_legs" && m.contains("not in the ARINC 424 set")),
            "{tables:?}"
        );
        assert!(
            tables
                .iter()
                .any(|(tb, m)| *tb == "procedure_legs" && m.contains("requires both altitudes")),
            "{tables:?}"
        );
        assert!(
            tables
                .iter()
                .any(|(tb, m)| *tb == "procedure_legs" && m.contains("duplicated")),
            "{tables:?}"
        );
        assert!(
            tables
                .iter()
                .any(|(tb, m)| *tb == "procedure_legs"
                    && m.contains("has no waypoint or navaid row")),
            "{tables:?}"
        );
        assert!(
            tables
                .iter()
                .any(|(tb, m)| *tb == "airway_legs" && m.contains("has no waypoint row")),
            "{tables:?}"
        );
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

    #[test]
    fn test_snapshot_change_does_not_revise_unchanged_entities() {
        let mut store = WorldStore::open_in_memory().unwrap();
        store.insert_source_snapshot(&snapshot("snap-A")).unwrap();
        store.insert_source_snapshot(&snapshot("snap-B")).unwrap();

        let t0 = Utc::now();
        let t1 = t0 + Duration::from_secs(3600);

        // Snapshot A: 100 airports.
        store
            .transact(|conn| {
                for i in 0..100 {
                    let ap = airport(&format!("APT{i:03}"), &format!("APT{i:03}"), t0, "snap-A");
                    insert_airport_conn(conn, &ap)?;
                }
                Ok(())
            })
            .unwrap();

        // Snapshot B: same payloads, new snapshot id, one airport changed.
        store
            .transact(|conn| {
                for i in 0..100 {
                    let mut ap =
                        airport(&format!("APT{i:03}"), &format!("APT{i:03}"), t1, "snap-B");
                    if i == 42 {
                        ap.name = "Changed Airport".to_string();
                    }
                    insert_airport_conn(conn, &ap)?;
                }
                Ok(())
            })
            .unwrap();

        // Only ONE new payload revision, not 100.
        let total: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM airports", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total, 101);

        // Provenance: snapshot B observed all 100 entities.
        let observed: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM entity_observations WHERE source_snapshot_id = 'snap-B'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(observed, 100);

        // The changed entity has two revisions; an unchanged one has one.
        let revisions_changed: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM airports WHERE id = 'APT042'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(revisions_changed, 2);
        let revisions_same: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM airports WHERE id = 'APT000'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(revisions_same, 1);
    }

    #[test]
    fn test_future_preload_correction() {
        let mut store = WorldStore::open_in_memory().unwrap();
        store.insert_source_snapshot(&snapshot("snap-001")).unwrap();

        let now = Utc::now();
        let t_future = now + Duration::from_secs(3600);

        // Preload a future revision.
        let ap = airport("KSFO", "KSFO", t_future, "snap-001");
        assert_eq!(store.insert_airport(&ap).unwrap(), EntityWrite::Created);

        // Correct it before it becomes effective: same valid_from replaced.
        let mut corrected = airport("KSFO", "KSFO", t_future, "snap-001");
        corrected.latitude = 37.7;
        store
            .transact(|conn| {
                insert_with_future_correction(
                    conn,
                    "airports",
                    "KSFO",
                    t_future,
                    now,
                    |c| -> Result<EntityWrite> { insert_airport_conn(c, &corrected) },
                )
            })
            .unwrap();
        let mut store = store;

        let total: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM airports WHERE id = 'KSFO'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(total, 1);
        let at = store.query_airports_at(t_future).unwrap();
        assert_eq!(at[0].latitude, 37.7);

        // Once effective, a same-valid_from rewrite must fail.
        let mut late = airport("KSFO", "KSFO", t_future, "snap-001");
        late.latitude = 37.8;
        let result = store.transact(|conn| {
            insert_with_future_correction(
                conn,
                "airports",
                "KSFO",
                t_future,
                t_future + Duration::from_secs(10),
                |c| insert_airport_conn(c, &late),
            )
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_v1_database_migrates_to_v3() {
        // Build a genuine v1 database with the historical schema and data.
        let path = std::env::temp_dir().join(format!(
            "openairac_v1_migration_{}.sqlite",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(include_str!("../migrations/v1_init.sql"))
                .unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
            conn.execute(
                "INSERT INTO source_snapshots
                    (id, provider, dataset, retrieved_at, source_uri, content_sha256, parser_version)
                 VALUES ('snap-v1', 'Test', 'airports', '2026-01-01T00:00:00+00:00',
                         'http://v1', 'hash', '0.1.0')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO airports
                    (id, ident, name, airport_type, latitude_deg, longitude_deg,
                     source_snapshot_id, valid_from, valid_until)
                 VALUES ('KSFO', 'KSFO', 'San Francisco', 'large_airport', 37.6188, -122.375,
                         'snap-v1', '2026-01-01T00:00:00+00:00', NULL)",
                [],
            )
            .unwrap();
        }

        // Opening with the current code must migrate v1 -> v3 in place.
        let store = WorldStore::open(&path).unwrap();
        assert_eq!(store.migration_version().unwrap(), 5);
        let at = store.query_airports_at(Utc::now()).unwrap();
        assert_eq!(at.len(), 1);
        assert_eq!(at[0].ident, "KSFO");

        // The migrated database accepts new temporal revisions.
        store.insert_source_snapshot(&snapshot("snap-v2")).unwrap();
        let t = Utc::now();
        let ap = airport("KSFO", "KSFO", t, "snap-v2");
        assert_eq!(store.insert_airport(&ap).unwrap(), EntityWrite::Updated);
        assert!(store.validate().unwrap().is_empty());

        drop(store);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }

    #[test]
    fn test_airway_leg_roundtrip() {
        let mut store = WorldStore::open_in_memory().unwrap();
        store.insert_source_snapshot(&snapshot("snap-001")).unwrap();

        let t = Utc::now();
        let leg = CanonicalAirwayLeg {
            object_id: AirwayLegId("faa:ER:V257:2".to_string()),
            route_ident: "V257".to_string(),
            route_type: "O".to_string(),
            level: Some('L'),
            sequence_number: 2,
            start_fix: "AADCO".to_string(),
            start_icao_code: "K2".to_string(),
            end_fix: "LOLIC".to_string(),
            end_icao_code: "K2".to_string(),
            direction: 'N',
            minimum_altitude_ft: Some(11_500),
            maximum_altitude_ft: Some(17_500),
            temporal: TemporalValidity {
                valid_from: t,
                valid_until: None,
                source_snapshot_id: SourceSnapshotId("snap-001".to_string()),
            },
        };
        // Endpoint fixes exist in the world so referential validation is
        // clean.
        for fix in ["AADCO", "LOLIC"] {
            store
                .insert_waypoint(&CanonicalWaypoint {
                    object_id: WaypointId(format!("WP-{fix}")),
                    ident: fix.to_string(),
                    name: fix.to_string(),
                    latitude: 39.0,
                    longitude: -110.0,
                    is_enroute: true,
                    region_code: "K2".to_string(),
                    terminal_area_ident: None,
                    waypoint_type: Some(0x202057),
                    temporal: TemporalValidity {
                        valid_from: t,
                        valid_until: None,
                        source_snapshot_id: SourceSnapshotId("snap-001".to_string()),
                    },
                })
                .unwrap();
        }
        store
            .transact(|conn| insert_airway_leg_conn(conn, &leg))
            .unwrap();

        let legs = query_airway_legs_at(&store.conn, t).unwrap();
        assert_eq!(legs.len(), 1);
        assert_eq!(legs[0].route_ident, "V257");
        assert_eq!(legs[0].end_fix, "LOLIC");
        assert_eq!(store.status().unwrap().total_airway_legs, 1);
        assert!(store.validate().unwrap().is_empty());
    }

    // -------------------------------------------------------------------
    // v0.4 lifecycle tests
    // -------------------------------------------------------------------

    fn cycle(id: &str, eff: Option<DateTime<Utc>>, status: CycleStatus) -> AiracCycle {
        let now = Utc::now();
        AiracCycle {
            id: CycleId(id.to_string()),
            effective_from: eff,
            effective_until: None,
            status,
            source_uri: Some(format!("https://example.invalid/{id}")),
            created_at: now,
            updated_at: now,
            notes: None,
        }
    }

    #[test]
    fn test_just_before_is_strictly_before() {
        // Sub-second boundaries: the smallest representable epsilon must
        // still land strictly before the instant in string form.
        let samples = [
            Utc::now(),
            DateTime::parse_from_rfc3339("2026-08-06T09:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            DateTime::parse_from_rfc3339("2026-08-06T09:00:00.5Z")
                .unwrap()
                .with_timezone(&Utc),
            // One nanosecond after a whole second: the probe must be the
            // whole second itself, not the previous second.
            DateTime::parse_from_rfc3339("2026-08-06T09:00:00.000000001Z")
                .unwrap()
                .with_timezone(&Utc),
            // Nanosecond value with trailing-zero elision.
            DateTime::parse_from_rfc3339("2026-08-06T09:00:00.100000000Z")
                .unwrap()
                .with_timezone(&Utc),
        ];
        for t in samples {
            let before = just_before(t);
            assert!(before < t, "{before} < {t}");
            assert!(
                rfc3339(before) < rfc3339(t),
                "rfc3339({before}) < rfc3339({t})"
            );
            // The probe lands within the same second when t has a
            // sub-second component (smallest-epsilon property).
            if t.timestamp_subsec_nanos() > 0 {
                assert_eq!(before.timestamp(), t.timestamp());
            }
        }
    }

    #[test]
    fn test_cycle_catalog_roundtrip_and_transitions() {
        let store = WorldStore::open_in_memory().unwrap();
        let t = Utc::now();
        let c = cycle("2608", Some(t), CycleStatus::Discovered);
        store.insert_cycle(&c).unwrap();
        assert_eq!(store.query_cycles().unwrap().len(), 1);
        let got = store.query_cycle(&CycleId("2608".into())).unwrap().unwrap();
        assert_eq!(got.status, CycleStatus::Discovered);
        assert_eq!(got.effective_from, Some(t));

        // Legal transitions.
        store
            .set_cycle_status(&CycleId("2608".into()), CycleStatus::Preloaded)
            .unwrap();
        store
            .set_cycle_status(&CycleId("2608".into()), CycleStatus::Active)
            .unwrap();
        store
            .set_cycle_status(&CycleId("2608".into()), CycleStatus::Superseded)
            .unwrap();
        // Terminal: no further transitions.
        let err = store
            .set_cycle_status(&CycleId("2608".into()), CycleStatus::Active)
            .unwrap_err();
        assert!(
            err.to_string().contains("illegal cycle transition"),
            "{err}"
        );
        // Same-status set is a no-op.
        store
            .set_cycle_status(&CycleId("2608".into()), CycleStatus::Superseded)
            .unwrap();
    }

    #[test]
    fn test_cycle_snapshots_and_events_roundtrip() {
        let store = WorldStore::open_in_memory().unwrap();
        let snap = snapshot("snap-001");
        store.insert_source_snapshot(&snap).unwrap();
        let t = Utc::now();
        store
            .insert_cycle(&cycle("2608", Some(t), CycleStatus::Preloaded))
            .unwrap();
        store
            .insert_cycle_snapshot(&CycleId("2608".into()), &snap.id)
            .unwrap();
        store
            .insert_cycle_snapshot(&CycleId("2608".into()), &snap.id)
            .unwrap(); // idempotent
        assert_eq!(
            store.cycle_snapshot_ids(&CycleId("2608".into())).unwrap(),
            vec![SourceSnapshotId("snap-001".into())]
        );

        let event = CycleEvent {
            id: 0,
            at: t,
            kind: CycleEventKind::Scheduled,
            cycle_id: CycleId("2608".into()),
            restored_cycle_id: None,
            notes: Some("preload".into()),
        };
        let id = store.record_cycle_event(&event).unwrap();
        assert!(id > 0);
        let events = store.query_cycle_events().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, CycleEventKind::Scheduled);

        // Rollback without restored_cycle_id violates the invariant.
        store
            .record_cycle_event(&CycleEvent {
                id: 0,
                at: t,
                kind: CycleEventKind::Rollback,
                cycle_id: CycleId("2608".into()),
                restored_cycle_id: None,
                notes: None,
            })
            .unwrap();
        let issues = store.validate().unwrap();
        assert!(
            issues
                .iter()
                .any(|i| i.table == "cycle_events" && i.message.contains("restored cycle")),
            "{issues:?}"
        );
    }

    #[test]
    fn test_dataset_versions_append_only() {
        let store = WorldStore::open_in_memory().unwrap();
        let t0 = Utc::now();
        let t1 = t0 + Duration::from_secs(60);
        let v1 = DatasetVersion {
            id: 0,
            provider: "FAA_CIFP".to_string(),
            dataset: "FAACIFP18".to_string(),
            airac_cycle: Some("2608".to_string()),
            content_sha256: "a".repeat(64),
            retrieved_at: t0,
            revision_kind: RevisionKind::Baseline,
            coverage: Coverage::FullSnapshot,
            notes: None,
        };
        let mut v2 = v1.clone();
        v2.content_sha256 = "b".repeat(64);
        v2.retrieved_at = t1;
        v2.revision_kind = RevisionKind::Correction;
        store.insert_dataset_version(&v1).unwrap();
        store.insert_dataset_version(&v2).unwrap();

        let latest = store
            .latest_dataset_version("FAA_CIFP", "FAACIFP18", Some("2608"))
            .unwrap()
            .unwrap();
        assert_eq!(latest.content_sha256, "b".repeat(64));
        assert_eq!(latest.revision_kind, RevisionKind::Correction);
        assert_eq!(latest.coverage, Coverage::FullSnapshot);
        assert!(
            store
                .latest_dataset_version("OurAirports", "FAACIFP18", Some("2608"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn test_close_absent_at_semantics() {
        let mut store = WorldStore::open_in_memory().unwrap();
        store.insert_source_snapshot(&snapshot("snap-001")).unwrap();
        let t0 = Utc::now();
        let t1 = t0 + Duration::from_secs(3600);
        let t2 = t1 + Duration::from_secs(3600);

        let ap = |id: &str, vf: DateTime<Utc>| airport(id, id, vf, "snap-001");
        store.insert_airport(&ap("faa:A", t0)).unwrap();
        store.insert_airport(&ap("faa:B", t0)).unwrap();
        // faa:E was rejected by the parser but its id was identified: it
        // must appear in the seen set and stay open.
        store.insert_airport(&ap("faa:E", t0)).unwrap();
        store.insert_airport(&ap("ourairports:C", t0)).unwrap();

        // New full snapshot at t1: A and E present (E via the rejected
        // record), B absent. B closes, A and E stay.
        let closed = store
            .transact(|conn| {
                close_absent_at(
                    conn,
                    EntityTable::Airports,
                    "faa",
                    t1,
                    &["faa:A".to_string(), "faa:E".to_string()],
                )
            })
            .unwrap();
        assert_eq!(closed, 1);
        let at_t1 = store.query_airports_at(t1).unwrap();
        assert_eq!(at_t1.len(), 3); // faa:A + faa:E + ourairports:C
        assert!(at_t1.iter().any(|a| a.ident == "faa:A"));
        assert!(at_t1.iter().any(|a| a.ident == "faa:E")); // rejected-present protection
        assert!(at_t1.iter().any(|a| a.ident == "ourairports:C"));
        assert!(!at_t1.iter().any(|a| a.ident == "faa:B"));

        // Idempotent: second call closes nothing.
        let again = store
            .transact(|conn| {
                close_absent_at(
                    conn,
                    EntityTable::Airports,
                    "faa",
                    t1,
                    &["faa:A".to_string(), "faa:E".to_string()],
                )
            })
            .unwrap();
        assert_eq!(again, 0);

        // A row created later in the cycle (valid_from = t2) is untouched.
        store.insert_airport(&ap("faa:D", t2)).unwrap();
        let closed2 = store
            .transact(|conn| {
                close_absent_at(
                    conn,
                    EntityTable::Airports,
                    "faa",
                    t1,
                    &["faa:A".to_string(), "faa:E".to_string()],
                )
            })
            .unwrap();
        assert_eq!(closed2, 0);
        let at_t2 = store.query_airports_at(t2).unwrap();
        assert!(at_t2.iter().any(|a| a.ident == "faa:D"));
    }

    #[test]
    fn test_close_entity_at_tombstone() {
        let mut store = WorldStore::open_in_memory().unwrap();
        store.insert_source_snapshot(&snapshot("snap-001")).unwrap();
        let t0 = Utc::now();
        let t1 = t0 + Duration::from_secs(3600);
        store
            .insert_airport(&airport("faa:A", "faa:A", t0, "snap-001"))
            .unwrap();

        let closed = store
            .transact(|conn| close_entity_at(conn, EntityTable::Airports, "faa:A", t1))
            .unwrap();
        assert!(closed);
        assert!(store.query_airports_at(t1).unwrap().is_empty());
        assert_eq!(store.query_airports_at(t0).unwrap().len(), 1); // history intact

        // Idempotent.
        let again = store
            .transact(|conn| close_entity_at(conn, EntityTable::Airports, "faa:A", t1))
            .unwrap();
        assert!(!again);
    }

    #[test]
    fn test_rollback_cycle_classes_and_isolation() {
        let mut store = WorldStore::open_in_memory().unwrap();
        let t0 = Utc::now();
        let t1 = t0 + Duration::from_secs(3600); // 2607 effective
        let t15 = t1 + Duration::from_secs(1800); // provider B update inside 2608 window
        let t2 = t1 + Duration::from_secs(3600); // 2608 effective
        let t3 = t2 + Duration::from_secs(3600); // rollback instant

        // Snapshots: two FAA_CIFP cycles + OurAirports provider B.
        let mut snap_a = snapshot("snap-2607");
        snap_a.provider = "FAA_CIFP".to_string();
        snap_a.dataset = "FAACIFP18".to_string();
        let mut snap_b = snapshot("snap-2608");
        snap_b.provider = "FAA_CIFP".to_string();
        snap_b.dataset = "FAACIFP18".to_string();
        let mut snap_b_oa = snapshot("snap-B");
        snap_b_oa.provider = "OurAirports".to_string();
        snap_b_oa.dataset = "navaids".to_string();
        for snap in [&snap_a, &snap_b, &snap_b_oa] {
            store.insert_source_snapshot(snap).unwrap();
        }
        store
            .insert_cycle(&cycle("2607", Some(t1), CycleStatus::Superseded))
            .unwrap();
        store
            .insert_cycle(&cycle("2608", Some(t2), CycleStatus::Active))
            .unwrap();
        store
            .insert_cycle_snapshot(&CycleId("2608".into()), &snap_b.id)
            .unwrap();

        let wp = |id: &str, lat: f64, vf: DateTime<Utc>, snap: &str| CanonicalWaypoint {
            object_id: WaypointId(id.to_string()),
            ident: id.to_string(),
            name: id.to_string(),
            latitude: lat,
            longitude: -100.0,
            is_enroute: true,
            region_code: "K2".to_string(),
            terminal_area_ident: None,
            waypoint_type: Some(0x202057),
            temporal: TemporalValidity {
                valid_from: vf,
                valid_until: None,
                source_snapshot_id: SourceSnapshotId(snap.to_string()),
            },
        };

        // 2607 world (namespace faa).
        store
            .insert_waypoint(&wp("faa:WP_CHG", 10.0, t1, "snap-2607"))
            .unwrap();
        store
            .insert_waypoint(&wp("faa:WP_REM", 11.0, t1, "snap-2607"))
            .unwrap();
        store
            .insert_waypoint(&wp("faa:WP_KEEP", 12.0, t1, "snap-2607"))
            .unwrap();
        // Provider B updates its own namespace during 2608's window.
        store
            .insert_waypoint(&wp("ourairports:WP_B", 50.0, t0, "snap-B"))
            .unwrap();
        store
            .insert_waypoint(&wp("ourairports:WP_B", 51.0, t15, "snap-B"))
            .unwrap();

        // 2608: CHG revised, ADD introduced, REM gone from the snapshot.
        store
            .insert_waypoint(&wp("faa:WP_CHG", 20.0, t2, "snap-2608"))
            .unwrap();
        store
            .insert_waypoint(&wp("faa:WP_ADD", 21.0, t2, "snap-2608"))
            .unwrap();
        store
            .transact(|conn| {
                close_absent_at(
                    conn,
                    EntityTable::Waypoints,
                    "faa",
                    t2,
                    &[
                        "faa:WP_CHG".to_string(),
                        "faa:WP_KEEP".to_string(),
                        "faa:WP_ADD".to_string(),
                    ],
                )
            })
            .unwrap();

        // Sanity: the 2608 world is as expected.
        let at_2608 = store
            .query_waypoints_at(t2 + Duration::from_secs(60))
            .unwrap();
        assert!(
            at_2608
                .iter()
                .any(|w| w.ident == "faa:WP_CHG" && w.latitude == 20.0)
        );
        assert!(!at_2608.iter().any(|w| w.ident == "faa:WP_REM"));

        let report = store.rollback_cycle(&CycleId("2608".into()), t3).unwrap();
        assert!(!report.noop);
        assert_eq!(report.restored_cycle_id, Some(CycleId("2607".into())));
        assert_eq!(report.added_closed, 1); // WP_ADD
        assert_eq!(report.changed_republished, 1); // WP_CHG
        assert_eq!(report.removed_republished, 1); // WP_REM

        // Post-rollback world: 2607 state, not the 2608 state.
        let after = store.query_waypoints_at(t3).unwrap();
        assert!(
            after
                .iter()
                .any(|w| w.ident == "faa:WP_CHG" && w.latitude == 10.0)
        );
        assert!(after.iter().any(|w| w.ident == "faa:WP_REM"));
        assert!(!after.iter().any(|w| w.ident == "faa:WP_ADD"));
        assert!(after.iter().any(|w| w.ident == "faa:WP_KEEP"));
        // Isolation: provider B's mid-window update is untouched.
        let wp_b = after
            .iter()
            .find(|w| w.ident == "ourairports:WP_B")
            .unwrap();
        assert_eq!(wp_b.latitude, 51.0);
        assert_eq!(wp_b.temporal.valid_from, t15);

        // History before the rollback instant is unchanged.
        let historic = store
            .query_waypoints_at(t2 + Duration::from_secs(60))
            .unwrap();
        assert!(
            historic
                .iter()
                .any(|w| w.ident == "faa:WP_CHG" && w.latitude == 20.0)
        );

        // Provenance equality: republished rows keep the historical
        // snapshot id, and no observations are fabricated for them.
        let prov: String = store
            .conn
            .query_row(
                "SELECT source_snapshot_id FROM waypoints WHERE id = 'faa:WP_CHG' AND valid_from = ?1",
                params![t3.to_rfc3339()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(prov, "snap-2607");
        let obs: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM entity_observations WHERE entity_id = 'faa:WP_CHG' AND valid_from = ?1",
                params![t3.to_rfc3339()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(obs, 0);

        // Journal + status.
        assert!(
            store
                .query_cycle_events()
                .unwrap()
                .iter()
                .any(|e| e.kind == CycleEventKind::Rollback
                    && e.cycle_id.0 == "2608"
                    && e.restored_cycle_id.as_ref().map(|c| c.0.as_str()) == Some("2607"))
        );
        assert_eq!(
            store
                .query_cycle(&CycleId("2608".into()))
                .unwrap()
                .unwrap()
                .status,
            CycleStatus::RolledBack
        );

        // Idempotent: a second rollback is a no-op.
        let second = store.rollback_cycle(&CycleId("2608".into()), t3).unwrap();
        assert!(second.noop);
    }

    #[test]
    fn test_rollback_requires_active_cycle() {
        let mut store = WorldStore::open_in_memory().unwrap();
        store
            .insert_cycle(&cycle("2608", Some(Utc::now()), CycleStatus::Discovered))
            .unwrap();
        let err = store
            .rollback_cycle(&CycleId("2608".into()), Utc::now())
            .unwrap_err();
        assert!(err.to_string().contains("only an Active cycle"), "{err}");
    }
}
