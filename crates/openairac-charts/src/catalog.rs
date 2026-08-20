//! Isolated SQLite Catalog Database for Chart Metadata and Associations.
//!
//! Stores chart records and reference associations in `openairac_charts.sqlite`
//! completely isolated from the Little Navmap navigation database.

use crate::model::{
    AssociationConfidence, ChartAssociation, ChartDocument, ChartDocumentId, ChartMimeType,
    GeoreferenceStatus, NormalizedChartType,
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use openairac_model::RedistributionPermission;
use rusqlite::{Connection, params};
use std::path::Path;

pub struct ChartCatalog {
    conn: Connection,
}

impl ChartCatalog {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        let cat = Self { conn };
        cat.init_schema()?;
        Ok(cat)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let cat = Self { conn };
        cat.init_schema()?;
        Ok(cat)
    }

    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;

            CREATE TABLE IF NOT EXISTS chart_documents (
                id TEXT PRIMARY KEY NOT NULL,
                provider_id TEXT NOT NULL,
                airport_icao TEXT NOT NULL,
                airport_iata TEXT,
                chart_type TEXT NOT NULL,
                provider_chart_type TEXT NOT NULL,
                title TEXT NOT NULL,
                procedure_name TEXT,
                runway TEXT,
                effective_from TEXT,
                effective_to TEXT,
                revision_date TEXT,
                airac_cycle TEXT NOT NULL,
                language TEXT,
                source_url TEXT NOT NULL,
                source_document_id TEXT,
                license_policy TEXT NOT NULL,
                attribution TEXT NOT NULL,
                mime_type TEXT NOT NULL,
                asset_sha256 TEXT,
                file_size_bytes INTEGER,
                georeference_status TEXT NOT NULL,
                change_flag TEXT
            );

            CREATE TABLE IF NOT EXISTS chart_associations (
                procedure_ident TEXT NOT NULL,
                procedure_kind TEXT NOT NULL,
                airport_icao TEXT NOT NULL,
                runway TEXT,
                chart_id TEXT NOT NULL,
                confidence TEXT NOT NULL,
                match_reason TEXT NOT NULL,
                PRIMARY KEY (procedure_ident, procedure_kind, airport_icao, chart_id),
                FOREIGN KEY (chart_id) REFERENCES chart_documents(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_charts_airport ON chart_documents(airport_icao);
            CREATE INDEX IF NOT EXISTS idx_charts_cycle ON chart_documents(airac_cycle);
            CREATE INDEX IF NOT EXISTS idx_charts_type ON chart_documents(chart_type);
            CREATE INDEX IF NOT EXISTS idx_charts_proc ON chart_documents(airport_icao, procedure_name);
            CREATE INDEX IF NOT EXISTS idx_assoc_proc ON chart_associations(airport_icao, procedure_ident);
            "#,
        )?;
        Ok(())
    }

    pub fn insert_or_replace_chart(&self, doc: &ChartDocument) -> Result<()> {
        self.conn.execute(
            r#"
            INSERT OR REPLACE INTO chart_documents (
                id, provider_id, airport_icao, airport_iata, chart_type, provider_chart_type,
                title, procedure_name, runway, effective_from, effective_to, revision_date,
                airac_cycle, language, source_url, source_document_id, license_policy,
                attribution, mime_type, asset_sha256, file_size_bytes, georeference_status, change_flag
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6,
                ?7, ?8, ?9, ?10, ?11, ?12,
                ?13, ?14, ?15, ?16, ?17,
                ?18, ?19, ?20, ?21, ?22, ?23
            )
            "#,
            params![
                doc.id.0,
                doc.provider_id,
                doc.airport_icao,
                doc.airport_iata,
                doc.chart_type.as_str(),
                doc.provider_chart_type,
                doc.title,
                doc.procedure_name,
                doc.runway,
                doc.effective_from.map(|d| d.to_rfc3339()),
                doc.effective_to.map(|d| d.to_rfc3339()),
                doc.revision_date.map(|d| d.to_rfc3339()),
                doc.airac_cycle,
                doc.language,
                doc.source_url,
                doc.source_document_id,
                serde_json::to_string(&doc.license_policy)?,
                doc.attribution,
                doc.mime_type.as_str(),
                doc.asset_sha256,
                doc.file_size_bytes.map(|s| s as i64),
                format!("{:?}", doc.georeference_status),
                doc.change_flag,
            ],
        )?;
        Ok(())
    }

    pub fn insert_or_replace_association(&self, assoc: &ChartAssociation) -> Result<()> {
        self.conn.execute(
            r#"
            INSERT OR REPLACE INTO chart_associations (
                procedure_ident, procedure_kind, airport_icao, runway,
                chart_id, confidence, match_reason
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![
                assoc.procedure_ident,
                assoc.procedure_kind.to_string(),
                assoc.airport_icao,
                assoc.runway,
                assoc.chart_id.0,
                format!("{:?}", assoc.confidence),
                assoc.match_reason,
            ],
        )?;
        Ok(())
    }

    pub fn query_charts_for_airport(&self, icao: &str) -> Result<Vec<ChartDocument>> {
        let clean_icao = icao.trim().to_uppercase();
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, provider_id, airport_icao, airport_iata, chart_type, provider_chart_type,
                   title, procedure_name, runway, effective_from, effective_to, revision_date,
                   airac_cycle, language, source_url, source_document_id, license_policy,
                   attribution, mime_type, asset_sha256, file_size_bytes, georeference_status, change_flag
            FROM chart_documents
            WHERE airport_icao = ?1 OR airport_iata = ?1
            ORDER BY chart_type, title
            "#,
        )?;

        let rows = stmt.query_map([&clean_icao], |row| {
            let id: String = row.get(0)?;
            let provider_id: String = row.get(1)?;
            let airport_icao: String = row.get(2)?;
            let airport_iata: Option<String> = row.get(3)?;
            let chart_type_str: String = row.get(4)?;
            let provider_chart_type: String = row.get(5)?;
            let title: String = row.get(6)?;
            let procedure_name: Option<String> = row.get(7)?;
            let runway: Option<String> = row.get(8)?;
            let eff_from_str: Option<String> = row.get(9)?;
            let eff_to_str: Option<String> = row.get(10)?;
            let rev_date_str: Option<String> = row.get(11)?;
            let airac_cycle: String = row.get(12)?;
            let language: Option<String> = row.get(13)?;
            let source_url: String = row.get(14)?;
            let source_document_id: Option<String> = row.get(15)?;
            let policy_json: String = row.get(16)?;
            let attribution: String = row.get(17)?;
            let mime_str: String = row.get(18)?;
            let asset_sha256: Option<String> = row.get(19)?;
            let size_int: Option<i64> = row.get(20)?;
            let geo_str: String = row.get(21)?;
            let change_flag: Option<String> = row.get(22)?;

            let chart_type = match chart_type_str.as_str() {
                "airport_diagram" => NormalizedChartType::AirportDiagram,
                "parking_docking" => NormalizedChartType::ParkingDocking,
                "ground_movement" => NormalizedChartType::GroundMovement,
                "sid" => NormalizedChartType::Sid,
                "star" => NormalizedChartType::Star,
                "approach" => NormalizedChartType::Approach,
                "approach_visual" => NormalizedChartType::ApproachVisual,
                "takeoff_minima" => NormalizedChartType::TakeoffMinima,
                "alternate_minima" => NormalizedChartType::AlternateMinima,
                "radar_minima" => NormalizedChartType::RadarMinima,
                "hot_spot" => NormalizedChartType::HotSpot,
                "holding" => NormalizedChartType::Holding,
                "obstacle" => NormalizedChartType::Obstacle,
                "noise" => NormalizedChartType::Noise,
                "general_info" => NormalizedChartType::GeneralInfo,
                _ => NormalizedChartType::Other,
            };

            let mime_type = match mime_str.as_str() {
                "application/pdf" => ChartMimeType::Pdf,
                "image/png" => ChartMimeType::Png,
                "image/svg+xml" => ChartMimeType::Svg,
                "image/jpeg" => ChartMimeType::Jpeg,
                other => ChartMimeType::Other(other.to_string()),
            };

            let georeference_status = match geo_str.as_str() {
                "Georeferenced" => GeoreferenceStatus::Georeferenced,
                "Approximate" => GeoreferenceStatus::Approximate,
                "Unsupported" => GeoreferenceStatus::Unsupported,
                _ => GeoreferenceStatus::NotGeoreferenced,
            };

            let license_policy: RedistributionPermission = serde_json::from_str(&policy_json)
                .unwrap_or(RedistributionPermission::PublicRedistribution);

            Ok(ChartDocument {
                id: ChartDocumentId(id),
                provider_id,
                airport_icao,
                airport_iata,
                chart_type,
                provider_chart_type,
                title,
                procedure_name,
                runway,
                effective_from: eff_from_str.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .ok()
                        .map(|d| d.with_timezone(&Utc))
                }),
                effective_to: eff_to_str.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .ok()
                        .map(|d| d.with_timezone(&Utc))
                }),
                revision_date: rev_date_str.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .ok()
                        .map(|d| d.with_timezone(&Utc))
                }),
                airac_cycle,
                language,
                source_url,
                source_document_id,
                license_policy,
                attribution,
                mime_type,
                asset_sha256,
                file_size_bytes: size_int.map(|s| s as u64),
                georeference_status,
                change_flag,
            })
        })?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn query_chart_by_id(&self, id: &ChartDocumentId) -> Result<Option<ChartDocument>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, provider_id, airport_icao, airport_iata, chart_type, provider_chart_type,
                   title, procedure_name, runway, effective_from, effective_to, revision_date,
                   airac_cycle, language, source_url, source_document_id, license_policy,
                   attribution, mime_type, asset_sha256, file_size_bytes, georeference_status, change_flag
            FROM chart_documents
            WHERE id = ?1
            LIMIT 1
            "#,
        )?;

        let mut rows = stmt.query([&id.0])?;
        if let Some(row) = rows.next()? {
            let id_str: String = row.get(0)?;
            let provider_id: String = row.get(1)?;
            let airport_icao: String = row.get(2)?;
            let airport_iata: Option<String> = row.get(3)?;
            let chart_type_str: String = row.get(4)?;
            let provider_chart_type: String = row.get(5)?;
            let title: String = row.get(6)?;
            let procedure_name: Option<String> = row.get(7)?;
            let runway: Option<String> = row.get(8)?;
            let eff_from_str: Option<String> = row.get(9)?;
            let eff_to_str: Option<String> = row.get(10)?;
            let rev_date_str: Option<String> = row.get(11)?;
            let airac_cycle: String = row.get(12)?;
            let language: Option<String> = row.get(13)?;
            let source_url: String = row.get(14)?;
            let source_document_id: Option<String> = row.get(15)?;
            let policy_json: String = row.get(16)?;
            let attribution: String = row.get(17)?;
            let mime_str: String = row.get(18)?;
            let asset_sha256: Option<String> = row.get(19)?;
            let size_int: Option<i64> = row.get(20)?;
            let geo_str: String = row.get(21)?;
            let change_flag: Option<String> = row.get(22)?;

            let chart_type = match chart_type_str.as_str() {
                "airport_diagram" => NormalizedChartType::AirportDiagram,
                "sid" => NormalizedChartType::Sid,
                "star" => NormalizedChartType::Star,
                "approach" => NormalizedChartType::Approach,
                "takeoff_minima" => NormalizedChartType::TakeoffMinima,
                "alternate_minima" => NormalizedChartType::AlternateMinima,
                _ => NormalizedChartType::Other,
            };

            let mime_type = match mime_str.as_str() {
                "application/pdf" => ChartMimeType::Pdf,
                _ => ChartMimeType::Other(mime_str),
            };

            let georeference_status = match geo_str.as_str() {
                "Georeferenced" => GeoreferenceStatus::Georeferenced,
                _ => GeoreferenceStatus::NotGeoreferenced,
            };

            let license_policy: RedistributionPermission = serde_json::from_str(&policy_json)
                .unwrap_or(RedistributionPermission::PublicRedistribution);

            Ok(Some(ChartDocument {
                id: ChartDocumentId(id_str),
                provider_id,
                airport_icao,
                airport_iata,
                chart_type,
                provider_chart_type,
                title,
                procedure_name,
                runway,
                effective_from: eff_from_str.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .ok()
                        .map(|d| d.with_timezone(&Utc))
                }),
                effective_to: eff_to_str.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .ok()
                        .map(|d| d.with_timezone(&Utc))
                }),
                revision_date: rev_date_str.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .ok()
                        .map(|d| d.with_timezone(&Utc))
                }),
                airac_cycle,
                language,
                source_url,
                source_document_id,
                license_policy,
                attribution,
                mime_type,
                asset_sha256,
                file_size_bytes: size_int.map(|s| s as u64),
                georeference_status,
                change_flag,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn query_charts_for_procedure(
        &self,
        icao: &str,
        procedure_ident: &str,
    ) -> Result<Vec<ChartAssociation>> {
        let clean_icao = icao.trim().to_uppercase();
        let clean_proc = procedure_ident.trim().to_uppercase();

        let mut stmt = self.conn.prepare(
            r#"
            SELECT procedure_ident, procedure_kind, airport_icao, runway,
                   chart_id, confidence, match_reason
            FROM chart_associations
            WHERE airport_icao = ?1 AND procedure_ident = ?2
            "#,
        )?;

        let rows = stmt.query_map([&clean_icao, &clean_proc], |row| {
            let proc_id: String = row.get(0)?;
            let kind_str: String = row.get(1)?;
            let apt_icao: String = row.get(2)?;
            let rwy: Option<String> = row.get(3)?;
            let ch_id: String = row.get(4)?;
            let conf_str: String = row.get(5)?;
            let reason: String = row.get(6)?;

            let confidence = match conf_str.as_str() {
                "Exact" => AssociationConfidence::Exact,
                "Likely" => AssociationConfidence::Likely,
                "Ambiguous" => AssociationConfidence::Ambiguous,
                _ => AssociationConfidence::Unmatched,
            };

            Ok(ChartAssociation {
                procedure_ident: proc_id,
                procedure_kind: kind_str.chars().next().unwrap_or('F'),
                airport_icao: apt_icao,
                runway: rwy,
                chart_id: ChartDocumentId(ch_id),
                confidence,
                match_reason: reason,
            })
        })?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }
    pub fn total_charts(&self) -> Result<usize> {
        let cnt: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM chart_documents", [], |r| r.get(0))?;
        Ok(cnt as usize)
    }

    pub fn airport_count(&self) -> Result<usize> {
        let cnt: i64 = self.conn.query_row(
            "SELECT COUNT(DISTINCT airport_icao) FROM chart_documents",
            [],
            |r| r.get(0),
        )?;
        Ok(cnt as usize)
    }
}
