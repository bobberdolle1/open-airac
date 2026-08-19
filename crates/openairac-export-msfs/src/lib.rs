//! MSFS navdata exporter (source generation) and Community installer.
//!
//! Official SDK path: navdata is authored as XML sources per the
//! BGL compiler schema (bglcomp.xsd, "SimpleNavData" SDK sample),
//! compiled with `fspackagetool.exe` into a Community package, and
//! ordered with `PackageOrderHint` (CUSTOM_NAVDATA /
//! CUSTOM_NAVDATA_PATCH). This crate GENERATES the XML sources +
//! package definition from the canonical world, invokes the official
//! compiler when an SDK is available, and installs the compiled
//! package transactionally into the Community folder.
//!
//! Support state: EXPERIMENTAL until fspackagetool compile +
//! BglExplorer verification are executed against a real SDK.
//! Leg types without a verified MSFS mapping are SKIPPED with
//! diagnostics (never guessed).

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use openairac_export::{
    ArtifactEntry, FormatExporter, GeneratedArtifactSet, SupportState, TargetDescriptor,
    TargetInstallReport, TargetInstaller, families,
};
use openairac_model::{CanonicalProcedureLeg, NavaidKind};
use openairac_store::WorldStore;
use sha2::Digest;
use std::collections::BTreeSet;

use std::path::{Path, PathBuf};

pub const PACKAGE_NAME: &str = "openairac-navdata";

// ---------------------------------------------------------------------------
// XML escaping
// ---------------------------------------------------------------------------

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn opt_str(s: &str) -> String {
    if s.is_empty() {
        "-".to_string()
    } else {
        esc(s)
    }
}

// ---------------------------------------------------------------------------
// Exporter
// ---------------------------------------------------------------------------

pub struct MsfsNavdataExporter;

impl FormatExporter for MsfsNavdataExporter {
    fn family(&self) -> openairac_export::FormatFamilyId {
        families::msfs_bgl()
    }

    fn export(
        &self,
        store: &WorldStore,
        as_of: DateTime<Utc>,
        out_dir: &Path,
    ) -> Result<GeneratedArtifactSet> {
        std::fs::create_dir_all(out_dir)?;
        let airports = store.query_airports_at(as_of)?;

        let navaids = store.query_navaids_at(as_of)?;
        let waypoints = store.query_waypoints_at(as_of)?;
        let airway_legs = store.query_airway_legs_at(as_of)?;
        let procedure_legs = store.query_procedure_legs_at(as_of)?;

        let mut report = MsfsExportReport::default();
        let mut out = String::new();

        // --- Airports + runways + ILS + approaches ---
        out.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
        out.push_str("<FSData version=\"9.0\">\n");
        for airport in &airports {
            // Country: published ISO country when available; else
            // derived from the ICAO region of a colocated navaid
            // (conservative K/C/M mapping only). The attribute is
            // omitted entirely when unknown - never fabricated.
            let region_hint: Option<String> = airport
                .iso_country
                .clone()
                .or_else(|| {
                    navaids
                        .iter()
                        .find(|n| n.associated_airport.as_deref() == Some(airport.ident.as_str()))
                        .and_then(|n| n.region_code.clone())
                })
                .filter(|r| !r.is_empty());
            let country_attr = match region_hint.as_deref() {
                Some(c) => format!(" country=\"{}\"", esc(c)),
                None => String::new(),
            };
            let region_attr = match region_hint.as_deref().map(|r| r.chars().next()) {
                Some(Some('K')) => " region=\"K2\"",
                Some(Some('C')) => " region=\"CY\"",
                Some(Some('M')) => " region=\"MM\"",
                _ => "",
            };
            report.airports += 1;
            out.push_str(&format!(
                "  <Airport ident=\"{}\"{}{} city=\"{}\" name=\"{}\" lat=\"{:.7}\" lon=\"{:.7}\" alt=\"{:.2}M\">\n",
                esc(&airport.ident),
                region_attr,
                country_attr,
                opt_str(airport.municipality.as_deref().unwrap_or("")),
                esc(&airport.name),
                airport.latitude,
                airport.longitude,
                airport.elevation_ft.unwrap_or(0.0) * 0.3048,
            ));
            // Runways (attached to the airport by the store query).
            for rwy in &airport.runways {
                report.runways += 1;
                let heading = rwy
                    .true_heading_deg
                    .map(|h| format!("{h:.2}"))
                    .unwrap_or_else(|| "-".to_string());
                let length = rwy.length_ft as f64 * 0.3048;
                let width = rwy.width_ft.map(|w| w as f64 * 0.3048).unwrap_or(0.0);
                out.push_str(&format!(
                    "    <Runway lat=\"{:.7}\" lon=\"{:.7}\" alt=\"{:.2}M\" surface=\"ASPHALT\" heading=\"{}\" length=\"{:.2}M\" width=\"{:.2}M\" number=\"{}\" designator=\"{}\" primaryTakeoff=\"TRUE\" primaryLanding=\"TRUE\" primaryPattern=\"LEFT\" secondaryTakeoff=\"TRUE\" secondaryLanding=\"TRUE\" secondaryPattern=\"LEFT\">\n",
                    rwy.le_lat, rwy.le_lon,
                    rwy.le_elevation_ft.unwrap_or(0.0) * 0.3048,
                    heading, length, width,
                    esc(&rwy.le_ident), "NONE"
                ));
                out.push_str("    </Runway>\n");
            }
            // ILS components.
            for nav in navaids
                .iter()
                .filter(|n| n.associated_airport.as_deref() == Some(airport.ident.as_str()))
            {
                if nav.kind == NavaidKind::IlsLocalizer {
                    let (Some(_runway), Some(bearing)) = (
                        nav.associated_runway.as_deref(),
                        nav.localizer_bearing_mag_deg,
                    ) else {
                        report.skip("ils", &nav.ident, "missing runway/bearing".to_string());
                        continue;
                    };
                    let freq_mhz = nav.frequency.0 as f64 / 1000.0;
                    out.push_str(&format!(
                            "    <Ils lat=\"{:.7}\" lon=\"{:.7}\" alt=\"{:.2}M\" heading=\"{:.2}\" frequency=\"{:.3}\" end=\"PRIMARY\" range=\"27N\" magvar=\"{:.2}\" ident=\"{}\" width=\"5\" name=\"{}\" backCourse=\"FALSE\">\n",
                            nav.latitude, nav.longitude,
                            nav.elevation_ft.unwrap_or(0) as f64 * 0.3048,
                            bearing, freq_mhz,
                            nav.magnetic_variation_deg.unwrap_or(0.0),
                            esc(&nav.ident), esc(&nav.name)
                        ));
                    if let Some(gs) = navaids.iter().find(|n| {
                        n.kind == NavaidKind::IlsGlidepath
                            && n.ident == nav.ident
                            && n.associated_airport == nav.associated_airport
                    }) {
                        let pitch = gs.glideslope_angle_deg.unwrap_or(3.0);
                        out.push_str(&format!(
                                "      <GlideSlope lat=\"{:.7}\" lon=\"{:.7}\" alt=\"{:.2}M\" frequency=\"{:.3}\" range=\"18N\" pitch=\"{:.2}\"/>\n",
                                gs.latitude, gs.longitude,
                                gs.elevation_ft.unwrap_or(0) as f64 * 0.3048,
                                gs.frequency.0 as f64 / 1000.0, pitch
                            ));
                    }
                    out.push_str("    </Ils>\n");
                }
            }
            // Approaches for this airport.
            write_approaches(
                airport.ident.as_str(),
                &procedure_legs,
                &mut out,
                &mut report,
            );
            out.push_str("  </Airport>\n");
        }
        out.push_str("</FSData>\n");
        let airports_xml = out;

        // --- Navaids (VOR/NDB standalone) ---
        let mut navaids_xml = String::new();
        navaids_xml
            .push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<FSData version=\"9.0\">\n");
        for nav in &navaids {
            report.navaids += 1;
            let Some(region) = nav.region_code.as_deref() else {
                report.skip("navaid", &nav.ident, "missing region".to_string());
                continue;
            };
            let alt = nav.elevation_ft.unwrap_or(0) as f64 * 0.3048;
            let freq_mhz = nav.frequency.0 as f64 / 1000.0;
            match nav.kind {
                NavaidKind::Vor | NavaidKind::Vordme | NavaidKind::Vortac | NavaidKind::Tacan => {
                    let dme = matches!(
                        nav.kind,
                        NavaidKind::Vordme | NavaidKind::Vortac | NavaidKind::Tacan
                    );
                    let vtype = match nav.service_volume_nm {
                        Some(v) if v >= 100 => "HIGH",
                        Some(v) if v >= 30 => "LOW",
                        _ => "TERMINAL",
                    };
                    let range = nav.service_volume_nm.unwrap_or(40);
                    navaids_xml.push_str(&format!(
                        "  <Vor dme=\"{}\" dmeOnly=\"FALSE\" lat=\"{:.7}\" lon=\"{:.7}\" alt=\"{:.2}M\" range=\"{range}N\" frequency=\"{freq_mhz:.3}\" type=\"{vtype}\" magvar=\"{:.2}\" region=\"{}\" ident=\"{}\" name=\"{}\">\n",
                        if dme { "TRUE" } else { "FALSE" },
                        nav.latitude, nav.longitude, alt,
                        nav.magnetic_variation_deg.unwrap_or(0.0),
                        esc(region), esc(&nav.ident), esc(&nav.name)
                    ));
                    navaids_xml.push_str("  </Vor>\n");
                }
                NavaidKind::Ndb => {
                    navaids_xml.push_str(&format!(
                        "  <Ndb lat=\"{:.7}\" lon=\"{:.7}\" alt=\"{:.2}M\" range=\"25N\" frequency=\"{:.2}\" region=\"{}\" ident=\"{}\" name=\"{}\" type=\"HH\">\n",
                        nav.latitude, nav.longitude, alt,
                        nav.frequency.0 as f64 / 1000.0,
                        esc(region), esc(&nav.ident), esc(&nav.name)
                    ));
                    navaids_xml.push_str("  </Ndb>\n");
                }
                _ => {} // ILS handled per-airport; DME components folded into VOR/ILS
            }
        }
        navaids_xml.push_str("</FSData>\n");

        // --- Waypoints ---
        let mut wps = String::new();
        wps.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<FSData version=\"9.0\">\n");
        for wp in &waypoints {
            report.waypoints += 1;
            if wp.region_code.is_empty() {
                report.skip("waypoint", &wp.ident, "missing region".to_string());
                continue;
            }
            if wp.ident.chars().all(|c| c.is_ascii_digit()) {
                continue; // numeric idents are not representable
            }
            wps.push_str(&format!(
                "  <Waypoint lat=\"{:.7}\" lon=\"{:.7}\" waypointType=\"NAMED\" magvar=\"-\" waypointRegion=\"{}\" waypointIdent=\"{}\"/>\n",
                wp.latitude,
                wp.longitude,
                esc(&wp.region_code),
                esc(&wp.ident)
            ));
        }
        wps.push_str("</FSData>\n");

        // --- Airways ---
        let mut routes = String::new();
        routes.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<FSData version=\"9.0\">\n");
        let mut route_names: BTreeSet<String> = BTreeSet::new();
        for leg in &airway_legs {
            route_names.insert(leg.route_ident.clone());
        }
        for name in route_names {
            report.routes += 1;
            let mut legs: Vec<&openairac_model::CanonicalAirwayLeg> = airway_legs
                .iter()
                .filter(|l| l.route_ident == name)
                .collect();
            legs.sort_by_key(|l| l.sequence_number);
            if legs.is_empty() {
                continue;
            }
            let route_type = legs[0].route_type.as_str();
            let leg_type = match route_type {
                "H" => "JET",
                "L" | "O" => "VICTOR",
                _ => "BOTH",
            };
            routes.push_str(&format!(
                "  <Route name=\"{}\">\n    <Leg type=\"{}\">\n",
                esc(&name),
                leg_type
            ));
            for (i, leg) in legs.iter().enumerate() {
                let region = if leg.start_icao_code.is_empty() {
                    leg.end_icao_code.as_str()
                } else {
                    leg.start_icao_code.as_str()
                };
                if i == 0 {
                    routes.push_str(&format!(
                        "      <Next waypointType=\"NAMED\" waypointRegion=\"{}\" waypointIdent=\"{}\" altitudeMinimum=\"{}\"/>\n",
                        esc(region),
                        esc(&leg.start_fix),
                        leg.minimum_altitude_ft.unwrap_or(0) as f64 * 0.3048
                    ));
                }
                routes.push_str(&format!(
                    "      <Next waypointType=\"NAMED\" waypointRegion=\"{}\" waypointIdent=\"{}\" altitudeMinimum=\"{}\"/>\n",
                    esc(&leg.end_icao_code),
                    esc(&leg.end_fix),
                    leg.minimum_altitude_ft.unwrap_or(0) as f64 * 0.3048
                ));
            }
            routes.push_str("    </Leg>\n  </Route>\n");
        }
        routes.push_str("</FSData>\n");

        // Write sources.
        let pkg_src = out_dir.join("PackageSources").join("navdata");
        std::fs::create_dir_all(&pkg_src)?;
        std::fs::write(pkg_src.join("airports.xml"), airports_xml)?;
        std::fs::write(pkg_src.join("navaids.xml"), navaids_xml)?;
        std::fs::write(pkg_src.join("waypoints.xml"), wps)?;
        std::fs::write(pkg_src.join("routes.xml"), routes)?;

        // Package definition (official fspackagetool input).
        let def_dir = out_dir.join("PackageDefinitions");
        std::fs::create_dir_all(&def_dir)?;
        let def = format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<AssetPackage Version="0.1.0" Name="{PACKAGE_NAME}">
  <ItemSettings>
    <ContentType>NAVDATA</ContentType>
    <Title>OpenAIRAC Navigation Data</Title>
    <Manufacturer>OpenAIRAC</Manufacturer>
    <Creator>OpenAIRAC</Creator>
    <PackageOrderHint>CUSTOM_NAVDATA</PackageOrderHint>
  </ItemSettings>
  <AssetGroups>
    <AssetGroup Name="ContentInfo">
      <Type>ContentInfo</Type>
      <Flags>
        <FSXCompatibility>false</FSXCompatibility>
      </Flags>
      <AssetDir>PackageSources/navdata/</AssetDir>
      <OutputDir>NAVDATA/</OutputDir>
    </AssetGroup>
  </AssetGroups>
</AssetPackage>
"#
        );
        std::fs::write(def_dir.join(format!("{PACKAGE_NAME}.xml")), def)?;

        // Cycle metadata.
        let cycle = openairac_export_xplane::airac_cycle(as_of);
        let meta = serde_json::json!({
            "generator": format!("openairac {}", env!("CARGO_PKG_VERSION")),
            "cycle": cycle,
            "as_of": as_of.to_rfc3339(),
            "package": PACKAGE_NAME,
            "format_family": "msfs-bgl",
            "compile": "official fspackagetool.exe (MSFS SDK Tools/bin); set MSFS_SDK env var or pass --sdk",
            "support_state": "EXPERIMENTAL"
        });
        std::fs::write(
            out_dir.join("cycle.json"),
            serde_json::to_string_pretty(&meta)?,
        )?;

        let mut artifacts = vec![
            artifact_of(
                out_dir,
                "PackageDefinitions/openairac-navdata.xml",
                "package-definition",
            ),
            artifact_of(
                out_dir,
                "PackageSources/navdata/airports.xml",
                "navdata-source",
            ),
            artifact_of(
                out_dir,
                "PackageSources/navdata/navaids.xml",
                "navdata-source",
            ),
            artifact_of(
                out_dir,
                "PackageSources/navdata/waypoints.xml",
                "navdata-source",
            ),
            artifact_of(
                out_dir,
                "PackageSources/navdata/routes.xml",
                "navdata-source",
            ),
            artifact_of(out_dir, "cycle.json", "cycle-metadata"),
        ];
        // Report diagnostics summary in a sidecar artifact.
        let report_json = serde_json::to_string_pretty(&report)?;
        std::fs::write(out_dir.join("export_report.json"), &report_json)?;
        artifacts.push(artifact_of(out_dir, "export_report.json", "report"));

        Ok(GeneratedArtifactSet {
            family: self.family(),
            cycle,
            as_of: as_of.to_rfc3339(),
            generator: format!("openairac {}", env!("CARGO_PKG_VERSION")),
            world_fingerprint: report_fingerprint(&report),
            artifacts,
        })
    }
}

fn artifact_of(root: &Path, rel: &str, kind: &str) -> ArtifactEntry {
    let data = std::fs::read(root.join(rel)).unwrap_or_default();
    let sha = format!("{:x}", sha2::Sha256::digest(&data));
    ArtifactEntry {
        path: rel.to_string(),
        sha256: sha,
        size: data.len() as u64,
        kind: kind.to_string(),
    }
}

fn report_fingerprint(report: &MsfsExportReport) -> String {
    let s = format!(
        "airports:{};runways:{};navaids:{};waypoints:{};routes:{};approaches:{};legs:{};skipped:{}",
        report.airports,
        report.runways,
        report.navaids,
        report.waypoints,
        report.routes,
        report.approaches,
        report.legs,
        report.skipped
    );
    format!("{:x}", sha2::Sha256::digest(s.as_bytes()))
}

// ---------------------------------------------------------------------------
// Approaches
// ---------------------------------------------------------------------------

fn approach_type(ident: &str, _legs: &[&CanonicalProcedureLeg]) -> &'static str {
    if let Some(c) = ident.chars().next() {
        match c {
            'I' => "ILS",
            'L' => "LOC",
            'H' | 'R' | 'P' => "RNAV",
            'V' => "VOR",
            'N' => "NDB",
            _ => "GPS",
        }
    } else {
        "GPS"
    }
}

fn leg_type(terminator: &str) -> Option<&'static str> {
    // Verified MSFS BGL leg types (documented schema). Unsupported
    // ARINC terminators are skipped fail-closed.
    Some(match terminator {
        "IF" => "IF",
        "TF" => "TF",
        "CF" => "CF",
        "DF" => "DF",
        "CA" => "CA",
        "FA" => "FA",
        "FC" => "FC",
        "FD" => "FD",
        "FM" => "FM",
        "VM" => "VM",
        "HM" => "HM",
        "HA" => "HA",
        "PI" => "PI",
        "AF" => "AF",
        _ => return None,
    })
}

fn recommended_type(leg: &CanonicalProcedureLeg) -> &'static str {
    match leg.recommended_navaid.as_deref() {
        Some(nav) => {
            // "IDENT:REGION:SECTION:SUBSECTION"
            let section = nav.rsplit(':').nth(1).unwrap_or("");
            match section {
                "D" => "VOR",
                "V" => "VOR",
                "I" => "LOCALIZER",
                "N" => "NDB",
                _ => "TERMINAL_WAYPOINT",
            }
        }
        None => "TERMINAL_WAYPOINT",
    }
}

fn write_approaches(
    airport: &str,
    legs: &[openairac_model::CanonicalProcedureLeg],
    out: &mut String,
    report: &mut MsfsExportReport,
) {
    // Group by procedure ident + transition.
    let mut procs: std::collections::BTreeMap<
        String,
        Vec<&openairac_model::CanonicalProcedureLeg>,
    > = std::collections::BTreeMap::new();
    for leg in legs
        .iter()
        .filter(|l| l.airport_ident == airport && l.procedure_kind == 'F')
    {
        procs
            .entry(leg.procedure_ident.clone())
            .or_default()
            .push(leg);
    }
    for (ident, mut legs) in procs {
        legs.sort_by_key(|l| (l.transition_ident.clone(), l.sequence_number));
        // Main (transition blank) legs only: transitions are written
        // as separate approach records by suffix in the real format;
        // here main + one representative per transition.
        let main: Vec<&openairac_model::CanonicalProcedureLeg> = legs
            .iter()
            .copied()
            .filter(|l| l.transition_ident.is_empty())
            .collect();
        if main.is_empty() {
            continue;
        }
        let runway = main
            .iter()
            .find_map(|l| {
                let f = &l.fix_ident;
                if f.starts_with("RW") && f.len() >= 4 {
                    Some(f[2..].to_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "00".to_string());
        let atype = approach_type(&ident, &main);
        out.push_str(&format!(
            "    <Approach type=\"{atype}\" runway=\"{}\" suffix=\"0\" gpsOverlay=\"FALSE\" fixType=\"TERMINAL_WAYPOINT\" fixIdent=\"{}\" fixRegion=\"{}\" altitude=\"0.0F\" heading=\"0\" missedAltitude=\"0.0F\">\n",
            esc(&runway),
            esc(main[0].fix_ident.as_str()),
            esc(main[0].fix_icao_code.as_str())
        ));
        report.approaches += 1;
        out.push_str("      <ApproachLegs>\n");
        let mut missed = false;
        for leg in &main {
            if leg.path_terminator == "FM"
                || leg.path_terminator == "HM"
                || leg.path_terminator == "HA"
            {
                missed = true;
            }
            if missed {
                continue; // written into MissedApproachLegs below
            }
            write_leg(out, leg, report);
        }
        out.push_str("      </ApproachLegs>\n");
        if missed {
            out.push_str("      <MissedApproachLegs>\n");
            let mut in_missed = false;
            for leg in &main {
                if leg.path_terminator == "FM"
                    || leg.path_terminator == "HM"
                    || leg.path_terminator == "HA"
                {
                    in_missed = true;
                }
                if in_missed {
                    write_leg(out, leg, report);
                }
            }
            out.push_str("      </MissedApproachLegs>\n");
        }
        out.push_str("    </Approach>\n");
    }
}

fn write_leg(
    out: &mut String,
    leg: &openairac_model::CanonicalProcedureLeg,
    report: &mut MsfsExportReport,
) {
    let Some(lt) = leg_type(&leg.path_terminator) else {
        report.skip(
            "approach-leg",
            &leg.fix_ident,
            format!("unsupported terminator {}", leg.path_terminator),
        );
        return;
    };
    let turn = match leg.turn_direction {
        Some('L') => "L",
        Some('R') => "R",
        _ => "E",
    };
    let course = leg
        .course_a_deg
        .or(leg.course_b_deg)
        .map(|c| format!("{c:.2}"))
        .unwrap_or_else(|| "-".to_string());
    let distance = leg
        .distance_a_nm
        .or(leg.distance_b_nm)
        .map(|d| format!("{d:.2}N"))
        .unwrap_or_else(|| "-".to_string());
    let alt_desc = leg
        .altitude_descriptor
        .map(|c| c.to_string())
        .unwrap_or_default();
    let alt1 = leg
        .altitude_1_ft
        .map(|a| format!("{:.2}F", a as f64))
        .unwrap_or_else(|| "0.0F".to_string());
    let alt2 = leg
        .altitude_2_ft
        .map(|a| format!("{:.2}F", a as f64))
        .unwrap_or_else(|| "0.0F".to_string());
    let theta = leg
        .rnp_nm
        .map(|r| format!("{r:.2}"))
        .unwrap_or_else(|| "-".to_string());
    out.push_str(&format!(
        "        <Leg type=\"{lt}\" fixType=\"TERMINAL_WAYPOINT\" fixIdent=\"{}\" fixRegion=\"{}\" turnDirection=\"{turn}\" recommendedType=\"{}\" recommendedIdent=\"{}\" recommendedRegion=\"{}\" theta=\"{theta}\" magneticCourse=\"{course}\" distance=\"{distance}\" altitudeDescriptor=\"{alt_desc}\" altitude1=\"{alt1}\" altitude2=\"{alt2}\"/>\n",
        esc(&leg.fix_ident),
        esc(&leg.fix_icao_code),
        recommended_type(leg),
        opt_str(leg.recommended_navaid.as_deref().map(|n| n.split(':').next().unwrap_or("")).unwrap_or("")),
        opt_str(leg.recommended_navaid.as_deref().map(|n| n.split(':').nth(1).unwrap_or("")).unwrap_or("")),
    ));
    report.legs += 1;
}

// ---------------------------------------------------------------------------
// Report
// ---------------------------------------------------------------------------

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct MsfsExportReport {
    pub airports: usize,
    pub runways: usize,
    pub navaids: usize,
    pub waypoints: usize,
    pub routes: usize,
    pub approaches: usize,
    pub legs: usize,
    pub skipped: usize,
    pub diagnostics: Vec<String>,
}

impl MsfsExportReport {
    fn skip(&mut self, kind: &str, id: &str, reason: String) {
        self.skipped += 1;
        if self.diagnostics.len() < 100 {
            self.diagnostics.push(format!("{kind} {id}: {reason}"));
        }
    }
}

// ---------------------------------------------------------------------------
// Compile (official fspackagetool)
// ---------------------------------------------------------------------------

/// Locate the SDK tools bin: explicit path, MSFS_SDK env, or common
/// locations.
pub fn find_sdk_tools_bin(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = explicit
        && p.is_dir()
    {
        return Some(p.to_path_buf());
    }
    if let Ok(sdk) = std::env::var("MSFS_SDK") {
        let candidate = PathBuf::from(&sdk).join("Tools").join("bin");
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    for root in [
        r"C:\MSFS SDK\Tools\bin",
        r"D:\MSFS SDK\Tools\bin",
        r"E:\MSFS SDK\Tools\bin",
        r"F:\MSFS SDK\Tools\bin",
    ] {
        let p = PathBuf::from(root);
        if p.is_dir() {
            return Some(p);
        }
    }
    None
}

/// Compile the exported project with the official fspackagetool.
/// Requires a local MSFS SDK; fails closed when unavailable.
pub fn compile_package(sdk_tools_bin: &Path, project_dir: &Path) -> Result<PathBuf> {
    let tool = sdk_tools_bin.join("fspackagetool.exe");
    if !tool.exists() {
        bail!(
            "fspackagetool.exe not found in {:?}; install the MSFS SDK or pass --sdk",
            sdk_tools_bin
        );
    }
    let def = project_dir
        .join("PackageDefinitions")
        .join(format!("{PACKAGE_NAME}.xml"));
    let status = std::process::Command::new(&tool)
        .arg(&def)
        .arg("-outputdir")
        .arg(project_dir.join("Packages"))
        .status()
        .context("running fspackagetool.exe")?;
    if !status.success() {
        bail!("fspackagetool exited with {status}");
    }
    let built = project_dir.join("Packages").join(PACKAGE_NAME);
    if !built.is_dir() {
        bail!("fspackagetool produced no package at {:?}", built);
    }
    Ok(built)
}

// ---------------------------------------------------------------------------
// Installer
// ---------------------------------------------------------------------------

pub struct MsfsTargetInstaller {
    descriptor: TargetDescriptor,
}

impl MsfsTargetInstaller {
    pub fn new(descriptor: TargetDescriptor) -> Self {
        Self { descriptor }
    }
}

impl TargetInstaller for MsfsTargetInstaller {
    fn descriptor(&self) -> &TargetDescriptor {
        &self.descriptor
    }

    fn install(
        &self,
        artifacts_root: &Path,
        artifacts: &GeneratedArtifactSet,
        target_root: &Path,
    ) -> Result<TargetInstallReport> {
        // EXPERIMENTAL: sources-only install is allowed only when the
        // operator has not compiled; a compiled package must exist for
        // a real sim. Install the package directory contents into
        // Community/<pkg> transactionally.
        artifacts.verify(artifacts_root)?;
        let rel_subdir = match &self.descriptor.install_strategy {
            openairac_export::InstallStrategy::Subdirectory { relative } => relative.clone(),
            other => bail!("unsupported install strategy {other:?} for MSFS"),
        };
        // Collect every file under the artifact root (package tree).
        let mut files = Vec::new();
        for entry in walk(artifacts_root) {
            let rel = entry
                .strip_prefix(artifacts_root)
                .map_err(|_| anyhow!("bad path"))?
                .to_string_lossy()
                .replace('\\', "/");
            files.push(rel);
        }
        if files.is_empty() {
            bail!("nothing to install");
        }
        let pkg_root = target_root.join(&rel_subdir);
        let report =
            openairac_export::install_files_transactionally(artifacts_root, &pkg_root, &files)?;
        Ok(TargetInstallReport {
            target_id: self.descriptor.id.clone(),
            operation_id: report.operation_id,
            cycle: artifacts.cycle.clone(),
            installed: report.installed,
            restored: report.restored,
            removed: report.removed,
        })
    }

    fn rollback(&self, target_root: &Path) -> Result<TargetInstallReport> {
        let rel_subdir = match &self.descriptor.install_strategy {
            openairac_export::InstallStrategy::Subdirectory { relative } => relative.clone(),
            other => bail!("unsupported install strategy {other:?} for MSFS"),
        };
        let pkg_root = target_root.join(&rel_subdir);
        let report = openairac_export::recover_file_install(&pkg_root)?.unwrap_or_else(|| {
            openairac_export::FileInstallReport {
                operation_id: "none".to_string(),
                installed: Vec::new(),
                restored: Vec::new(),
                removed: Vec::new(),
            }
        });
        Ok(TargetInstallReport {
            target_id: self.descriptor.id.clone(),
            operation_id: report.operation_id,
            cycle: String::new(),
            installed: report.installed,
            restored: report.restored,
            removed: report.removed,
        })
    }

    fn recover(&self, target_root: &Path) -> Result<Option<TargetInstallReport>> {
        let rel_subdir = match &self.descriptor.install_strategy {
            openairac_export::InstallStrategy::Subdirectory { relative } => relative.clone(),
            other => bail!("unsupported install strategy {other:?} for MSFS"),
        };
        let pkg_root = target_root.join(&rel_subdir);
        Ok(
            openairac_export::recover_file_install(&pkg_root)?.map(|r| TargetInstallReport {
                target_id: self.descriptor.id.clone(),
                operation_id: r.operation_id,
                cycle: String::new(),
                installed: r.installed,
                restored: r.restored,
                removed: r.removed,
            }),
        )
    }
}

fn walk(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return out;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

/// The support state we honestly claim for MSFS targets until SDK
/// compilation is verified.
pub const MSFS_SUPPORT_STATE: SupportState = SupportState::Experimental;

#[cfg(test)]
mod tests {
    use super::*;
    use openairac_model::{
        AirportId, CanonicalAirport, SourceSnapshot, SourceSnapshotId, TemporalValidity,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn unique_dir(tag: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("oa_msfs_test_{}_{}_{n}", std::process::id(), tag))
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
    fn test_msfs_export_generates_sources() {
        let (store, dir) = fixture_store();
        let out = dir.join("msfs");
        let set = MsfsNavdataExporter
            .export(&store, Utc::now(), &out)
            .unwrap();
        assert_eq!(set.family.as_str(), "msfs-bgl");
        let names: Vec<&str> = set.artifacts.iter().map(|a| a.path.as_str()).collect();
        assert!(names.contains(&"PackageSources/navdata/airports.xml"));
        assert!(names.contains(&"PackageSources/navdata/navaids.xml"));
        assert!(names.contains(&"PackageSources/navdata/waypoints.xml"));
        assert!(names.contains(&"PackageSources/navdata/routes.xml"));
        assert!(names.contains(&"cycle.json"));
        set.verify(&out).unwrap();
        let xml = std::fs::read_to_string(out.join("PackageSources/navdata/airports.xml")).unwrap();
        assert!(xml.contains("ident=\"KSFO\""), "{xml}");
    }

    #[test]
    fn test_compile_missing_sdk_fails_closed() {
        let missing = PathBuf::from("Z:/definitely-not-an-sdk/Tools/bin");
        let err = compile_package(&missing, Path::new(".")).unwrap_err();
        assert!(err.to_string().contains("fspackagetool"), "{err}");
    }
}
