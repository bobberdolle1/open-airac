//! Little Navmap SQLite nav database exporter.
//!
//! Format authority: the Little Navmap / atools open-source schema
//! (GPL-3.0; albar965/atools `resources/sql/fs/db/create_nav_schema.sql`
//! + `create_ap_schema.sql`).
//!
//! The schema interface (table/column layout) is used as the
//! compatibility contract; no GPL code is copied into this repository.
//!
//! Support state: SUPPORTED. The database is schema-valid, referentially
//! consistent, and verified against unmodified Little Navmap 3.0.18.

use anyhow::Result;
use chrono::{DateTime, Utc};
use openairac_export::{ArtifactEntry, FormatExporter, GeneratedArtifactSet, families};
#[cfg(test)]
use openairac_model::TemporalValidity;
use openairac_model::{CanonicalProcedureLeg, NavaidKind};
use openairac_store::WorldStore;
use sha2::Digest;
use std::collections::{BTreeMap, HashMap};
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
        let db_path = out_dir.join("openairac.sqlite");
        let legacy_db_path = out_dir.join("little_navmap_openairac.db");
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(&legacy_db_path);

        let mut conn = rusqlite::Connection::open(&db_path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF; PRAGMA foreign_keys=OFF;",
        )?;

        create_schema(&conn)?;

        let airports = store.query_airports_at(as_of)?;
        let navaids = store.query_navaids_at(as_of)?;
        let waypoints = store.query_waypoints_at(as_of)?;
        let airway_legs = store.query_airway_legs_at(as_of)?;
        let procedure_legs = store.query_procedure_legs_at(as_of)?;

        // scenery_area: primary area for OpenAIRAC
        conn.execute(
            "INSERT INTO scenery_area (scenery_area_id, number, layer, title, remote_path, local_path, active, required, exclude)
             VALUES (1, 1, 1, 'OpenAIRAC', NULL, NULL, 1, 1, NULL)",
            [],
        )?;

        // bgl_file: one synthetic file for provenance.
        conn.execute(
            "INSERT INTO bgl_file (bgl_file_id, scenery_area_id, bgl_create_time, file_modification_time, filepath, filename, size, comment)
             VALUES (1, 1, ?1, ?1, 'openairac', 'openairac', ?2, NULL)",
            rusqlite::params![as_of.timestamp(), 0i64],
        )?;

        // Note: magdecl table is created empty; LNM uses internal WMM fallback when magdecl is empty.
        // metadata: required for Little Navmap database compatibility.
        let cycle_str = openairac_export_xplane::airac_cycle(as_of);
        let has_procs = if procedure_legs.is_empty() {
            0i64
        } else {
            1i64
        };
        conn.execute(
            "INSERT INTO metadata (db_version_major, db_version_minor, last_load_timestamp,
                has_sid_star, airac_cycle, valid_through, data_source, compiler_version, properties)
             VALUES (14, 29, ?1, ?2, ?3, NULL, 'OPENAIRAC', ?4, NULL)",
            rusqlite::params![
                as_of.to_rfc3339(),
                has_procs,
                cycle_str,
                format!("OpenAIRAC {}", env!("CARGO_PKG_VERSION")),
            ],
        )?;

        let tx = conn.transaction()?;
        {
            // Airports + runways + runway ends.
            let mut runway_id_counter = 0i64;
            let mut runway_end_map: HashMap<(String, String), i64> = HashMap::new(); // (ident, rwy_name) -> runway_end_id
            let mut airport_id_map: HashMap<String, i64> = HashMap::new(); // ident -> airport_id

            for (airport_idx, airport) in airports.iter().enumerate() {
                let airport_id = airport_idx as i64 + 1;
                airport_id_map.insert(airport.ident.clone(), airport_id);
                let type_int: i64 = match airport.airport_type.as_str() {
                    "seaplane_base" => 16,
                    "heliport" => 17,
                    _ => 1,
                };

                let num_rwy_hard = airport
                    .runways
                    .iter()
                    .filter(|r| {
                        r.surface
                            .as_deref()
                            .map(|s| s.contains("ASP") || s.contains("CON") || s.contains("HARD"))
                            .unwrap_or(true)
                    })
                    .count() as i64;
                let num_rwy_soft = (airport.runways.len() as i64).saturating_sub(num_rwy_hard);
                let longest_len = airport
                    .runways
                    .iter()
                    .map(|r| r.length_ft)
                    .max()
                    .unwrap_or(0) as f64;
                let longest_wid = airport
                    .runways
                    .iter()
                    .map(|r| r.width_ft.unwrap_or(0))
                    .max()
                    .unwrap_or(0) as f64;
                let longest_hdg = airport
                    .runways
                    .first()
                    .map(|r| r.true_heading())
                    .unwrap_or(0.0);
                let longest_surf = airport
                    .runways
                    .first()
                    .and_then(|r| r.surface.clone())
                    .unwrap_or_else(|| "HARD".to_string());

                tx.execute(
                    "INSERT INTO airport (airport_id, file_id, ident, icao, iata, faa, local,
                        name, city, state, country, region, flatten, type,
                        fuel_flags, has_avgas, has_jetfuel, has_tower_object,
                        tower_frequency, atis_frequency, awos_frequency, asos_frequency, unicom_frequency,
                        is_closed, is_military, is_addon,
                        num_com, num_parking_gate, num_parking_ga_ramp, num_parking_cargo,
                        num_parking_mil_cargo, num_parking_mil_combat, num_approach,
                        num_runway_hard, num_runway_soft, num_runway_water, num_runway_light,
                        num_runway_end_closed, num_runway_end_vasi, num_runway_end_als,
                        num_runway_end_ils, num_apron, num_taxi_path, num_helipad, num_jetway, num_starts,
                        longest_runway_length, longest_runway_width, longest_runway_heading,
                        longest_runway_surface, num_runways, largest_parking_ramp, largest_parking_gate,
                        rating, is_3d, scenery_local_path, bgl_filename,
                        left_lonx, top_laty, right_lonx, bottom_laty,
                        mag_var, tower_altitude, tower_lonx, tower_laty,
                        transition_altitude, transition_level,
                        altitude, lonx, laty)
                    VALUES (?1, 1, ?2, ?2, NULL, NULL, NULL,
                        ?3, ?4, NULL, ?5, ?6, NULL, ?7,
                        0, 0, 0, 0,
                        NULL, NULL, NULL, NULL, NULL,
                        0, 0, 0,
                        0, 0, 0, 0, 0, 0, 0,
                        ?8, ?9, 0, 0,
                        0, 0, 0, 0, 0, 0, 0, 0, 0,
                        ?10, ?11, ?12, ?13, ?14, NULL, NULL,
                        0, 0, NULL, NULL,
                        ?15, ?16, ?17, ?18,
                        0.0, NULL, NULL, NULL,
                        NULL, NULL,
                        ?19, ?20, ?21)",
                    rusqlite::params![
                        airport_id,
                        airport.ident,
                        airport.name,
                        airport.municipality,
                        airport.iso_country,
                        airport.iso_country,
                        type_int,
                        num_rwy_hard,
                        num_rwy_soft,
                        longest_len as i64,
                        longest_wid as i64,
                        longest_hdg,
                        longest_surf,
                        airport.runways.len() as i64,
                        airport.longitude,
                        airport.latitude,
                        airport.longitude,
                        airport.latitude,
                        airport.elevation_ft.unwrap_or(0.0) as i64,
                        airport.longitude,
                        airport.latitude,
                    ],
                )?;

                for rwy in &airport.runways {
                    let end1 = runway_id_counter * 2 + 1;
                    let end2 = runway_id_counter * 2 + 2;
                    runway_end_map.insert((airport.ident.clone(), rwy.le_ident.clone()), end1);
                    runway_end_map.insert((airport.ident.clone(), rwy.he_ident.clone()), end2);

                    let hdg1 = rwy.true_heading();
                    let hdg2 = rwy.reciprocal_true_heading();
                    let alt = airport.elevation_ft.unwrap_or(0.0) as i64;

                    tx.execute(
                        "INSERT INTO runway_end (runway_end_id, name, end_type, offset_threshold, blast_pad, overrun,
                            left_vasi_type, left_vasi_pitch, right_vasi_type, right_vasi_pitch,
                            has_closed_markings, has_stol_markings, is_takeoff, is_landing, is_pattern,
                            app_light_system_type, has_end_lights, has_reils, has_touchdown_lights,
                            num_strobes, ils_ident, heading, altitude, lonx, laty)
                         VALUES (?1, ?2, 'C', NULL, NULL, NULL,
                            NULL, NULL, NULL, NULL,
                            0, 0, 1, 1, 0,
                            NULL, 0, 0, 0,
                            0, NULL, ?3, ?4, ?5, ?6)",
                        rusqlite::params![end1, rwy.le_ident, hdg1, alt, rwy.le_lon, rwy.le_lat],
                    )?;

                    tx.execute(
                        "INSERT INTO runway_end (runway_end_id, name, end_type, offset_threshold, blast_pad, overrun,
                            left_vasi_type, left_vasi_pitch, right_vasi_type, right_vasi_pitch,
                            has_closed_markings, has_stol_markings, is_takeoff, is_landing, is_pattern,
                            app_light_system_type, has_end_lights, has_reils, has_touchdown_lights,
                            num_strobes, ils_ident, heading, altitude, lonx, laty)
                         VALUES (?1, ?2, 'C', NULL, NULL, NULL,
                            NULL, NULL, NULL, NULL,
                            0, 0, 1, 1, 0,
                            NULL, 0, 0, 0,
                            0, NULL, ?3, ?4, ?5, ?6)",
                        rusqlite::params![end2, rwy.he_ident, hdg2, alt, rwy.he_lon, rwy.he_lat],
                    )?;

                    let rwy_id = runway_id_counter + 1;
                    let mid_lon = (rwy.le_lon + rwy.he_lon) / 2.0;
                    let mid_lat = (rwy.le_lat + rwy.he_lat) / 2.0;
                    tx.execute(
                        "INSERT INTO runway (runway_id, airport_id, primary_end_id, secondary_end_id,
                            surface, smoothness, shoulder, length, width, heading, pattern_altitude,
                            marking_flags, edge_light, center_light, has_center_red,
                            primary_lonx, primary_laty, secondary_lonx, secondary_laty,
                            altitude, lonx, laty)
                         VALUES (?1, ?2, ?3, ?4,
                            ?5, NULL, NULL, ?6, ?7, ?8, NULL,
                            0, NULL, NULL, 0,
                            ?9, ?10, ?11, ?12,
                            ?13, ?14, ?15)",
                        rusqlite::params![
                            rwy_id,
                            airport_id,
                            end1,
                            end2,
                            rwy.surface.clone().unwrap_or_else(|| "U".to_string()),
                            rwy.length_ft as f64,
                            rwy.width_ft.unwrap_or(0) as f64,
                            hdg1,
                            rwy.le_lon,
                            rwy.le_lat,
                            rwy.he_lon,
                            rwy.he_lat,
                            alt,
                            mid_lon,
                            mid_lat,
                        ],
                    )?;
                    runway_id_counter += 1;
                }
            }

            // Waypoints (named fixes).
            let mut wp_id_counter = 0i64;
            let mut wp_map: HashMap<String, i64> = HashMap::new();
            let mut wp_coords: HashMap<String, (f64, f64)> = HashMap::new();
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
                wp_coords.insert(wp.ident.clone(), (wp.longitude, wp.latitude));
                wp_id_counter += 1;
            }

            // VOR & NDB.
            let mut vor_id_counter = 0i64;
            let mut ndb_id_counter = 0i64;
            for nav in &navaids {
                match nav.kind {
                    NavaidKind::Vor
                    | NavaidKind::Vordme
                    | NavaidKind::Vortac
                    | NavaidKind::Tacan
                    | NavaidKind::Rsbn => {
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
                        wp_coords.insert(nav.ident.clone(), (nav.longitude, nav.latitude));
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
                                (nav.frequency.0 as f64 * 10.0) as i64,
                                nav.service_volume_nm.unwrap_or(25),
                                nav.magnetic_variation_deg.unwrap_or(0.0),
                                nav.elevation_ft,
                                nav.longitude,
                                nav.latitude,
                            ],
                        )?;
                        wp_coords.insert(nav.ident.clone(), (nav.longitude, nav.latitude));
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
                let rwy_end_id = nav.associated_airport.as_ref().and_then(|apt| {
                    nav.associated_runway
                        .as_ref()
                        .and_then(|rwy| runway_end_map.get(&(apt.clone(), rwy.clone())).copied())
                });

                let loc_heading_true = nav
                    .localizer_bearing_true_deg
                    .or_else(|| {
                        nav.localizer_bearing_mag_deg.map(|mag| {
                            (mag + nav.magnetic_variation_deg.unwrap_or(0.0)).rem_euclid(360.0)
                        })
                    })
                    .unwrap_or(0.0);

                let loc_width: f64 = 5.0; // Standard 5° localizer feather width
                let feather_len_m = 10.0 * 1852.0; // 10 NM standard display feather length
                let opp_hdg = (loc_heading_true + 180.0).rem_euclid(360.0);
                let (p1_lat, p1_lon) = openairac_model::geodesic_endpoint(
                    nav.latitude,
                    nav.longitude,
                    feather_len_m,
                    (opp_hdg - loc_width / 2.0).rem_euclid(360.0),
                );
                let (p2_lat, p2_lon) = openairac_model::geodesic_endpoint(
                    nav.latitude,
                    nav.longitude,
                    feather_len_m,
                    (opp_hdg + loc_width / 2.0).rem_euclid(360.0),
                );
                let (mid_lat, mid_lon) = openairac_model::geodesic_endpoint(
                    nav.latitude,
                    nav.longitude,
                    feather_len_m * 0.9,
                    opp_hdg,
                );

                tx.execute(
                    "INSERT INTO ils (ils_id, ident, name, region, type, perf_indicator,
                        provider, frequency, range, mag_var, has_backcourse,
                        dme_range, dme_altitude, dme_lonx, dme_laty,
                        gs_range, gs_pitch, gs_altitude, gs_lonx, gs_laty,
                        loc_runway_end_id, loc_airport_ident, loc_runway_name,
                        loc_heading, loc_width,
                        end1_lonx, end1_laty, end_mid_lonx, end_mid_laty, end2_lonx, end2_laty,
                        altitude, lonx, laty)
                     VALUES (?1, ?2, ?3, NULL, 'I', NULL, NULL, ?4, 27, ?5, 0,
                        NULL, NULL, NULL, NULL, NULL, ?6, NULL, ?7, ?8,
                        ?9, ?10, ?11, ?12, ?13,
                        ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
                    rusqlite::params![
                        ils_id_counter as i64 + 1,
                        nav.ident,
                        nav.name,
                        nav.frequency.0,
                        nav.magnetic_variation_deg.unwrap_or(0.0),
                        gs.and_then(|g| g.glideslope_angle_deg),
                        gs.map(|g| g.longitude),
                        gs.map(|g| g.latitude),
                        rwy_end_id,
                        nav.associated_airport,
                        nav.associated_runway,
                        loc_heading_true,
                        loc_width,
                        p1_lon,
                        p1_lat,
                        mid_lon,
                        mid_lat,
                        p2_lon,
                        p2_lat,
                        nav.elevation_ft.unwrap_or(0),
                        nav.longitude,
                        nav.latitude,
                    ],
                )?;
            }
            // Airways.
            let mut airway_id_counter = 0i64;
            let mut routes: BTreeMap<String, Vec<_>> = Default::default();
            for leg in &airway_legs {
                routes.entry(leg.route_ident.clone()).or_default().push(leg);
            }
            for (name, mut legs) in routes {
                legs.sort_by_key(|l| l.sequence_number);
                for (i, leg) in legs.iter().enumerate() {
                    let (Some(&from_id), Some(&to_id)) =
                        (wp_map.get(&leg.start_fix), wp_map.get(&leg.end_fix))
                    else {
                        continue;
                    };
                    let atype = match leg.route_type.as_str() {
                        "H" => "J",
                        _ => "V",
                    };
                    let from_c = wp_coords.get(&leg.start_fix).copied().unwrap_or((0.0, 0.0));
                    let to_c = wp_coords.get(&leg.end_fix).copied().unwrap_or((0.0, 0.0));

                    let min_lon = from_c.0.min(to_c.0);
                    let max_lon = from_c.0.max(to_c.0);
                    let min_lat = from_c.1.min(to_c.1);
                    let max_lat = from_c.1.max(to_c.1);

                    tx.execute(
                        "INSERT INTO airway (airway_id, airway_name, airway_type, route_type,
                            airway_fragment_no, sequence_no, from_waypoint_id, to_waypoint_id,
                            direction, minimum_altitude, maximum_altitude,
                            left_lonx, top_laty, right_lonx, bottom_laty,
                            from_lonx, from_laty, to_lonx, to_laty)
                         VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, ?7, 'N', ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
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
                            min_lon,
                            max_lat,
                            max_lon,
                            min_lat,
                            from_c.0,
                            from_c.1,
                            to_c.0,
                            to_c.1,
                        ],
                    )?;
                    airway_id_counter += 1;
                }
            }

            // Terminal Procedures (SIDs, STARs, Approaches).
            export_procedures(
                &tx,
                &procedure_legs,
                &airport_id_map,
                &runway_end_map,
                &wp_coords,
            )?;
        }
        tx.commit()?;
        drop(conn);

        // Copy primary database to legacy database path for backward compatibility.
        std::fs::copy(&db_path, &legacy_db_path)?;

        // Artifact descriptions.
        let data = std::fs::read(&db_path)?;
        let sha = format!("{:x}", sha2::Sha256::digest(&data));
        let cycle = openairac_export_xplane::airac_cycle(as_of);

        let artifact_primary = ArtifactEntry {
            path: "openairac.sqlite".to_string(),
            sha256: sha.clone(),
            size: data.len() as u64,
            kind: "navdata-database".to_string(),
        };
        let artifact_legacy = ArtifactEntry {
            path: "little_navmap_openairac.db".to_string(),
            sha256: sha.clone(),
            size: data.len() as u64,
            kind: "navdata-database".to_string(),
        };

        let meta = serde_json::json!({
            "generator": format!("openairac {}", env!("CARGO_PKG_VERSION")),
            "cycle": cycle,
            "as_of": as_of.to_rfc3339(),
            "format_family": "little-navmap-sqlite",
            "schema_authority": "albar965/atools resources/sql/fs/db (GPL-3.0; interface reference only)",
            "support_state": "SUPPORTED"
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
                artifact_primary,
                artifact_legacy,
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

/// Export procedure legs into LNM tables: approach, approach_leg, transition, transition_leg.
fn export_procedures(
    tx: &rusqlite::Transaction,
    legs: &[CanonicalProcedureLeg],
    airport_id_map: &HashMap<String, i64>,
    runway_end_map: &HashMap<(String, String), i64>,
    wp_coords: &HashMap<String, (f64, f64)>,
) -> Result<()> {
    // Group procedure legs by (airport_ident, procedure_kind, procedure_ident)
    type ProcKey = (String, char, String);
    let mut grouped_procs: BTreeMap<ProcKey, Vec<&CanonicalProcedureLeg>> = BTreeMap::new();
    for leg in legs {
        let key = (
            leg.airport_ident.clone(),
            leg.procedure_kind,
            leg.procedure_ident.clone(),
        );
        grouped_procs.entry(key).or_default().push(leg);
    }

    let mut approach_id_counter = 0i64;
    let mut approach_leg_id_counter = 0i64;
    let mut transition_id_counter = 0i64;
    let mut transition_leg_id_counter = 0i64;

    for ((airport_ident, kind, proc_ident), mut proc_legs) in grouped_procs {
        let airport_id = match airport_id_map.get(&airport_ident) {
            Some(&id) => id,
            None => continue, // referential integrity
        };

        proc_legs.sort_by_key(|l| (l.transition_ident.clone(), l.sequence_number));

        // Separate common/runway legs and transition legs
        let mut common_legs: Vec<&CanonicalProcedureLeg> = Vec::new();
        let mut transitions: BTreeMap<String, Vec<&CanonicalProcedureLeg>> = BTreeMap::new();

        for leg in proc_legs {
            let trans = leg.transition_ident.trim();
            if trans.is_empty() || trans == "ALL" || trans == "RW" || trans.starts_with("RW") {
                common_legs.push(leg);
            } else {
                transitions.entry(trans.to_string()).or_default().push(leg);
            }
        }

        // Derive procedure parameters
        let (proc_type, suffix, has_gps_overlay) = match kind {
            'D' => ("GPS".to_string(), Some("D".to_string()), 1i64),
            'E' => ("GPS".to_string(), Some("A".to_string()), 1i64),
            'F' => {
                let ptype = if proc_ident.starts_with('I') {
                    "ILS"
                } else if proc_ident.starts_with('R') {
                    "RNAV"
                } else if proc_ident.starts_with('V') {
                    "VOR"
                } else if proc_ident.starts_with('D') {
                    "VORDME"
                } else if proc_ident.starts_with('N') {
                    "NDB"
                } else if proc_ident.starts_with('L') {
                    "LOC"
                } else {
                    "GPS"
                };
                (ptype.to_string(), None, 1i64)
            }
            _ => ("GPS".to_string(), None, 1i64),
        };

        let mut runway_name: Option<String> = None;
        if kind == 'F' {
            // Extract runway from approach ident if present (e.g. I22L -> 22L, R13R -> 13R)
            if proc_ident.len() >= 3 {
                let candidate = &proc_ident[1..];
                if candidate
                    .chars()
                    .next()
                    .map(|c| c.is_ascii_digit())
                    .unwrap_or(false)
                {
                    runway_name = Some(candidate.to_string());
                }
            }
        }

        let rwy_end_id = runway_name.as_ref().and_then(|r| {
            runway_end_map
                .get(&(airport_ident.clone(), r.clone()))
                .copied()
        });

        let first_fix = common_legs
            .first()
            .map(|l| l.fix_ident.clone())
            .unwrap_or_else(|| proc_ident.clone());

        approach_id_counter += 1;
        let app_id = approach_id_counter;

        tx.execute(
            "INSERT INTO approach (approach_id, airport_id, runway_end_id, arinc_name,
                airport_ident, runway_name, type, suffix, has_gps_overlay, has_vertical_angle, has_rnp,
                fix_type, fix_ident, fix_region, fix_airport_ident, aircraft_category, altitude, heading, missed_altitude)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, 0, 'W', ?10, NULL, NULL, NULL, NULL, NULL, NULL)",
            rusqlite::params![
                app_id,
                airport_id,
                rwy_end_id,
                proc_ident,
                airport_ident,
                runway_name,
                proc_type,
                suffix,
                has_gps_overlay,
                first_fix,
            ],
        )?;

        // Insert common legs into approach_leg
        for leg in common_legs {
            approach_leg_id_counter += 1;
            let leg_id = approach_leg_id_counter;
            let coords = wp_coords.get(&leg.fix_ident).copied();
            let is_missed = if leg.route_type.contains('M') {
                1i64
            } else {
                0i64
            };
            let speed = leg.speed_limit_kts.map(|s| s as i64);
            let alt1 = leg.altitude_1_ft.map(|a| a as f64);
            let alt2 = leg.altitude_2_ft.map(|a| a as f64);
            let alt_desc = leg.altitude_descriptor.map(|c| c.to_string());
            let turn_dir = leg.turn_direction.map(|c| c.to_string());
            tx.execute(
                "INSERT INTO approach_leg (approach_leg_id, approach_id, is_missed, type,
                    arinc_descr_code, approach_fix_type, alt_descriptor, turn_direction, rnp,
                    fix_type, fix_ident, fix_region, fix_airport_ident, fix_lonx, fix_laty,
                    recommended_fix_type, recommended_fix_ident, recommended_fix_region,
                    recommended_fix_lonx, recommended_fix_laty, is_flyover, is_true_course,
                    course, distance, time, theta, rho, altitude1, altitude2,
                    speed_limit_type, speed_limit, vertical_angle)
                 VALUES (?1, ?2, ?3, ?4, NULL, 'W', ?5, ?6, ?7,
                    'W', ?8, NULL, NULL, ?9, ?10,
                    NULL, ?11, NULL, NULL, NULL,
                    0, 0, ?12, ?13, NULL, NULL, NULL, ?14, ?15,
                    NULL, ?16, ?17)",
                rusqlite::params![
                    leg_id,
                    app_id,
                    is_missed,
                    leg.path_terminator,
                    alt_desc,
                    turn_dir,
                    leg.rnp_nm,
                    leg.fix_ident,
                    coords.map(|c| c.0),
                    coords.map(|c| c.1),
                    leg.recommended_navaid,
                    leg.course_a_deg,
                    leg.distance_a_nm,
                    alt1,
                    alt2,
                    speed,
                    leg.vertical_angle_deg,
                ],
            )?;
        }

        // Insert transitions
        for (trans_name, trans_legs) in transitions {
            transition_id_counter += 1;
            let trans_id = transition_id_counter;
            let first_trans_fix = trans_legs
                .first()
                .map(|l| l.fix_ident.clone())
                .unwrap_or_else(|| trans_name.clone());

            tx.execute(
                "INSERT INTO transition (transition_id, approach_id, type, fix_type, fix_ident,
                    fix_region, fix_airport_ident, aircraft_category, altitude,
                    dme_ident, dme_region, dme_airport_ident, dme_radial, dme_distance)
                 VALUES (?1, ?2, 'F', 'W', ?3, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL)",
                rusqlite::params![trans_id, app_id, first_trans_fix],
            )?;

            for leg in trans_legs {
                transition_leg_id_counter += 1;
                let trans_leg_id = transition_leg_id_counter;
                let coords = wp_coords.get(&leg.fix_ident).copied();
                let speed = leg.speed_limit_kts.map(|s| s as i64);
                let alt1 = leg.altitude_1_ft.map(|a| a as f64);
                let alt2 = leg.altitude_2_ft.map(|a| a as f64);
                let alt_desc = leg.altitude_descriptor.map(|c| c.to_string());
                let turn_dir = leg.turn_direction.map(|c| c.to_string());
                tx.execute(
                    "INSERT INTO transition_leg (transition_leg_id, transition_id, type,
                        arinc_descr_code, approach_fix_type, alt_descriptor, turn_direction, rnp,
                        fix_type, fix_ident, fix_region, fix_airport_ident, fix_lonx, fix_laty,
                        recommended_fix_type, recommended_fix_ident, recommended_fix_region,
                        recommended_fix_lonx, recommended_fix_laty, is_flyover, is_true_course,
                        course, distance, time, theta, rho, altitude1, altitude2,
                        speed_limit_type, speed_limit, vertical_angle)
                     VALUES (?1, ?2, ?3, NULL, 'W', ?4, ?5, ?6,
                        'W', ?7, NULL, NULL, ?8, ?9,
                        NULL, ?10, NULL, NULL, NULL,
                        0, 0, ?11, ?12, NULL, NULL, NULL, ?13, ?14,
                        NULL, ?15, ?16)",
                    rusqlite::params![
                        trans_leg_id,
                        trans_id,
                        leg.path_terminator,
                        alt_desc,
                        turn_dir,
                        leg.rnp_nm,
                        leg.fix_ident,
                        coords.map(|c| c.0),
                        coords.map(|c| c.1),
                        leg.recommended_navaid,
                        leg.course_a_deg,
                        leg.distance_a_nm,
                        alt1,
                        alt2,
                        speed,
                        leg.vertical_angle_deg,
                    ],
                )?;
            }
        }
    }
    Ok(())
}

/// Create the complete Little Navmap nav database schema (v14.29).
fn create_schema(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute_batch(
        r#"
CREATE TABLE bgl_file (
  bgl_file_id integer primary key,
  scenery_area_id integer not null,
  bgl_create_time integer not null,
  file_modification_time integer not null,
  filepath varchar(1000) collate nocase,
  filename varchar(250) collate nocase,
  size integer not null,
  comment varchar(1000),
  foreign key(scenery_area_id) references scenery_area(scenery_area_id)
);
CREATE TABLE magdecl (
  magdecl_id integer primary key,
  reference_time integer not null,
  mag_var blob
);
CREATE TABLE airport (
  airport_id integer primary key,
  file_id integer not null,
  ident varchar(10) not null,
  icao varchar(10),
  iata varchar(10),
  faa varchar(10),
  local varchar(10),
  name varchar(50) collate nocase,
  city varchar(50) collate nocase,
  state varchar(50) collate nocase,
  country varchar(50) collate nocase,
  region varchar(4) collate nocase,
  flatten integer,
  type integer,
  fuel_flags integer not null,
  has_avgas integer not null,
  has_jetfuel integer not null,
  has_tower_object integer not null,
  tower_frequency integer,
  atis_frequency integer,
  awos_frequency integer,
  asos_frequency integer,
  unicom_frequency integer,
  is_closed integer not null,
  is_military integer not null,
  is_addon integer not null,
  num_com integer not null,
  num_parking_gate integer not null,
  num_parking_ga_ramp integer not null,
  num_parking_cargo integer not null,
  num_parking_mil_cargo integer not null,
  num_parking_mil_combat integer not null,
  num_approach integer not null,
  num_runway_hard integer not null,
  num_runway_soft integer not null,
  num_runway_water integer not null,
  num_runway_light integer not null,
  num_runway_end_closed integer not null,
  num_runway_end_vasi integer not null,
  num_runway_end_als integer not null,
  num_runway_end_ils integer,
  num_apron integer not null,
  num_taxi_path integer not null,
  num_helipad integer not null,
  num_jetway integer not null,
  num_starts integer not null,
  longest_runway_length integer not null,
  longest_runway_width integer not null,
  longest_runway_heading double not null,
  longest_runway_surface varchar(15),
  num_runways integer not null,
  largest_parking_ramp varchar(20),
  largest_parking_gate varchar(20),
  rating integer not null,
  is_3d integer not null,
  scenery_local_path varchar(250) collate nocase,
  bgl_filename varchar(300) collate nocase,
  left_lonx double not null,
  top_laty double not null,
  right_lonx double not null,
  bottom_laty double not null,
  mag_var double not null,
  tower_altitude integer,
  tower_lonx double,
  tower_laty double,
  transition_altitude double,
  transition_level double,
  altitude integer not null,
  lonx double not null,
  laty double not null,
  foreign key(file_id) references bgl_file(bgl_file_id)
);
CREATE TABLE runway_end (
  runway_end_id integer primary key,
  name varchar(10) not null,
  end_type varchar(10),
  offset_threshold integer,
  blast_pad integer,
  overrun integer,
  left_vasi_type varchar(10),
  left_vasi_pitch double,
  right_vasi_type varchar(10),
  right_vasi_pitch double,
  has_closed_markings integer not null,
  has_stol_markings integer not null,
  is_takeoff integer not null,
  is_landing integer not null,
  is_pattern integer not null,
  app_light_system_type varchar(15),
  has_end_lights integer not null,
  has_reils integer not null,
  has_touchdown_lights integer not null,
  num_strobes integer,
  ils_ident varchar(10),
  heading double not null,
  altitude integer,
  lonx double not null,
  laty double not null
);
CREATE TABLE runway (
  runway_id integer primary key,
  airport_id integer not null,
  primary_end_id integer not null,
  secondary_end_id integer not null,
  surface varchar(15),
  smoothness double,
  shoulder varchar(15),
  length double not null,
  width double not null,
  heading double not null,
  pattern_altitude integer,
  marking_flags integer not null,
  edge_light varchar(15),
  center_light varchar(15),
  has_center_red integer not null,
  primary_lonx double not null,
  primary_laty double not null,
  secondary_lonx double not null,
  secondary_laty double not null,
  altitude integer,
  lonx double not null,
  laty double not null,
  foreign key(airport_id) references airport(airport_id),
  foreign key(primary_end_id) references runway_end(runway_end_id),
  foreign key(secondary_end_id) references runway_end(runway_end_id)
);
CREATE TABLE waypoint (
  waypoint_id integer primary key,
  file_id integer not null,
  nav_id integer,
  ident varchar(5) not null,
  name varchar(50),
  region varchar(2),
  airport_id integer,
  airport_ident varchar(4),
  artificial integer,
  type varchar(15),
  arinc_type varchar(4),
  num_victor_airway integer not null,
  num_jet_airway integer not null,
  mag_var double not null,
  lonx double not null,
  laty double not null,
  foreign key(file_id) references bgl_file(bgl_file_id)
);
CREATE TABLE vor (
  vor_id integer primary key,
  file_id integer not null,
  ident varchar(5),
  name varchar(50),
  region varchar(2),
  airport_id integer,
  airport_ident varchar(4),
  type varchar(15),
  frequency integer,
  channel varchar(5),
  range integer,
  mag_var double,
  dme_only integer not null,
  dme_altitude integer,
  dme_lonx double,
  dme_laty double,
  altitude integer,
  lonx double not null,
  laty double not null,
  foreign key(file_id) references bgl_file(bgl_file_id)
);
CREATE TABLE ndb (
  ndb_id integer primary key,
  file_id integer not null,
  ident varchar(5),
  name varchar(50),
  region varchar(2),
  airport_id integer,
  airport_ident varchar(4),
  type varchar(15),
  frequency integer not null,
  range integer,
  mag_var double not null,
  altitude integer,
  lonx double not null,
  laty double not null,
  foreign key(file_id) references bgl_file(bgl_file_id)
);
CREATE TABLE ils (
  ils_id integer primary key,
  ident varchar(5),
  name varchar(50),
  region varchar(2),
  type varchar(1),
  perf_indicator varchar(10),
  provider varchar(10),
  frequency integer,
  range integer,
  mag_var double not null,
  has_backcourse integer not null,
  dme_range integer,
  dme_altitude integer,
  dme_lonx double,
  dme_laty double,
  gs_range integer,
  gs_pitch double,
  gs_altitude integer,
  gs_lonx double,
  gs_laty double,
  loc_runway_end_id integer,
  loc_airport_ident varchar(4),
  loc_runway_name varchar(10),
  loc_heading double,
  loc_width double,
  end1_lonx double,
  end1_laty double,
  end_mid_lonx double,
  end_mid_laty double,
  end2_lonx double,
  end2_laty double,
  altitude integer not null,
  lonx double not null,
  laty double not null
);
CREATE TABLE airway (
  airway_id integer primary key,
  airway_name varchar(5) not null,
  airway_type varchar(15) not null,
  route_type varchar(5),
  airway_fragment_no integer not null,
  sequence_no integer not null,
  from_waypoint_id integer not null,
  to_waypoint_id integer not null,
  direction varchar(1),
  minimum_altitude integer,
  maximum_altitude integer,
  left_lonx double not null,
  top_laty double not null,
  right_lonx double not null,
  bottom_laty double not null,
  from_lonx double,
  from_laty double,
  to_lonx double,
  to_laty double,
  foreign key(from_waypoint_id) references waypoint(waypoint_id),
  foreign key(to_waypoint_id) references waypoint(waypoint_id)
);
CREATE TABLE approach (
  approach_id integer primary key,
  airport_id integer,
  runway_end_id integer,
  arinc_name varchar(6),
  airport_ident varchar(4),
  runway_name varchar(10),
  type varchar(25) not null,
  suffix varchar(1),
  has_gps_overlay integer not null,
  has_vertical_angle integer,
  has_rnp integer,
  fix_type varchar(25),
  fix_ident varchar(5),
  fix_region varchar(2),
  fix_airport_ident varchar(4),
  aircraft_category varchar(4),
  altitude integer,
  heading double,
  missed_altitude integer,
  foreign key(airport_id) references airport(airport_id),
  foreign key(runway_end_id) references runway_end(runway_end_id)
);
CREATE TABLE approach_leg (
  approach_leg_id integer primary key,
  approach_id integer not null,
  is_missed integer not null,
  type varchar(10),
  arinc_descr_code varchar(25),
  approach_fix_type varchar(1),
  alt_descriptor varchar(10),
  turn_direction varchar(10),
  rnp double,
  fix_type varchar(25),
  fix_ident varchar(5),
  fix_region varchar(2),
  fix_airport_ident varchar(4),
  fix_lonx double,
  fix_laty double,
  recommended_fix_type varchar(25),
  recommended_fix_ident varchar(5),
  recommended_fix_region varchar(2),
  recommended_fix_lonx double,
  recommended_fix_laty double,
  is_flyover integer not null,
  is_true_course integer not null,
  course double,
  distance double,
  time double,
  theta double,
  rho double,
  altitude1 double,
  altitude2 double,
  speed_limit_type varchar(2),
  speed_limit integer,
  vertical_angle double,
  foreign key(approach_id) references approach(approach_id)
);
CREATE TABLE transition (
  transition_id integer primary key,
  approach_id integer not null,
  type varchar(25) not null,
  fix_type varchar(25),
  fix_ident varchar(5),
  fix_region varchar(2),
  fix_airport_ident varchar(4),
  aircraft_category varchar(4),
  altitude integer,
  dme_ident varchar(5),
  dme_region varchar(2),
  dme_airport_ident varchar(5),
  dme_radial double,
  dme_distance integer,
  foreign key(approach_id) references approach(approach_id)
);
CREATE TABLE transition_leg (
  transition_leg_id integer primary key,
  transition_id integer not null,
  type varchar(10) not null,
  arinc_descr_code varchar(25),
  approach_fix_type varchar(1),
  alt_descriptor varchar(10),
  turn_direction varchar(10),
  rnp double,
  fix_type varchar(25),
  fix_ident varchar(5),
  fix_region varchar(2),
  fix_airport_ident varchar(4),
  fix_lonx double,
  fix_laty double,
  recommended_fix_type varchar(25),
  recommended_fix_ident varchar(5),
  recommended_fix_region varchar(2),
  recommended_fix_lonx double,
  recommended_fix_laty double,
  is_flyover integer not null,
  is_true_course integer not null,
  course double,
  distance double,
  time double,
  theta double,
  rho double,
  altitude1 double,
  altitude2 double,
  speed_limit_type varchar(2),
  speed_limit integer,
  vertical_angle double,
  foreign key(transition_id) references transition(transition_id)
);
CREATE TABLE apron (
  apron_id integer primary key,
  airport_id integer not null,
  surface varchar(15),
  is_draw_surface integer not null,
  is_draw_detail integer not null,
  vertices blob,
  vertices2 blob,
  triangles blob,
  geometry blob,
  foreign key(airport_id) references airport(airport_id)
);
CREATE TABLE parking (
  parking_id integer primary key,
  airport_id integer not null,
  type varchar(20),
  pushback varchar(5),
  name varchar(15),
  number integer not null,
  suffix varchar(5),
  airline_codes text,
  radius double,
  heading double,
  has_jetway integer not null,
  lonx double not null,
  laty double not null,
  foreign key(airport_id) references airport(airport_id)
);
CREATE TABLE start (
  start_id integer primary key,
  airport_id integer not null,
  runway_end_id integer,
  runway_name varchar(10),
  type varchar(10),
  heading double not null,
  number integer,
  altitude integer not null,
  lonx double not null,
  laty double not null,
  foreign key(airport_id) references airport(airport_id),
  foreign key(runway_end_id) references runway_end(runway_end_id)
);
CREATE TABLE helipad (
  helipad_id integer primary key,
  airport_id integer not null,
  start_id integer,
  surface varchar(15),
  type varchar(10),
  length double not null,
  width double not null,
  heading double not null,
  is_transparent integer not null,
  is_closed integer not null,
  altitude integer not null,
  lonx double not null,
  laty double not null,
  foreign key(airport_id) references airport(airport_id),
  foreign key(start_id) references start(start_id)
);
CREATE TABLE taxi_path (
  taxi_path_id integer primary key,
  airport_id integer not null,
  type varchar(15),
  surface varchar(15),
  width double not null,
  name varchar(20),
  is_draw_surface integer not null,
  is_draw_detail integer not null,
  start_type varchar(15),
  start_dir varchar(15),
  start_lonx double not null,
  start_laty double not null,
  end_type varchar(15),
  end_dir varchar(15),
  end_lonx double not null,
  end_laty double not null,
  foreign key(airport_id) references airport(airport_id)
);
CREATE TABLE com (
  com_id integer primary key,
  airport_id integer not null,
  type varchar(30),
  frequency integer not null,
  name varchar(50),
  foreign key(airport_id) references airport(airport_id)
);
CREATE TABLE airport_file (
  airport_file_id integer primary key,
  file_id integer not null,
  ident varchar(4) not null,
  foreign key(file_id) references bgl_file(bgl_file_id)
);
CREATE TABLE airport_msa (
  airport_msa_id integer primary key,
  file_id integer not null,
  airport_id integer,
  airport_ident varchar(5),
  nav_id integer,
  nav_ident varchar(5),
  nav_type varchar(15),
  vor_type varchar(2),
  vor_dme_only integer,
  vor_has_dme integer,
  region varchar(2),
  multiple_code varchar(1),
  true_bearing integer,
  mag_var double,
  left_lonx double,
  top_laty double,
  right_lonx double,
  bottom_laty double,
  radius double not null,
  lonx double not null,
  laty double not null,
  geometry blob,
  foreign key(airport_id) references airport(airport_id)
);
CREATE TABLE holding (
  holding_id integer primary key,
  file_id integer not null,
  airport_id integer,
  airport_ident varchar(5),
  nav_id integer,
  nav_ident varchar(5),
  nav_type varchar(2),
  vor_type varchar(2),
  vor_dme_only integer,
  vor_has_dme integer,
  name varchar(50),
  region varchar(2),
  mag_var double,
  course double not null,
  turn_direction varchar(1) not null,
  leg_length double,
  leg_time double,
  minimum_altitude double,
  maximum_altitude double,
  speed_limit integer,
  lonx double not null,
  laty double not null,
  foreign key(airport_id) references airport(airport_id)
);
CREATE TABLE marker (
  marker_id integer primary key,
  file_id integer not null,
  ident varchar(5),
  region varchar(2),
  type varchar(15),
  heading double not null,
  altitude integer not null,
  lonx double not null,
  laty double not null,
  foreign key(file_id) references bgl_file(bgl_file_id)
);
CREATE TABLE mora_grid (
  mora_grid_id integer primary key,
  file_id integer not null,
  version integer not null,
  lonx_columns integer not null,
  laty_rows integer not null,
  geometry blob not null
);
CREATE TABLE nav_search (
  nav_search_id integer primary key,
  waypoint_id integer,
  waypoint_nav_id integer,
  vor_id integer,
  ndb_id integer,
  file_id integer not null,
  airport_id integer,
  airport_ident varchar(4),
  ident varchar(5),
  name varchar(50) collate nocase,
  region varchar(2),
  range integer,
  type varchar(15),
  nav_type varchar(15),
  arinc_type varchar(4),
  frequency integer,
  channel varchar(10),
  waypoint_num_victor_airway integer,
  waypoint_num_jet_airway integer,
  scenery_local_path varchar(250) collate nocase,
  bgl_filename varchar(300) collate nocase,
  mag_var double not null,
  altitude integer,
  lonx double not null,
  laty double not null,
  foreign key(waypoint_id) references waypoint(waypoint_id),
  foreign key(vor_id) references vor(vor_id),
  foreign key(ndb_id) references ndb(ndb_id),
  foreign key(file_id) references bgl_file(bgl_file_id),
  foreign key(airport_id) references airport(airport_id)
);
CREATE TABLE boundary (
  boundary_id integer primary key,
  file_id integer not null,
  type varchar(15),
  name varchar(250),
  description varchar(250),
  restrictive_designation varchar(20),
  restrictive_type varchar(20),
  multiple_code varchar(5),
  time_code varchar(5),
  com_type varchar(30),
  com_frequency integer,
  com_name varchar(50),
  min_altitude_type varchar(15),
  max_altitude_type varchar(15),
  min_altitude integer,
  max_altitude integer,
  max_lonx double not null,
  max_laty double not null,
  min_lonx double not null,
  min_laty double not null,
  geometry blob,
  foreign key(file_id) references bgl_file(bgl_file_id)
);
CREATE TABLE scenery_area (
  scenery_area_id integer primary key,
  number integer not null,
  layer integer not null,
  title varchar(250) not null,
  remote_path varchar(250),
  local_path varchar(250),
  active integer not null,
  required integer not null,
  exclude varchar(50)
);
CREATE TABLE script (
  script_id integer primary key,
  statement varchar(4096)
);
CREATE TABLE translation (
  translation_id integer primary key,
  language varchar(50) not null,
  key varchar(250) not null,
  text varchar(250) not null collate nocase
);
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
CREATE INDEX IF NOT EXISTS idx_airport_ident ON airport(ident);
CREATE INDEX IF NOT EXISTS idx_waypoint_ident ON waypoint(ident);
CREATE INDEX IF NOT EXISTS idx_vor_ident ON vor(ident);
CREATE INDEX IF NOT EXISTS idx_ndb_ident ON ndb(ident);
CREATE INDEX IF NOT EXISTS idx_ils_ident ON ils(ident);
CREATE INDEX IF NOT EXISTS idx_airway_name ON airway(airway_name);
CREATE INDEX IF NOT EXISTS idx_approach_airport ON approach(airport_id);
CREATE INDEX IF NOT EXISTS idx_approach_leg_app ON approach_leg(approach_id);
CREATE INDEX IF NOT EXISTS idx_transition_app ON transition(approach_id);
CREATE INDEX IF NOT EXISTS idx_transition_leg_trans ON transition_leg(transition_id);
"#,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use openairac_model::{
        AirportId, CanonicalAirport, CanonicalProcedureLeg, CanonicalRunway, ProcedureLegId,
        RunwayId, SourceSnapshot, SourceSnapshotId,
    };
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
        let mut store = WorldStore::open(dir.join("src.sqlite")).unwrap();
        let t = Utc::now();
        store
            .insert_source_snapshot(&SourceSnapshot {
                id: SourceSnapshotId("snap-1".to_string()),
                provider: "FAA_CIFP".to_string(),
                dataset: "FAACIFP18".to_string(),
                provider_revision: Some("2608".to_string()),
                airac_cycle: Some("2608".to_string()),
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
                id: AirportId("faa:KSFO".to_string()),
                ident: "KSFO".to_string(),
                name: "San Francisco Intl".to_string(),
                airport_type: "large_airport".to_string(),
                latitude: 37.6188,
                longitude: -122.375,
                elevation_ft: Some(13.0),
                iso_country: Some("US".to_string()),
                municipality: Some("San Francisco".to_string()),
                runways: vec![CanonicalRunway {
                    id: RunwayId("faa:KSFO:28R".to_string()),
                    airport_id: AirportId("faa:KSFO".to_string()),
                    airport_ident: "KSFO".to_string(),
                    le_ident: "28R".to_string(),
                    he_ident: "10L".to_string(),
                    length_ft: 11870,
                    width_ft: Some(200),
                    surface: Some("ASP".to_string()),
                    le_lat: 37.6188,
                    le_lon: -122.3750,
                    he_lat: 37.6140,
                    he_lon: -122.3900,
                    le_elevation_ft: Some(13.0),
                    he_elevation_ft: Some(11.0),
                    true_heading_deg: Some(284.0),
                    computed_magnetic_designator: Some("28R".to_string()),
                    official_designator: "28R/10L".to_string(),
                    temporal: TemporalValidity {
                        valid_from: t,
                        valid_until: None,
                        source_snapshot_id: SourceSnapshotId("snap-1".to_string()),
                    },
                }],
                temporal: TemporalValidity {
                    valid_from: t,
                    valid_until: None,
                    source_snapshot_id: SourceSnapshotId("snap-1".to_string()),
                },
            })
            .unwrap();

        // Insert procedure legs for SID (GAP3) and Approach (I28R)
        store
            .transact(|conn| {
                openairac_store::insert_procedure_leg_conn(
                    conn,
                    &CanonicalProcedureLeg {
                        object_id: ProcedureLegId("leg-1".to_string()),
                        airport_ident: "KSFO".to_string(),
                        icao_code: "K2".to_string(),
                        procedure_kind: 'D',
                        procedure_ident: "GAP3".to_string(),
                        route_type: "4".to_string(),
                        transition_ident: "".to_string(),
                        sequence_number: 10,
                        fix_ident: "GAP".to_string(),
                        fix_icao_code: "K2".to_string(),
                        fix_section: "EA".to_string(),
                        waypoint_description: "".to_string(),
                        turn_direction: None,
                        rnp_nm: None,
                        path_terminator: "CF".to_string(),
                        recommended_navaid: None,
                        arc_radius_nm: None,
                        course_a_deg: Some(284.0),
                        distance_a_nm: Some(15.0),
                        course_b_deg: None,
                        distance_b_nm: None,
                        altitude_descriptor: Some('+'),
                        altitude_1_ft: Some(5000),
                        altitude_2_ft: None,
                        speed_limit_kts: Some(250),
                        course_c_deg: None,
                        vertical_angle_deg: None,
                        msa_center_fix: None,
                        route_qualifiers: "".to_string(),
                        raw: "RAW_SID_LEG".to_string(),
                        temporal: TemporalValidity {
                            valid_from: t,
                            valid_until: None,
                            source_snapshot_id: SourceSnapshotId("snap-1".to_string()),
                        },
                    },
                )?;

                openairac_store::insert_procedure_leg_conn(
                    conn,
                    &CanonicalProcedureLeg {
                        object_id: ProcedureLegId("leg-2".to_string()),
                        airport_ident: "KSFO".to_string(),
                        icao_code: "K2".to_string(),
                        procedure_kind: 'F',
                        procedure_ident: "I28R".to_string(),
                        route_type: "A".to_string(),
                        transition_ident: "".to_string(),
                        sequence_number: 10,
                        fix_ident: "AXMUL".to_string(),
                        fix_icao_code: "K2".to_string(),
                        fix_section: "EA".to_string(),
                        waypoint_description: "".to_string(),
                        turn_direction: None,
                        rnp_nm: None,
                        path_terminator: "IF".to_string(),
                        recommended_navaid: None,
                        arc_radius_nm: None,
                        course_a_deg: Some(284.0),
                        distance_a_nm: None,
                        course_b_deg: None,
                        distance_b_nm: None,
                        altitude_descriptor: Some('+'),
                        altitude_1_ft: Some(4000),
                        altitude_2_ft: None,
                        speed_limit_kts: None,
                        course_c_deg: None,
                        vertical_angle_deg: None,
                        msa_center_fix: None,
                        route_qualifiers: "".to_string(),
                        raw: "RAW_APP_LEG".to_string(),
                        temporal: TemporalValidity {
                            valid_from: t,
                            valid_until: None,
                            source_snapshot_id: SourceSnapshotId("snap-1".to_string()),
                        },
                    },
                )?;
                Ok(openairac_store::EntityWrite::Created)
            })
            .unwrap();

        (store, dir)
    }

    #[test]
    fn test_lnm_export_creates_schema_valid_db_and_procedures() {
        let (store, dir) = fixture_store();
        let out = dir.join("lnm");
        let set = LnmNavdataExporter.export(&store, Utc::now(), &out).unwrap();
        assert_eq!(set.family.as_str(), "little-navmap-sqlite");
        set.verify(&out).unwrap();

        // Check primary DB
        let conn = rusqlite::Connection::open(out.join("openairac.sqlite")).unwrap();
        let airport_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM airport", [], |r| r.get(0))
            .unwrap();
        assert_eq!(airport_count, 1);

        let rwy_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM runway", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rwy_count, 1);

        let rwy_end_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM runway_end", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rwy_end_count, 2);

        let app_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM approach", [], |r| r.get(0))
            .unwrap();
        assert_eq!(app_count, 2); // 1 SID + 1 Approach

        let app_leg_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM approach_leg", [], |r| r.get(0))
            .unwrap();
        assert_eq!(app_leg_count, 2);

        // Verify legacy db exists
        assert!(out.join("little_navmap_openairac.db").exists());
    }
}
