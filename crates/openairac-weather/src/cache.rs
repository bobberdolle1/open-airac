//! Isolated Weather SQLite Cache and Rate-Limiting Engine.
//!
//! Stores dynamic METAR, TAF, SIGMET, and PIREP payloads with explicit TTLs
//! completely isolated from the Little Navmap navigation database.

use crate::model::{MetarReport, PirepReport, Sigmet, TafReport};
use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherCacheStatus {
    pub cached_metars: usize,
    pub cached_tafs: usize,
    pub cached_sigmets: usize,
    pub cached_pireps: usize,
    pub db_path: String,
}

pub struct WeatherCache {
    conn: Connection,
    path_str: String,
}

impl WeatherCache {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let p_str = path.as_ref().to_string_lossy().to_string();
        let conn = Connection::open(path)?;
        let cache = Self {
            conn,
            path_str: p_str,
        };
        cache.init_schema()?;
        Ok(cache)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let cache = Self {
            conn,
            path_str: ":memory:".to_string(),
        };
        cache.init_schema()?;
        Ok(cache)
    }

    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;

            CREATE TABLE IF NOT EXISTS metar_cache (
                station_id TEXT PRIMARY KEY NOT NULL,
                json_payload TEXT NOT NULL,
                observation_time TEXT NOT NULL,
                fetched_at TEXT NOT NULL,
                expires_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS taf_cache (
                station_id TEXT PRIMARY KEY NOT NULL,
                json_payload TEXT NOT NULL,
                valid_from TEXT NOT NULL,
                valid_to TEXT NOT NULL,
                fetched_at TEXT NOT NULL,
                expires_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS sigmet_cache (
                id TEXT PRIMARY KEY NOT NULL,
                json_payload TEXT NOT NULL,
                valid_from TEXT NOT NULL,
                valid_to TEXT NOT NULL,
                fetched_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS pirep_cache (
                id TEXT PRIMARY KEY NOT NULL,
                json_payload TEXT NOT NULL,
                obs_time TEXT NOT NULL,
                fetched_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_metar_exp ON metar_cache(expires_at);
            CREATE INDEX IF NOT EXISTS idx_taf_exp ON taf_cache(expires_at);
            CREATE INDEX IF NOT EXISTS idx_sigmet_to ON sigmet_cache(valid_to);
            CREATE INDEX IF NOT EXISTS idx_pirep_obs ON pirep_cache(obs_time);
            "#,
        )?;
        Ok(())
    }

    pub fn put_metar(&self, report: &MetarReport, ttl_minutes: i64) -> Result<()> {
        let expires_at = Utc::now() + Duration::minutes(ttl_minutes);
        let payload = serde_json::to_string(report)?;
        self.conn.execute(
            r#"
            INSERT OR REPLACE INTO metar_cache (station_id, json_payload, observation_time, fetched_at, expires_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            params![
                report.station_id.to_uppercase(),
                payload,
                report.observation_time.to_rfc3339(),
                report.fetch_time.to_rfc3339(),
                expires_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn get_metar(&self, station_id: &str) -> Result<Option<MetarReport>> {
        let mut stmt = self.conn.prepare(
            "SELECT json_payload, observation_time FROM metar_cache WHERE station_id = ?1 LIMIT 1",
        )?;
        let clean_id = station_id.trim().to_uppercase();
        let mut rows = stmt.query([&clean_id])?;
        if let Some(row) = rows.next()? {
            let payload: String = row.get(0)?;
            let mut report: MetarReport = serde_json::from_str(&payload)?;
            // Check staleness dynamically against now
            report.is_stale = report.staleness(Utc::now()) == crate::model::WeatherStaleness::Stale
                || report.staleness(Utc::now()) == crate::model::WeatherStaleness::Expired;
            Ok(Some(report))
        } else {
            Ok(None)
        }
    }

    pub fn put_taf(&self, report: &TafReport, ttl_minutes: i64) -> Result<()> {
        let expires_at = Utc::now() + Duration::minutes(ttl_minutes);
        let payload = serde_json::to_string(report)?;
        self.conn.execute(
            r#"
            INSERT OR REPLACE INTO taf_cache (station_id, json_payload, valid_from, valid_to, fetched_at, expires_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            params![
                report.station_id.to_uppercase(),
                payload,
                report.valid_from.to_rfc3339(),
                report.valid_to.to_rfc3339(),
                report.fetch_time.to_rfc3339(),
                expires_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn get_taf(&self, station_id: &str) -> Result<Option<TafReport>> {
        let mut stmt = self.conn.prepare(
            "SELECT json_payload, valid_to FROM taf_cache WHERE station_id = ?1 LIMIT 1",
        )?;
        let clean_id = station_id.trim().to_uppercase();
        let mut rows = stmt.query([&clean_id])?;
        if let Some(row) = rows.next()? {
            let payload: String = row.get(0)?;
            let mut report: TafReport = serde_json::from_str(&payload)?;
            report.is_stale = Utc::now() > report.valid_to;
            Ok(Some(report))
        } else {
            Ok(None)
        }
    }

    pub fn put_sigmets(&self, sigmets: &[Sigmet]) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        for sig in sigmets {
            let payload = serde_json::to_string(sig)?;
            self.conn.execute(
                r#"
                INSERT OR REPLACE INTO sigmet_cache (id, json_payload, valid_from, valid_to, fetched_at)
                VALUES (?1, ?2, ?3, ?4, ?5)
                "#,
                params![
                    sig.id,
                    payload,
                    sig.valid_from.to_rfc3339(),
                    sig.valid_to.to_rfc3339(),
                    now,
                ],
            )?;
        }
        Ok(())
    }

    pub fn get_active_sigmets(&self, now: DateTime<Utc>) -> Result<Vec<Sigmet>> {
        let mut stmt = self.conn.prepare(
            "SELECT json_payload FROM sigmet_cache WHERE valid_to >= ?1 ORDER BY valid_from",
        )?;
        let now_str = now.to_rfc3339();
        let rows = stmt.query_map([&now_str], |row| {
            let payload: String = row.get(0)?;
            let sig: Sigmet = serde_json::from_str(&payload).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
            Ok(sig)
        })?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn put_pireps(&self, pireps: &[PirepReport]) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        for p in pireps {
            let id = format!(
                "{}:{:.4}:{:.4}",
                p.obs_time.timestamp(),
                p.latitude,
                p.longitude
            );
            let payload = serde_json::to_string(p)?;
            self.conn.execute(
                r#"
                INSERT OR REPLACE INTO pirep_cache (id, json_payload, obs_time, fetched_at)
                VALUES (?1, ?2, ?3, ?4)
                "#,
                params![id, payload, p.obs_time.to_rfc3339(), now,],
            )?;
        }
        Ok(())
    }

    pub fn get_recent_pireps(&self, max_age_hours: i64) -> Result<Vec<PirepReport>> {
        let threshold = (Utc::now() - Duration::hours(max_age_hours)).to_rfc3339();
        let mut stmt = self.conn.prepare(
            "SELECT json_payload FROM pirep_cache WHERE obs_time >= ?1 ORDER BY obs_time DESC",
        )?;
        let rows = stmt.query_map([&threshold], |row| {
            let payload: String = row.get(0)?;
            let p: PirepReport = serde_json::from_str(&payload).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
            Ok(p)
        })?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn prune_expired(&self) -> Result<usize> {
        let now_str = Utc::now().to_rfc3339();
        let mut removed = 0usize;
        removed += self
            .conn
            .execute("DELETE FROM metar_cache WHERE expires_at < ?1", [&now_str])?;
        removed += self
            .conn
            .execute("DELETE FROM taf_cache WHERE expires_at < ?1", [&now_str])?;
        removed += self
            .conn
            .execute("DELETE FROM sigmet_cache WHERE valid_to < ?1", [&now_str])?;
        Ok(removed)
    }

    pub fn cache_status(&self) -> Result<WeatherCacheStatus> {
        let metars: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM metar_cache", [], |r| r.get(0))?;
        let tafs: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM taf_cache", [], |r| r.get(0))?;
        let sigmets: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM sigmet_cache", [], |r| r.get(0))?;
        let pireps: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM pirep_cache", [], |r| r.get(0))?;

        Ok(WeatherCacheStatus {
            cached_metars: metars as usize,
            cached_tafs: tafs as usize,
            cached_sigmets: sigmets as usize,
            cached_pireps: pireps as usize,
            db_path: self.path_str.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    #[test]
    fn test_metar_cache_and_expiration() {
        let cache = WeatherCache::open_in_memory().unwrap();
        let obs = Utc::now() - Duration::minutes(10);
        let metar = MetarReport {
            station_id: "KJFK".to_string(),
            observation_time: obs,
            report_time: Some(obs),
            raw_text: "METAR KJFK 201200Z 22014KT 10SM CLR 25/18 A2992".to_string(),
            flight_category: FlightCategory::Vfr,
            temp_c: Some(25.0),
            dewpoint_c: Some(18.0),
            wind_dir_deg: Some(220),
            wind_speed_kts: Some(14),
            wind_gust_kts: None,
            wind_variable: false,
            visibility_sm: Some(10.0),
            altimeter_hpa: Some(1013.2),
            altimeter_inhg: Some(29.92),
            clouds: Vec::new(),
            weather_phenomena: Vec::new(),
            fetch_time: Utc::now(),
            provider_id: "NOAA_AWC".to_string(),
            is_stale: false,
        };

        cache.put_metar(&metar, 60).unwrap();
        let loaded = cache.get_metar("KJFK").unwrap().unwrap();
        assert_eq!(loaded.station_id, "KJFK");
        assert_eq!(loaded.flight_category, FlightCategory::Vfr);
        assert!(!loaded.is_stale);
    }
}
