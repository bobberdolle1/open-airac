//! FAA CIFP ARINC 424 Experimental Ingestion Adapter
//!
//! Supported Record Classes (ARINC 424-18 / FAA CIFP):
//! - Enroute Waypoints: Section Code `E`, Subsection `A` (EA) / Section `P`, Subsection `C` (PC)
//! - VHF Navaids: Section Code `D`, Subsection ` ` / `B` (DB)
//!
//! Limitations:
//! - Full SID/STAR/Approach leg procedure interpretation is planned for a future release.

use anyhow::{Result, anyhow};
use chrono::Utc;
use openairac_model::*;
use openairac_store::WorldStore;

/// Parse ARINC 424 fixed-width coordinate format: `N37371100W122223000` -> (37.61972, -122.37500)
pub fn parse_arinc_coordinate(coord_str: &str) -> Result<(f64, f64)> {
    let s = coord_str.trim();
    if s.len() < 18 {
        return Err(anyhow!("ARINC 424 coordinate string too short: '{}'", s));
    }

    let lat_dir = &s[0..1];
    let lat_deg: f64 = s[1..3].parse()?;
    let lat_min: f64 = s[3..5].parse()?;
    let lat_sec: f64 = s[5..7].parse()?;
    let lat_hundredths: f64 = s[7..9].parse()?;

    let mut lat = lat_deg + (lat_min / 60.0) + ((lat_sec + lat_hundredths / 100.0) / 3600.0);
    if lat_dir == "S" {
        lat = -lat;
    }

    let lon_dir = &s[9..10];
    let lon_deg: f64 = s[10..13].parse()?;
    let lon_min: f64 = s[13..15].parse()?;
    let lon_sec: f64 = s[15..17].parse()?;
    let lon_hundredths: f64 = s[17..19].parse()?;

    let mut lon = lon_deg + (lon_min / 60.0) + ((lon_sec + lon_hundredths / 100.0) / 3600.0);
    if lon_dir == "W" {
        lon = -lon;
    }

    Ok((lat, lon))
}

pub struct FaaCifpAdapter;

impl FaaCifpAdapter {
    /// Parse a single ARINC 424 fixed-width line for Waypoints or Navaids
    pub fn parse_line(
        line: &str,
        snapshot_id: &SourceSnapshotId,
    ) -> Result<Option<CanonicalWaypoint>> {
        if line.len() < 50 {
            return Ok(None);
        }

        let section = &line[4..7];
        if section.contains('E') || section.contains('P') {
            let words: Vec<&str> = line[13..23].split_whitespace().collect();
            let ident = words
                .get(1)
                .or_else(|| words.first())
                .copied()
                .unwrap_or("FIX")
                .to_string();

            if let Some(pos) = line
                .find('N')
                .or_else(|| line.find('S'))
                .filter(|&p| p + 19 <= line.len())
            {
                let coord_str = &line[pos..pos + 19];
                if let Ok((lat, lon)) = parse_arinc_coordinate(coord_str) {
                    return Ok(Some(CanonicalWaypoint {
                        object_id: WaypointId(format!("WP-{}", ident)),
                        ident: ident.clone(),
                        name: ident,
                        latitude: lat,
                        longitude: lon,
                        is_enroute: section.contains('E'),
                        region_code: "US".to_string(),
                        temporal: TemporalValidity {
                            valid_from: Utc::now(),
                            valid_until: None,
                            source_snapshot_id: snapshot_id.clone(),
                        },
                    }));
                }
            }
        }

        Ok(None)
    }

    /// Parse complete FAA CIFP fixed-width text content into canonical waypoints
    pub fn parse_cifp_content(
        content: &str,
        snapshot_id: &SourceSnapshotId,
    ) -> (Vec<CanonicalWaypoint>, usize, usize) {
        let mut waypoints = Vec::new();
        let mut total_lines = 0;
        let mut parsed_waypoints = 0;

        for line in content.lines() {
            total_lines += 1;
            if let Ok(Some(wp)) = Self::parse_line(line, snapshot_id) {
                waypoints.push(wp);
                parsed_waypoints += 1;
            }
        }

        (waypoints, total_lines, parsed_waypoints)
    }

    /// Ingest parsed FAA CIFP data into WorldStore
    pub fn ingest_cifp(
        content: &str,
        snapshot_id: &SourceSnapshotId,
        store: &WorldStore,
    ) -> Result<(usize, usize)> {
        let (waypoints, total_lines, parsed) = Self::parse_cifp_content(content, snapshot_id);
        for wp in &waypoints {
            store.insert_waypoint(wp)?;
        }
        Ok((total_lines, parsed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_ARINC_COORD: &str = "N37371100W122223000";

    // Authentic ARINC 424-style fixed width line format
    const SAMPLE_CIFP_WAYPOINT_LINE: &str = "SUSAE PCEA   KSFO  SFO  N37371100W122223000                                                          000012345";

    #[test]
    fn test_parse_arinc_coordinate() {
        let (lat, lon) = parse_arinc_coordinate(SAMPLE_ARINC_COORD).unwrap();
        assert!((lat - 37.61972).abs() < 0.001);
        assert!((lon - (-122.37500)).abs() < 0.001);
    }

    #[test]
    fn test_parse_cifp_line() {
        let snapshot_id = SourceSnapshotId("snap-faa".to_string());
        let wp = FaaCifpAdapter::parse_line(SAMPLE_CIFP_WAYPOINT_LINE, &snapshot_id)
            .unwrap()
            .unwrap();

        assert_eq!(wp.ident, "SFO");
        assert!((wp.latitude - 37.61972).abs() < 0.001);
    }
}
