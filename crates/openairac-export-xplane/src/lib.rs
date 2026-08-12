use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use openairac_model::*;
use openairac_store::WorldStore;
use std::fs::File;
use std::io::Write;
use std::path::Path;

pub struct XPlane12Exporter;

impl XPlane12Exporter {
    /// Export active canonical records from SQLite database into X-Plane 12 format files
    pub fn export_from_db<P: AsRef<Path>>(
        store: &WorldStore,
        date: DateTime<Utc>,
        out_dir: P,
    ) -> Result<(usize, usize)> {
        let dir = out_dir.as_ref();
        std::fs::create_dir_all(dir)?;

        let waypoints = store.query_waypoints_at(date)?;
        let navaids = store.query_navaids_at(date)?;

        let fix_path = dir.join("earth_fix.dat");
        let fix_file = File::create(&fix_path)
            .with_context(|| format!("Failed to create X-Plane 12 fix file at {:?}", fix_path))?;
        Self::export_earth_fix(&waypoints, fix_file)?;

        let nav_path = dir.join("earth_nav.dat");
        let nav_file = File::create(&nav_path)
            .with_context(|| format!("Failed to create X-Plane 12 nav file at {:?}", nav_path))?;
        Self::export_earth_nav(&navaids, nav_file)?;

        Ok((waypoints.len(), navaids.len()))
    }

    /// Export waypoints into X-Plane 12 `earth_fix.dat` format (1200 Version)
    pub fn export_earth_fix<W: Write>(
        waypoints: &[CanonicalWaypoint],
        mut writer: W,
    ) -> Result<()> {
        writeln!(writer, "I")?;
        writeln!(
            writer,
            "1200 Version - OpenAIRAC Canonical World, NOAA WMM2025"
        )?;
        writeln!(writer)?;

        for wp in waypoints {
            let region = if wp.region_code.is_empty() {
                "ENROUTE"
            } else {
                &wp.region_code
            };
            writeln!(
                writer,
                "{:11.8} {:12.8} {:5} {}",
                wp.latitude, wp.longitude, wp.ident, region
            )?;
        }

        writeln!(writer, "99")?;
        Ok(())
    }

    /// Export navaids into X-Plane 12 `earth_nav.dat` format (1200 Version)
    pub fn export_earth_nav<W: Write>(navaids: &[CanonicalNavaid], mut writer: W) -> Result<()> {
        writeln!(writer, "I")?;
        writeln!(
            writer,
            "1200 Version - OpenAIRAC Canonical World, NOAA WMM2025 Engine"
        )?;
        writeln!(writer)?;

        for nav in navaids {
            let type_code = match nav.kind {
                NavaidKind::Ndb => 2,
                NavaidKind::Vor => 3,
                NavaidKind::Vordme | NavaidKind::Vortac => 3,
                NavaidKind::IlsLocalizer => 4,
                NavaidKind::IlsGlidepath => 6,
            };

            let range_nm = match nav.kind {
                NavaidKind::Ndb => 50,
                _ => 130,
            };

            let magvar = nav.magnetic_variation_deg.unwrap_or(0.0);

            writeln!(
                writer,
                "{} {:11.8} {:12.8} {:5} {:5} {:3} {:6.1} {:5} {}",
                type_code,
                nav.latitude,
                nav.longitude,
                nav.elevation_ft,
                nav.frequency.0 / 10, // X-Plane nav.dat frequency in tens of kHz (e.g. 115.80 MHz -> 11580)
                range_nm,
                magvar,
                nav.ident,
                nav.name
            )?;
        }

        writeln!(writer, "99")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_export_earth_fix_golden_format() {
        let waypoints = vec![CanonicalWaypoint {
            object_id: WaypointId("WP-SFO".to_string()),
            ident: "SFO".to_string(),
            name: "SFO".to_string(),
            latitude: 37.6195,
            longitude: -122.3739,
            is_enroute: true,
            region_code: "K2".to_string(),
            temporal: TemporalValidity {
                valid_from: Utc::now(),
                valid_until: None,
                source_snapshot_id: SourceSnapshotId("snap-test".to_string()),
            },
        }];

        let mut buf = Vec::new();
        XPlane12Exporter::export_earth_fix(&waypoints, &mut buf).unwrap();
        let content = String::from_utf8(buf).unwrap();

        assert!(content.starts_with("I\n1200 Version"));
        assert!(content.contains("37.61950000 -122.37390000 SFO   K2"));
        assert!(content.ends_with("99\n"));
    }

    #[test]
    fn test_export_from_db() {
        let store = WorldStore::open_in_memory().unwrap();
        let snapshot = SourceSnapshot {
            id: SourceSnapshotId("snap-test".to_string()),
            provider: "Test".to_string(),
            dataset: "navaids".to_string(),
            provider_revision: None,
            airac_cycle: None,
            effective_from: Some(Utc::now()),
            effective_until: None,
            retrieved_at: Utc::now(),
            source_uri: "http://test".to_string(),
            content_sha256: "hash".to_string(),
            license_notes: None,
            parser_version: "0.2.0".to_string(),
        };
        store.insert_source_snapshot(&snapshot).unwrap();

        let navaid = CanonicalNavaid {
            object_id: NavaidId("SFO".to_string()),
            ident: "SFO".to_string(),
            name: "San Francisco VOR-DME".to_string(),
            kind: NavaidKind::Vordme,
            frequency: FrequencyKhz::from_mhz(115.80),
            latitude: 37.6195,
            longitude: -122.3739,
            elevation_ft: 13,
            associated_airport: Some("KSFO".to_string()),
            magnetic_variation_deg: Some(-13.0),
            temporal: TemporalValidity {
                valid_from: Utc::now(),
                valid_until: None,
                source_snapshot_id: SourceSnapshotId("snap-test".to_string()),
            },
        };
        store.insert_navaid(&navaid).unwrap();

        let temp_dir = std::env::temp_dir().join("openairac_xp12_test");
        let (wp_cnt, nav_cnt) =
            XPlane12Exporter::export_from_db(&store, Utc::now(), &temp_dir).unwrap();

        assert_eq!(wp_cnt, 0);
        assert_eq!(nav_cnt, 1);
        assert!(temp_dir.join("earth_nav.dat").exists());
        assert!(temp_dir.join("earth_fix.dat").exists());
    }
}
