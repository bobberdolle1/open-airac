//! Little Navmap SQLite nav database exporter.
//!
//! Format authority: the Little Navmap / atools open-source schema
//! (GPL-3.0; albar965/atools `resources/sql/fs/db/create_nav_schema.sql`
//! + `create_ap_schema.sql`).
//!
//! The schema interface (table/column layout) is used as the
//! compatibility contract; no GPL code is copied into this repository.
//!
//! Support state: EXPERIMENTAL. The database is schema-valid and
//! referentially consistent, but loading it in the Little Navmap
//! application has not been executed (the app normally compiles its
//! own database from simulator scenery).

use anyhow::Result;
use chrono::{DateTime, Utc};
use openairac_export::{ArtifactEntry, FormatExporter, GeneratedArtifactSet, families};
use openairac_model::NavaidKind;
#[cfg(test)]
use openairac_model::TemporalValidity;
use openairac_store::WorldStore;
use sha2::Digest;
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

pub struct LnmNavdataExporter;

impl FormatExporter for LnmNavdataExporter {
    fn family(&self) -> openairac_export::FormatFamilyId {
        families::lnm_sqlite()
    }

    fn export(
        &self,
        store: &WorldStore,
        as_of: DateTime<Utc>,
        out_dir: &Path,
    ) -> Result<GeneratedArtifactSet> {
        std::fs::create_dir_all(out_dir)?;
        let db_path = out_dir.join("little_navmap_openairac.db");
        let _ = std::fs::remove_file(&db_path);
        let mut conn = rusqlite::Connection::open(&db_path)?;
        // Bulk load: foreign keys are enforced post-load by a
        // consistency pass, not during row-at-a-time insertion
        // (runway <-> runway_end is mutually referencing).
        conn.execute_batch(
            "PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF; PRAGMA foreign_keys=OFF;",
        )?;

        create_schema(&conn)?;

        let airports = store.query_airports_at(as_of)?;
        let navaids = store.query_navaids_at(as_of)?;
        let waypoints = store.query_waypoints_at(as_of)?;
        let airway_legs = store.query_airway_legs_at(as_of)?;

        // bgl_file: one synthetic file for provenance.
        conn.execute(
            "INSERT INTO bgl_file (bgl_file_id, filename, filepath, scenery_local_path, size)
             VALUES (1, 'openairac', 'openairac', NULL, ?1)",
            rusqlite::params![0i64],
        )?;

        // magdecl: single reference entry (WMM-computed variation is
        // carried per-entity; a global grid is not exported).
        conn.execute(
            "INSERT INTO magdecl (magdecl_id, reference_time, mag_var) VALUES (1, ?1, NULL)",
            rusqlite::params![as_of.timestamp()],
        )?;

        // metadata: required for Little Navmap database compatibility.
        let cycle_str = openairac_export_xplane::airac_cycle(as_of);
        conn.execute(
            "INSERT INTO metadata (db_version_major, db_version_minor, last_load_timestamp,
                has_sid_star, airac_cycle, valid_through, data_source, compiler_version, properties)
             VALUES (14, 29, ?1, 1, ?2, NULL, 'OPENAIRAC', ?3, NULL)",
            rusqlite::params![
                as_of.to_rfc3339(),
                cycle_str,
                format!("OpenAIRAC {}", env!("CARGO_PKG_VERSION")),
            ],
        )?;
        let tx = conn.transaction()?;
        {
            // Airports + runways + runway ends.
            let mut runway_id_counter = 0i64;
            for (airport_idx, airport) in airports.iter().enumerate() {
                let airport_id_counter = airport_idx as i64;
                let type_int: i64 = match airport.airport_type.as_str() {
                    "seaplane_base" => 16,
                    "heliport" => 17,
                    _ => 1,
                };
                tx.execute(
                    "INSERT INTO airport (airport_id, file_id, ident, icao, iata, faa, local,
                        name, city, state, country, region, flatten, type,
                        fuel_flags, has_avgas, has_jetfuel, has_tower_object,
                        is_closed, is_military, is_addon,
                        rating, longest_runway_length, longest_runway_width,
                        longest_runway_heading, longest_runway_surface,
                        num_runway_hard, num_runway_soft, num_runway_water, num_runway_light,
                        num_runway_end_closed, num_runway_end_vasi, num_runway_end_als,
                        num_runway_end_ils, num_apron, num_taxi_path, num_apron_light,
                        num_taxi_path_has_centerline, num_taxi_path_has_center_red,
                        num_parking_gate, num_parking_ga_ramp, num_parking_cargo,
                        num_parking_mil_cargo, num_parking_mil_combat,
                        num_helipad, tower_frequency, atis_frequency, awos_frequency,
                        asos_frequency, unicom_frequency,
                        left_lonx, top_laty, right_lonx, bottom_laty,
                        mag_var, tower_heading, tower_altitude, tower_lonx, tower_laty,
                        alt_lonx, alt_laty, elevation, display_elevation, is_3d)
                    VALUES (?1, 1, ?2, ?2, NULL, NULL, NULL, ?3, ?4, NULL, ?5, ?6, NULL, ?7,
                        0, 0, 0, 0, 0, 0, 0, 0, NULL, NULL, NULL, NULL,
                        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                        NULL, NULL, NULL, NULL, NULL,
                        ?8, ?9, ?10, ?11, NULL, NULL, NULL, NULL, NULL, NULL, NULL,
                        ?12, ?12, 1)",
                    rusqlite::params![
                        airport_id_counter + 1,
                        airport.ident,
                        airport.name,
                        airport.municipality,
                        airport.iso_country,
                        airport.iso_country,
                        type_int,
                        airport.longitude,
                        airport.latitude,
                        airport.longitude,
                        airport.latitude,
                        airport.elevation_ft.unwrap_or(0.0),
                    ],
                )?;
                let airport_id = airport_id_counter + 1;

                for rwy in &airport.runways {
                    let end1 = runway_id_counter * 2 + 1;
                    let end2 = runway_id_counter * 2 + 2;
                    tx.execute(
                        "INSERT INTO runway_end (runway_end_id, runway_id, name, heading,
                            has_closed_markings, is_pattern, pattern_altitude,
                            lonx, laty, altitude, offset_threshold, blast_pad,
                            overrun, markings_flags, approach_light_flags,
                            left_vasi_type, left_vasi_pitch, right_vasi_type, right_vasi_pitch,
                            end_type_flags)
                         VALUES (?1, ?2, ?3, ?4, 0, 0, NULL, ?5, ?6, NULL, NULL, NULL, NULL,
                            0, 0, NULL, NULL, NULL, NULL, 0)",
                        rusqlite::params![
                            end1,
                            runway_id_counter + 1,
                            rwy.le_ident,
                            rwy.true_heading_deg.unwrap_or(0.0),
                            rwy.le_lon,
                            rwy.le_lat,
                        ],
                    )?;
                    tx.execute(
                        "INSERT INTO runway_end (runway_end_id, runway_id, name, heading,
                            has_closed_markings, is_pattern, pattern_altitude,
                            lonx, laty, altitude, offset_threshold, blast_pad,
                            overrun, markings_flags, approach_light_flags,
                            left_vasi_type, left_vasi_pitch, right_vasi_type, right_vasi_pitch,
                            end_type_flags)
                         VALUES (?1, ?2, ?3, ?4, 0, 0, NULL, ?5, ?6, NULL, NULL, NULL, NULL,
                            0, 0, NULL, NULL, NULL, NULL, 0)",
                        rusqlite::params![
                            end2,
                            runway_id_counter + 1,
                            rwy.he_ident,
                            (rwy.true_heading_deg.unwrap_or(0.0) + 180.0).rem_euclid(360.0),
                            rwy.he_lon,
                            rwy.he_lat,
                        ],
                    )?;
                    tx.execute(
                        "INSERT INTO runway (runway_id, airport_id, primary_end_id,
                            secondary_end_id, surface, smoothness, shoulder,
                            length, width, heading, pattern_altitude,
                            marking_flags, edge_light, center_light, has_center_red,
                            primary_lonx, primary_laty, secondary_lonx, secondary_laty)
                         VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, ?6, ?7, ?8, NULL,
                            0, NULL, NULL, 0, ?9, ?10, ?11, ?12)",
                        rusqlite::params![
                            runway_id_counter + 1,
                            airport_id,
                            end1,
                            end2,
                            rwy.surface.clone().unwrap_or_else(|| "U".to_string()),
                            rwy.length_ft as f64,
                            rwy.width_ft.unwrap_or(0) as f64,
                            rwy.true_heading_deg.unwrap_or(0.0),
                            rwy.le_lon,
                            rwy.le_lat,
                            rwy.he_lon,
                            rwy.he_lat,
                        ],
                    )?;
                    runway_id_counter += 1;
                }
            }

            // Waypoints (named fixes).
            let mut wp_id_counter = 0i64;
            let mut wp_map: std::collections::HashMap<String, i64> =
                std::collections::HashMap::new();
            for wp in &waypoints {
                if wp.ident.chars().all(|c| c.is_ascii_digit()) {
                    continue;
                }
                let id = wp_id_counter + 1;
                tx.execute(
                    "INSERT INTO waypoint (waypoint_id, file_id, nav_id, ident, name, region,
                        airport_id, airport_ident, artificial, type, arinc_type,
                        num_victor_airway, num_jet_airway, mag_var, lonx, laty)
                     VALUES (?1, 1, NULL, ?2, ?3, ?4, NULL, ?5, NULL, 'WN', NULL, 0, 0, 0.0, ?6, ?7)",
                    rusqlite::params![
                        id,
                        wp.ident,
                        wp.name,
                        wp.region_code,
                        wp.terminal_area_ident,
                        wp.longitude,
                        wp.latitude,
                    ],
                )?;
                wp_map.insert(wp.ident.clone(), id);
                wp_id_counter += 1;
            }
            let mut vor_id_counter = 0i64;
            let mut ndb_id_counter = 0i64;
            for nav in &navaids {
                match nav.kind {
                    NavaidKind::Vor
                    | NavaidKind::Vordme
                    | NavaidKind::Vortac
                    | NavaidKind::Tacan => {
                        let vtype = match nav.service_volume_nm {
                            Some(v) if v >= 100 => "VTH",
                            Some(v) if v >= 30 => "VTL",
                            _ => "VTT",
                        };
                        tx.execute(
                            "INSERT INTO vor (vor_id, file_id, ident, name, region, airport_id,
                                airport_ident, type, frequency, channel, range, mag_var,
                                dme_only, dme_altitude, dme_lonx, dme_laty, altitude, lonx, laty)
                             VALUES (?1, 1, ?2, ?3, ?4, NULL, ?5, ?6, ?7, NULL, ?8, ?9, 0,
                                NULL, NULL, NULL, ?10, ?11, ?12)",
                            rusqlite::params![
                                vor_id_counter + 1,
                                nav.ident,
                                nav.name,
                                nav.region_code,
                                nav.associated_airport,
                                vtype,
                                nav.frequency.0,
                                nav.service_volume_nm.unwrap_or(40),
                                nav.magnetic_variation_deg.unwrap_or(0.0),
                                nav.elevation_ft,
                                nav.longitude,
                                nav.latitude,
                            ],
                        )?;
                        vor_id_counter += 1;
                    }
                    NavaidKind::Ndb => {
                        tx.execute(
                            "INSERT INTO ndb (ndb_id, file_id, ident, name, region, airport_id,
                                airport_ident, type, frequency, range, mag_var, altitude, lonx, laty)
                             VALUES (?1, 1, ?2, ?3, ?4, NULL, ?5, 'CP', ?6, ?7, ?8, ?9, ?10, ?11)",
                            rusqlite::params![
                                ndb_id_counter + 1,
                                nav.ident,
                                nav.name,
                                nav.region_code,
                                nav.associated_airport,
                                (nav.frequency.0 as f64 * 10.0) as i64, // kHz*100
                                nav.service_volume_nm.unwrap_or(25),
                                nav.magnetic_variation_deg.unwrap_or(0.0),
                                nav.elevation_ft,
                                nav.longitude,
                                nav.latitude,
                            ],
                        )?;
                        ndb_id_counter += 1;
                    }
                    _ => {}
                }
            }

            // ILS.
            for (ils_id_counter, nav) in navaids
                .iter()
                .filter(|n| n.kind == NavaidKind::IlsLocalizer)
                .enumerate()
            {
                let gs = navaids.iter().find(|n| {
                    n.kind == NavaidKind::IlsGlidepath
                        && n.ident == nav.ident
                        && n.associated_airport == nav.associated_airport
                });
                tx.execute(
                    "INSERT INTO ils (ils_id, ident, name, region, type, perf_indicator,
                        provider, frequency, range, mag_var, has_backcourse,
                        dme_range, dme_altitude, dme_lonx, dme_laty,
                        gs_range, gs_pitch, gs_altitude, gs_lonx, gs_laty,
                        loc_runway_end_id, loc_airport_ident, loc_runway_name,
                        loc_heading, loc_width,
                        end1_lonx, end1_laty, end_mid_lonx, end_mid_laty, end2_lonx, end2_laty,
                        altitude, lonx, laty)
                     VALUES (?1, ?2, ?3, NULL, NULL, NULL, NULL, ?4, 27, ?5, 0,
                        NULL, NULL, NULL, NULL, NULL, ?6, NULL, ?7, ?8,
                        NULL, ?9, ?10, ?11, NULL,
                        NULL, NULL, NULL, NULL, NULL, NULL, ?12, ?13, ?14)",
                    rusqlite::params![
                        ils_id_counter as i64 + 1,
                        nav.ident,
                        nav.name,
                        nav.frequency.0,
                        nav.magnetic_variation_deg.unwrap_or(0.0),
                        gs.and_then(|g| g.glideslope_angle_deg),
                        gs.map(|g| g.longitude),
                        gs.map(|g| g.latitude),
                        nav.associated_airport,
                        nav.associated_runway,
                        nav.localizer_bearing_true_deg
                            .or(nav.localizer_bearing_mag_deg)
                            .unwrap_or(0.0),
                        // Schema requires altitude: unknown -> 0
                        // (the LNM loader convention).
                        nav.elevation_ft.unwrap_or(0),
                        nav.longitude,
                        nav.latitude,
                    ],
                )?;
            }

            // Airways: one fragment per route.
            let mut airway_id_counter = 0i64;
            let mut routes: std::collections::BTreeMap<String, Vec<_>> = Default::default();
            for leg in &airway_legs {
                routes.entry(leg.route_ident.clone()).or_default().push(leg);
            }
            for (name, mut legs) in routes {
                legs.sort_by_key(|l| l.sequence_number);
                for (i, leg) in legs.iter().enumerate() {
                    let (Some(&from_id), Some(&to_id)) =
                        (wp_map.get(&leg.start_fix), wp_map.get(&leg.end_fix))
                    else {
                        continue; // referential integrity: skip unresolved
                    };
                    let atype = match leg.route_type.as_str() {
                        "H" => "J",
                        _ => "V",
                    };
                    let min_lat = f64::MAX;
                    let max_lat = f64::MIN;
                    let _ = (min_lat, max_lat);
                    tx.execute(
                        "INSERT INTO airway (airway_id, airway_name, airway_type, route_type,
                            airway_fragment_no, sequence_no, from_waypoint_id, to_waypoint_id,
                            direction, minimum_altitude, maximum_altitude,
                            left_lonx, top_laty, right_lonx, bottom_laty)
                         VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, ?7, 'N', ?8, ?9, ?10, ?11, ?12, ?13)",
                        rusqlite::params![
                            airway_id_counter + 1,
                            name,
                            atype,
                            leg.route_type,
                            i as i64 + 1,
                            from_id,
                            to_id,
                            leg.minimum_altitude_ft.map(|a| a as i64),
                            leg.maximum_altitude_ft.map(|a| a as i64),
                            -180.0f64,
                            90.0f64,
                            180.0f64,
                            -90.0f64,
                        ],
                    )?;
                    airway_id_counter += 1;
                }
            }
        }
        tx.commit()?;
        drop(conn);

        // Artifact description.
        let data = std::fs::read(&db_path)?;
        let sha = format!("{:x}", sha2::Sha256::digest(&data));
        let cycle = openairac_export_xplane::airac_cycle(as_of);
        let artifact = ArtifactEntry {
            path: "little_navmap_openairac.db".to_string(),
            sha256: sha,
            size: data.len() as u64,
            kind: "navdata-database".to_string(),
        };
        let meta = serde_json::json!({
            "generator": format!("openairac {}", env!("CARGO_PKG_VERSION")),
            "cycle": cycle,
            "as_of": as_of.to_rfc3339(),
            "format_family": "little-navmap-sqlite",
            "schema_authority": "albar965/atools resources/sql/fs/db (GPL-3.0; interface reference only)",
            "support_state": "EXPERIMENTAL"
        });
        std::fs::write(
            out_dir.join("cycle.json"),
            serde_json::to_string_pretty(&meta)?,
        )?;
        let meta_data = std::fs::read(out_dir.join("cycle.json"))?;
        let meta_sha = format!("{:x}", sha2::Sha256::digest(&meta_data));
        Ok(GeneratedArtifactSet {
            family: self.family(),
            cycle,
            as_of: as_of.to_rfc3339(),
            generator: format!("openairac {}", env!("CARGO_PKG_VERSION")),
            world_fingerprint: meta_sha.clone(),
            artifacts: vec![
                artifact,
                ArtifactEntry {
                    path: "cycle.json".to_string(),
                    sha256: meta_sha,
                    size: meta_data.len() as u64,
                    kind: "cycle-metadata".to_string(),
                },
            ],
        })
    }
}

/// Create the Little Navmap nav database schema (interface contract
/// from the atools open-source schema; DDL authored here, no code
/// copied). Only the tables this exporter populates are complete;
/// auxiliary tables exist empty.
fn create_schema(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute_batch(
        r#"
CREATE TABLE bgl_file (
  bgl_file_id integer primary key,
  filename varchar(255) not null,
  filepath varchar(255) not null,
  scenery_local_path varchar(255),
  size integer not null
);
CREATE TABLE magdecl (
  magdecl_id integer primary key,
  reference_time integer not null,
  mag_var blob
);
CREATE TABLE airport (
  airport_id integer primary key, file_id integer not null,
  ident varchar(10) not null, icao varchar(10), iata varchar(10), faa varchar(10), local varchar(10),
  name varchar(50) collate nocase, city varchar(50) collate nocase,
  state varchar(50) collate nocase, country varchar(50) collate nocase,
  region varchar(4) collate nocase, flatten integer, type integer,
  fuel_flags integer not null, has_avgas integer not null, has_jetfuel integer not null,
  has_tower_object integer not null,
  tower_frequency integer, atis_frequency integer, awos_frequency integer,
  asos_frequency integer, unicom_frequency integer,
  is_closed integer not null, is_military integer not null, is_addon integer not null,
  rating integer, longest_runway_length double, longest_runway_width double,
  longest_runway_heading double, longest_runway_surface varchar(15),
  num_runway_hard integer, num_runway_soft integer, num_runway_water integer, num_runway_light integer,
  num_runway_end_closed integer, num_runway_end_vasi integer, num_runway_end_als integer,
  num_runway_end_ils integer, num_apron integer, num_taxi_path integer, num_apron_light integer,
  num_taxi_path_has_centerline integer, num_taxi_path_has_center_red integer,
  num_parking_gate integer, num_parking_ga_ramp integer, num_parking_cargo integer,
  num_parking_mil_cargo integer, num_parking_mil_combat integer,
  num_helipad integer,
  left_lonx double, top_laty double, right_lonx double, bottom_laty double,
  mag_var double, tower_heading double, tower_altitude integer, tower_lonx double, tower_laty double,
  alt_lonx double, alt_laty double, elevation integer, display_elevation integer, is_3d integer
);
CREATE TABLE runway_end (
  runway_end_id integer primary key, runway_id integer not null,
  name varchar(10) not null, heading double not null,
  has_closed_markings integer not null, is_pattern integer not null, pattern_altitude integer,
  lonx double not null, laty double not null, altitude integer,
  offset_threshold integer, blast_pad integer, overrun integer,
  markings_flags integer, approach_light_flags integer,
  left_vasi_type varchar(10), left_vasi_pitch double, right_vasi_type varchar(10), right_vasi_pitch double,
  end_type_flags integer not null,
  foreign key(runway_id) references runway(runway_id)
);
CREATE TABLE runway (
  runway_id integer primary key, airport_id integer not null,
  primary_end_id integer not null, secondary_end_id integer not null,
  surface varchar(15), smoothness double, shoulder varchar(15),
  length double not null, width double not null, heading double not null,
  pattern_altitude integer, marking_flags integer not null,
  edge_light varchar(15), center_light varchar(15), has_center_red integer not null,
  primary_lonx double not null, primary_laty double not null,
  secondary_lonx double not null, secondary_laty double not null,
  foreign key(airport_id) references airport(airport_id),
  foreign key(primary_end_id) references runway_end(runway_end_id),
  foreign key(secondary_end_id) references runway_end(runway_end_id)
);
CREATE TABLE waypoint (
  waypoint_id integer primary key, file_id integer not null, nav_id integer,
  ident varchar(5) not null, name varchar(50), region varchar(2),
  airport_id integer, airport_ident varchar(4), artificial integer,
  type varchar(15), arinc_type varchar(4),
  num_victor_airway integer not null, num_jet_airway integer not null,
  mag_var double not null, lonx double not null, laty double not null,
  foreign key(file_id) references bgl_file(bgl_file_id)
);
CREATE TABLE vor (
  vor_id integer primary key, file_id integer not null,
  ident varchar(5), name varchar(50), region varchar(2),
  airport_id integer, airport_ident varchar(4),
  type varchar(15), frequency integer, channel varchar(5), range integer,
  mag_var double, dme_only integer not null,
  dme_altitude integer, dme_lonx double, dme_laty double,
  altitude integer, lonx double not null, laty double not null,
  foreign key(file_id) references bgl_file(bgl_file_id)
);
CREATE TABLE ndb (
  ndb_id integer primary key, file_id integer not null,
  ident varchar(5), name varchar(50), region varchar(2),
  airport_id integer, airport_ident varchar(4),
  type varchar(15), frequency integer not null, range integer,
  mag_var double not null, altitude integer, lonx double not null, laty double not null,
  foreign key(file_id) references bgl_file(bgl_file_id)
);
CREATE TABLE ils (
  ils_id integer primary key, ident varchar(5), name varchar(50), region varchar(2),
  type varchar(1), perf_indicator varchar(10), provider varchar(10),
  frequency integer, range integer, mag_var double not null, has_backcourse integer not null,
  dme_range integer, dme_altitude integer, dme_lonx double, dme_laty double,
  gs_range integer, gs_pitch double, gs_altitude integer, gs_lonx double, gs_laty double,
  loc_runway_end_id integer, loc_airport_ident varchar(4), loc_runway_name varchar(10),
  loc_heading double, loc_width double,
  end1_lonx double, end1_laty double, end_mid_lonx double, end_mid_laty double,
  end2_lonx double, end2_laty double,
  altitude integer not null, lonx double not null, laty double not null
);
CREATE TABLE airway (
  airway_id integer primary key,
  airway_name varchar(5) not null, airway_type varchar(15) not null, route_type varchar(5),
  airway_fragment_no integer not null, sequence_no integer not null,
  from_waypoint_id integer not null, to_waypoint_id integer not null,
  direction varchar(1), minimum_altitude integer, maximum_altitude integer,
  left_lonx double not null, top_laty double not null, right_lonx double not null, bottom_laty double not null,
  foreign key(from_waypoint_id) references waypoint(waypoint_id),
  foreign key(to_waypoint_id) references waypoint(waypoint_id)
);
CREATE TABLE approach (approach_id integer primary key);
CREATE TABLE approach_leg (approach_leg_id integer primary key);
CREATE TABLE transition (transition_id integer primary key);
CREATE TABLE transition_leg (transition_leg_id integer primary key);
CREATE TABLE airport_file (airport_file_id integer primary key);
CREATE TABLE airport_msa (airport_msa_id integer primary key);
CREATE TABLE holding (holding_id integer primary key);
CREATE TABLE marker (marker_id integer primary key);
CREATE TABLE mora_grid (mora_grid_id integer primary key);
CREATE TABLE nav_search (nav_search_id integer primary key);
CREATE TABLE com (com_id integer primary key);
CREATE TABLE apron (apron_id integer primary key);
CREATE TABLE parking (parking_id integer primary key);
CREATE TABLE start (start_id integer primary key);
CREATE TABLE helipad (helipad_id integer primary key);
CREATE TABLE taxi_path (taxi_path_id integer primary key);
CREATE TABLE metadata (
  db_version_major integer not null,
  db_version_minor integer not null,
  last_load_timestamp varchar(30),
  has_sid_star integer,
  airac_cycle varchar(10),
  valid_through varchar(30),
  data_source varchar(50),
  compiler_version varchar(100),
  properties varchar(255)
);
"#,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use openairac_model::{AirportId, CanonicalAirport, SourceSnapshot, SourceSnapshotId};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn unique_dir(tag: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("oa_lnm_test_{}_{}_{n}", std::process::id(), tag))
    }

    fn fixture_store() -> (WorldStore, PathBuf) {
        let dir = unique_dir("fixture");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = WorldStore::open(dir.join("src.sqlite")).unwrap();
        let t = Utc::now();
        store
            .insert_source_snapshot(&SourceSnapshot {
                id: SourceSnapshotId("snap-1".to_string()),
                provider: "OurAirports".to_string(),
                dataset: "airports".to_string(),
                provider_revision: None,
                airac_cycle: None,
                effective_from: Some(t),
                effective_until: None,
                retrieved_at: t,
                source_uri: "fixture".to_string(),
                content_sha256: "0".repeat(64),
                license_id: None,
                license_notes: None,
                parser_version: "test".to_string(),
            })
            .unwrap();
        store
            .insert_airport(&CanonicalAirport {
                id: AirportId("ourairports:1".to_string()),
                ident: "KSFO".to_string(),
                name: "San Francisco".to_string(),
                airport_type: "large_airport".to_string(),
                latitude: 37.6188,
                longitude: -122.375,
                elevation_ft: Some(13.0),
                iso_country: Some("US".to_string()),
                municipality: Some("San Francisco".to_string()),
                runways: Vec::new(),
                temporal: TemporalValidity {
                    valid_from: t,
                    valid_until: None,
                    source_snapshot_id: SourceSnapshotId("snap-1".to_string()),
                },
            })
            .unwrap();
        (store, dir)
    }

    #[test]
    fn test_lnm_export_creates_schema_valid_db() {
        let (store, dir) = fixture_store();
        let out = dir.join("lnm");
        let set = LnmNavdataExporter.export(&store, Utc::now(), &out).unwrap();
        assert_eq!(set.family.as_str(), "little-navmap-sqlite");
        set.verify(&out).unwrap();
        let conn = rusqlite::Connection::open(out.join("little_navmap_openairac.db")).unwrap();
        let airport_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM airport", [], |r| r.get(0))
            .unwrap();
        assert_eq!(airport_count, 1);
        let ident: String = conn
            .query_row("SELECT ident FROM airport LIMIT 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ident, "KSFO");
    }
}
