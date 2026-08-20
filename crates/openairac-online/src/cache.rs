//! Ephemeral SQLite Cache for Real-Time Online Network Data.
//!
//! Stores the latest network snapshot, operational caches, and active events
//! with strict retention bounds and zero persistent user identity tracking.

use crate::model::{NetworkSnapshot, OnlineEvent};
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};

/// Ephemeral online cache backed by SQLite.
pub struct OnlineCache {
    path: PathBuf,
    conn: Connection,
}

impl OnlineCache {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let p = path.as_ref().to_path_buf();
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&p)
            .with_context(|| format!("opening online cache at {}", p.display()))?;

        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;

            CREATE TABLE IF NOT EXISTS network_snapshot (
                provider_name TEXT PRIMARY KEY,
                generated_at TEXT NOT NULL,
                received_at TEXT NOT NULL,
                connected_clients INTEGER NOT NULL,
                pilots_count INTEGER NOT NULL,
                controllers_count INTEGER NOT NULL,
                snapshot_json TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS online_events (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                event_type TEXT,
                start_time TEXT NOT NULL,
                end_time TEXT NOT NULL,
                airports_json TEXT NOT NULL,
                routes_json TEXT NOT NULL,
                organizers_json TEXT NOT NULL,
                link TEXT,
                description TEXT,
                cached_at TEXT NOT NULL
            );
            "#,
        )?;

        Ok(Self { path: p, conn })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS network_snapshot (
                provider_name TEXT PRIMARY KEY,
                generated_at TEXT NOT NULL,
                received_at TEXT NOT NULL,
                connected_clients INTEGER NOT NULL,
                pilots_count INTEGER NOT NULL,
                controllers_count INTEGER NOT NULL,
                snapshot_json TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS online_events (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                event_type TEXT,
                start_time TEXT NOT NULL,
                end_time TEXT NOT NULL,
                airports_json TEXT NOT NULL,
                routes_json TEXT NOT NULL,
                organizers_json TEXT NOT NULL,
                link TEXT,
                description TEXT,
                cached_at TEXT NOT NULL
            );
            "#,
        )?;

        Ok(Self {
            path: PathBuf::from(":memory:"),
            conn,
        })
    }

    pub fn put_snapshot(&self, snapshot: &NetworkSnapshot) -> Result<()> {
        let json_str = serde_json::to_string(snapshot)?;
        self.conn.execute(
            r#"
            INSERT INTO network_snapshot (
                provider_name, generated_at, received_at, connected_clients,
                pilots_count, controllers_count, snapshot_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(provider_name) DO UPDATE SET
                generated_at = excluded.generated_at,
                received_at = excluded.received_at,
                connected_clients = excluded.connected_clients,
                pilots_count = excluded.pilots_count,
                controllers_count = excluded.controllers_count,
                snapshot_json = excluded.snapshot_json
            "#,
            params![
                snapshot.provider_name,
                snapshot.generated_at.to_rfc3339(),
                snapshot.received_at.to_rfc3339(),
                snapshot.connected_clients,
                snapshot.pilots.len(),
                snapshot.controllers.len(),
                json_str,
            ],
        )?;

        Ok(())
    }

    pub fn get_snapshot(&self, provider_name: &str) -> Result<Option<NetworkSnapshot>> {
        let mut stmt = self
            .conn
            .prepare("SELECT snapshot_json FROM network_snapshot WHERE provider_name = ?1")?;

        let mut rows = stmt.query(params![provider_name])?;
        if let Some(row) = rows.next()? {
            let json_str: String = row.get(0)?;
            let snapshot: NetworkSnapshot = serde_json::from_str(&json_str)?;
            Ok(Some(snapshot))
        } else {
            Ok(None)
        }
    }

    pub fn put_events(&mut self, events: &[OnlineEvent]) -> Result<()> {
        let tx = self.conn.transaction()?;

        let now = Utc::now().to_rfc3339();
        for ev in events {
            let apts = serde_json::to_string(&ev.airports)?;
            let rts = serde_json::to_string(&ev.routes)?;
            let orgs = serde_json::to_string(&ev.organizers)?;

            tx.execute(
                r#"
                INSERT INTO online_events (
                    id, name, event_type, start_time, end_time,
                    airports_json, routes_json, organizers_json, link, description, cached_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name,
                    event_type = excluded.event_type,
                    start_time = excluded.start_time,
                    end_time = excluded.end_time,
                    airports_json = excluded.airports_json,
                    routes_json = excluded.routes_json,
                    organizers_json = excluded.organizers_json,
                    link = excluded.link,
                    description = excluded.description,
                    cached_at = excluded.cached_at
                "#,
                params![
                    ev.id,
                    ev.name,
                    ev.event_type,
                    ev.start_time.to_rfc3339(),
                    ev.end_time.to_rfc3339(),
                    apts,
                    rts,
                    orgs,
                    ev.link,
                    ev.description,
                    now,
                ],
            )?;
        }

        // Prune expired events (older than 2 days ago)
        let expire_threshold = (Utc::now() - Duration::days(2)).to_rfc3339();
        tx.execute(
            "DELETE FROM online_events WHERE end_time < ?1",
            params![expire_threshold],
        )?;

        tx.commit()?;
        Ok(())
    }

    pub fn get_active_and_upcoming_events(&self, now: DateTime<Utc>) -> Result<Vec<OnlineEvent>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, name, event_type, start_time, end_time,
                   airports_json, routes_json, organizers_json, link, description
            FROM online_events
            WHERE end_time >= ?1
            ORDER BY start_time ASC
            "#,
        )?;

        let now_str = now.to_rfc3339();
        let rows = stmt.query_map(params![now_str], |row| {
            let id: u64 = row.get(0)?;
            let name: String = row.get(1)?;
            let event_type: Option<String> = row.get(2)?;
            let start_time_str: String = row.get(3)?;
            let end_time_str: String = row.get(4)?;
            let apts_json: String = row.get(5)?;
            let rts_json: String = row.get(6)?;
            let orgs_json: String = row.get(7)?;
            let link: Option<String> = row.get(8)?;
            let description: Option<String> = row.get(9)?;

            let start_time = DateTime::parse_from_rfc3339(&start_time_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or(now);
            let end_time = DateTime::parse_from_rfc3339(&end_time_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or(now);

            let airports: Vec<String> = serde_json::from_str(&apts_json).unwrap_or_default();
            let routes: Vec<String> = serde_json::from_str(&rts_json).unwrap_or_default();
            let organizers: Vec<String> = serde_json::from_str(&orgs_json).unwrap_or_default();

            Ok(OnlineEvent {
                id,
                name,
                event_type,
                start_time,
                end_time,
                airports,
                routes,
                organizers,
                link,
                description,
            })
        })?;

        let mut events = Vec::new();
        for r in rows {
            events.push(r?);
        }

        Ok(events)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
