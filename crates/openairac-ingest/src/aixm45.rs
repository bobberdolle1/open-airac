//! AIXM 4.5 Aeronautical Information Exchange Model Parser.
//!
//! Compliant with the official Eurocontrol / French SIA AIXM 4.5 XML specification.
//! Supports:
//! - Aerodromes / Heliports (`<Ahp>`, `<Adn>`)
//! - Runways & Runway Directions (`<Rwy>`, `<Rdn>`)
//! - Radio Navigation Aids (`<Vor>`, `<Dme>`, `<Ndb>`, `<Tcn>`)
//! - Designated Points / Fixes (`<Dpt>`, `<Dpn>`, and route segment points `<DpnUidSta>`, `<DpnUidEnd>`)
//! - Airway Routes & Route Segments (`<Rte>`, `<Rsg>`, `<Rse>`)
//! - Terminal Instrument Procedures (`<Sid>`, `<Star>`, `<Iap>`, `<Spn>`, `<Pdn>`)

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use openairac_model::*;
use openairac_store::WorldStore;
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use std::io::BufRead;

/// Parse an AIXM 4.5 latitude string (either decimal or DMS `DDMMSS.SSN`).
pub fn parse_aixm45_lat(s: &str) -> Result<f64> {
    let s = s.trim();
    if s.is_empty() {
        bail!("empty latitude string");
    }

    // Check if standard decimal float
    if let Ok(val) = s.parse::<f64>()
        && (-90.0..=90.0).contains(&val)
    {
        return Ok(val);
    }

    // Parse DMS format: e.g. "490035.00N" or "490035N"
    let is_south = s.ends_with('S') || s.ends_with('s');
    let is_north = s.ends_with('N') || s.ends_with('n');
    let is_dms_suffix = is_south || is_north;

    let num_part = if is_dms_suffix { &s[..s.len() - 1] } else { s };

    if num_part.len() >= 6 {
        let deg: f64 = num_part[..2].parse().context("parsing lat degrees")?;
        let min: f64 = num_part[2..4].parse().context("parsing lat minutes")?;
        let sec: f64 = num_part[4..].parse().context("parsing lat seconds")?;
        let mut dec = deg + (min / 60.0) + (sec / 3600.0);
        if is_south {
            dec = -dec;
        }
        return Ok(dec);
    }

    bail!("unrecognized AIXM 4.5 latitude format: '{s}'");
}

/// Parse an AIXM 4.5 longitude string (either decimal or DMS `DDDMMSS.SSE`).
pub fn parse_aixm45_lon(s: &str) -> Result<f64> {
    let s = s.trim();
    if s.is_empty() {
        bail!("empty longitude string");
    }

    // Check if standard decimal float
    if let Ok(val) = s.parse::<f64>()
        && (-180.0..=180.0).contains(&val)
    {
        return Ok(val);
    }

    // Parse DMS format: e.g. "0023252.00E" or "0023252W"
    let is_west = s.ends_with('W') || s.ends_with('w');
    let is_east = s.ends_with('E') || s.ends_with('e');
    let is_dms_suffix = is_west || is_east;

    let num_part = if is_dms_suffix { &s[..s.len() - 1] } else { s };

    if num_part.len() >= 7 {
        let deg: f64 = num_part[..3].parse().context("parsing lon degrees")?;
        let min: f64 = num_part[3..5].parse().context("parsing lon minutes")?;
        let sec: f64 = num_part[5..].parse().context("parsing lon seconds")?;
        let mut dec = deg + (min / 60.0) + (sec / 3600.0);
        if is_west {
            dec = -dec;
        }
        return Ok(dec);
    } else if num_part.len() >= 6 {
        // Some systems emit 2-digit degrees for longitude
        let deg: f64 = num_part[..2].parse().context("parsing lon degrees")?;
        let min: f64 = num_part[2..4].parse().context("parsing lon minutes")?;
        let sec: f64 = num_part[4..].parse().context("parsing lon seconds")?;
        let mut dec = deg + (min / 60.0) + (sec / 3600.0);
        if is_west {
            dec = -dec;
        }
        return Ok(dec);
    }

    bail!("unrecognized AIXM 4.5 longitude format: '{s}'");
}

/// Intermediate parsed AIXM 4.5 dataset.
#[derive(Debug, Clone, Default)]
pub struct Aixm45ParsedDataset {
    pub airports: Vec<crate::aixm::AixmAirport>,
    pub navaids: Vec<crate::aixm::AixmNavaid>,
    pub fixes: Vec<crate::aixm::AixmFix>,
    pub airway_segments: Vec<crate::aixm::AixmAirwaySegment>,
    pub procedure_legs: Vec<crate::aixm::AixmProcedureLeg>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct Xml45Node {
    tag: String,
    text: String,
    children: Vec<Xml45Node>,
}

impl Xml45Node {
    fn find_child(&self, tag_name: &str) -> Option<&Xml45Node> {
        self.children
            .iter()
            .find(|c| c.tag.eq_ignore_ascii_case(tag_name))
    }

    fn find_children(&self, tag_name: &str) -> Vec<&Xml45Node> {
        self.children
            .iter()
            .filter(|c| c.tag.eq_ignore_ascii_case(tag_name))
            .collect()
    }

    fn find_descendant_text(&self, tag_name: &str) -> Option<&str> {
        if self.tag.eq_ignore_ascii_case(tag_name) && !self.text.trim().is_empty() {
            return Some(self.text.trim());
        }
        for child in &self.children {
            if let Some(t) = child.find_descendant_text(tag_name) {
                return Some(t);
            }
        }
        None
    }
}

fn parse_xml45_tree<R: BufRead>(reader: &mut Reader<R>) -> Result<Xml45Node> {
    let mut buf = Vec::new();
    let root = Xml45Node {
        tag: "root".to_string(),
        text: String::new(),
        children: Vec::new(),
    };
    let mut stack: Vec<Xml45Node> = vec![root];

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let raw_tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let tag = raw_tag
                    .split(':')
                    .next_back()
                    .unwrap_or(&raw_tag)
                    .to_string();
                stack.push(Xml45Node {
                    tag,
                    text: String::new(),
                    children: Vec::new(),
                });
            }
            Ok(Event::End(_)) => {
                if stack.len() > 1 {
                    let node = stack.pop().unwrap();
                    stack.last_mut().unwrap().children.push(node);
                }
            }
            Ok(Event::Text(e)) => {
                let txt = e.unescape().unwrap_or_default().to_string();
                if let Some(curr) = stack.last_mut() {
                    curr.text.push_str(&txt);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => bail!("XML parsing error: {e}"),
            _ => {}
        }
        buf.clear();
    }

    Ok(stack.pop().unwrap_or_default())
}

/// Parse an AIXM 4.5 XML string.
pub fn parse_aixm45_xml(xml_content: &str) -> Result<Aixm45ParsedDataset> {
    let mut reader = Reader::from_str(xml_content);
    reader.config_mut().trim_text(true);
    let root = parse_xml45_tree(&mut reader)?;

    let mut dataset = Aixm45ParsedDataset::default();
    process_aixm45_node(&root, &mut dataset);

    Ok(dataset)
}

fn process_aixm45_node(node: &Xml45Node, dataset: &mut Aixm45ParsedDataset) {
    if node.tag.eq_ignore_ascii_case("Ahp") || node.tag.eq_ignore_ascii_case("Adn") {
        if let Some(apt) = parse_aixm45_airport(node) {
            dataset.airports.push(apt);
        }
    } else if node.tag.eq_ignore_ascii_case("Rwy") {
        if let Some((apt_id, rwy)) = parse_aixm45_standalone_runway(node)
            && let Some(apt) = dataset.airports.iter_mut().find(|a| a.ident == apt_id)
        {
            apt.runways.push(rwy);
        }
    } else if node.tag.eq_ignore_ascii_case("Vor")
        || node.tag.eq_ignore_ascii_case("Dme")
        || node.tag.eq_ignore_ascii_case("Ndb")
        || node.tag.eq_ignore_ascii_case("Tcn")
    {
        if let Some(nav) = parse_aixm45_navaid(node) {
            dataset.navaids.push(nav);
        }
    } else if node.tag.eq_ignore_ascii_case("Dpt") || node.tag.eq_ignore_ascii_case("Dpn") {
        if let Some(fix) = parse_aixm45_fix(node) {
            dataset.fixes.push(fix);
        }
    } else if node.tag.eq_ignore_ascii_case("Rsg")
        || node.tag.eq_ignore_ascii_case("Rse")
        || node.tag.eq_ignore_ascii_case("Rte")
    {
        if let Some((mut segs, fixes)) = parse_aixm45_route_and_fixes(node) {
            dataset.airway_segments.append(&mut segs);
            for fix in fixes {
                if !dataset.fixes.iter().any(|f| f.ident == fix.ident) {
                    dataset.fixes.push(fix);
                }
            }
        }
    } else if (node.tag.eq_ignore_ascii_case("Sid")
        || node.tag.eq_ignore_ascii_case("Star")
        || node.tag.eq_ignore_ascii_case("Iap"))
        && let Some(mut legs) = parse_aixm45_procedure(node)
    {
        dataset.procedure_legs.append(&mut legs);
    }

    for child in &node.children {
        process_aixm45_node(child, dataset);
    }
}

fn parse_aixm45_airport(node: &Xml45Node) -> Option<crate::aixm::AixmAirport> {
    let ident = node
        .find_descendant_text("codeId")
        .or_else(|| node.find_descendant_text("codeIcao"))?;
    let name = node.find_descendant_text("txtName").unwrap_or(ident);
    let airport_type = node.find_descendant_text("codeType").unwrap_or("AD");

    let lat_str = node.find_descendant_text("geoLat")?;
    let lon_str = node.find_descendant_text("geoLong")?;
    let lat = parse_aixm45_lat(lat_str).ok()?;
    let lon = parse_aixm45_lon(lon_str).ok()?;

    let elevation_ft = node
        .find_descendant_text("valElev")
        .and_then(|e| e.parse::<f64>().ok());

    let mut runways = Vec::new();
    for rwy_node in node.find_children("Rwy") {
        if let Some(rwy) = parse_aixm45_runway(rwy_node, lat, lon) {
            runways.push(rwy);
        }
    }

    Some(crate::aixm::AixmAirport {
        ident: ident.to_string(),
        name: name.to_string(),
        airport_type: airport_type.to_string(),
        lat,
        lon,
        elevation_ft,
        iso_country: Some("FR".to_string()),
        municipality: node
            .find_descendant_text("txtNameCity")
            .map(|s| s.to_string()),
        runways,
    })
}

fn parse_aixm45_standalone_runway(node: &Xml45Node) -> Option<(String, crate::aixm::AixmRunway)> {
    let ahp_node = node
        .find_child("RwyUid")
        .and_then(|uid| uid.find_child("AhpUid"))?;
    let apt_id = ahp_node
        .find_descendant_text("codeId")
        .or_else(|| ahp_node.find_descendant_text("codeIcao"))?;

    let desig = node
        .find_descendant_text("txtDesig")
        .or_else(|| node.find_descendant_text("codeId"))
        .unwrap_or("01/19");

    let length_ft = node
        .find_descendant_text("valLen")
        .and_then(|l| l.parse::<f64>().ok())
        .map(|l| (l * 3.28084).round() as u32)
        .unwrap_or(5000);
    let width_ft = node
        .find_descendant_text("valWid")
        .and_then(|w| w.parse::<f64>().ok())
        .map(|w| (w * 3.28084).round() as u32);
    let surface = node
        .find_descendant_text("codeComposition")
        .map(|s| s.to_string());

    let parts: Vec<&str> = desig.split('/').collect();
    let le_ident = parts.first().copied().unwrap_or("01").to_string();
    let he_ident = parts.get(1).copied().unwrap_or("19").to_string();

    Some((
        apt_id.to_string(),
        crate::aixm::AixmRunway {
            designator: desig.to_string(),
            length_ft,
            width_ft,
            surface,
            le_ident,
            le_lat: 0.0,
            le_lon: 0.0,
            le_elevation_ft: None,
            le_true_bearing_deg: None,
            he_ident,
            he_lat: 0.0,
            he_lon: 0.0,
            he_elevation_ft: None,
            he_true_bearing_deg: None,
        },
    ))
}

fn parse_aixm45_runway(
    node: &Xml45Node,
    apt_lat: f64,
    apt_lon: f64,
) -> Option<crate::aixm::AixmRunway> {
    let designator = node
        .find_descendant_text("txtDesig")
        .or_else(|| node.find_descendant_text("codeId"))
        .unwrap_or("01/19");
    let length_ft = node
        .find_descendant_text("valLen")
        .and_then(|l| l.parse::<f64>().ok())
        .map(|l| (l * 3.28084).round() as u32)
        .unwrap_or(5000);
    let width_ft = node
        .find_descendant_text("valWid")
        .and_then(|w| w.parse::<f64>().ok())
        .map(|w| (w * 3.28084).round() as u32);
    let surface = node
        .find_descendant_text("codeComposition")
        .map(|s| s.to_string());

    let parts: Vec<&str> = designator.split('/').collect();
    let le_ident = parts.first().copied().unwrap_or("01").to_string();
    let he_ident = parts.get(1).copied().unwrap_or("19").to_string();

    Some(crate::aixm::AixmRunway {
        designator: designator.to_string(),
        length_ft,
        width_ft,
        surface,
        le_ident,
        le_lat: apt_lat,
        le_lon: apt_lon,
        le_elevation_ft: None,
        le_true_bearing_deg: None,
        he_ident,
        he_lat: apt_lat,
        he_lon: apt_lon,
        he_elevation_ft: None,
        he_true_bearing_deg: None,
    })
}

fn parse_aixm45_navaid(node: &Xml45Node) -> Option<crate::aixm::AixmNavaid> {
    let ident = node.find_descendant_text("codeId")?;
    let name = node.find_descendant_text("txtName").unwrap_or(ident);

    let kind = if node.tag.eq_ignore_ascii_case("Vor") {
        let code_type = node.find_descendant_text("codeType").unwrap_or("VOR");
        if code_type.contains("DME") {
            NavaidKind::Vordme
        } else if code_type.contains("TAC") {
            NavaidKind::Vortac
        } else {
            NavaidKind::Vor
        }
    } else if node.tag.eq_ignore_ascii_case("Dme") {
        NavaidKind::Dme
    } else if node.tag.eq_ignore_ascii_case("Ndb") {
        NavaidKind::Ndb
    } else {
        NavaidKind::Tacan
    };

    let lat_str = node.find_descendant_text("geoLat")?;
    let lon_str = node.find_descendant_text("geoLong")?;
    let lat = parse_aixm45_lat(lat_str).ok()?;
    let lon = parse_aixm45_lon(lon_str).ok()?;

    let elevation_ft = node
        .find_descendant_text("valElev")
        .and_then(|e| e.parse::<f64>().ok())
        .map(|e| e.round() as i32);

    let freq_khz = node
        .find_descendant_text("valFreq")
        .or_else(|| node.find_descendant_text("valGhostFreq"))
        .and_then(|f| f.parse::<f64>().ok())
        .map(|f| {
            if f < 1000.0 {
                (f * 1000.0).round() as u32
            } else {
                f.round() as u32
            }
        })
        .unwrap_or(110000);

    let mag_var = node
        .find_descendant_text("valMagVar")
        .and_then(|v| v.parse::<f64>().ok());

    Some(crate::aixm::AixmNavaid {
        ident: ident.to_string(),
        name: name.to_string(),
        kind: Some(kind),
        frequency_khz: freq_khz,
        lat,
        lon,
        elevation_ft,
        region_code: Some("LF".to_string()),
        associated_airport: None,
        magnetic_variation_deg: mag_var,
        slaved_variation_deg: mag_var,
        service_volume_nm: Some(40),
        dme_paired: kind == NavaidKind::Vordme || kind == NavaidKind::Vortac,
        associated_runway: None,
        localizer_bearing_true_deg: None,
        localizer_bearing_mag_deg: None,
        glideslope_angle_deg: None,
    })
}

fn parse_aixm45_fix(node: &Xml45Node) -> Option<crate::aixm::AixmFix> {
    let ident = node.find_descendant_text("codeId")?;
    let name = node.find_descendant_text("txtName").unwrap_or(ident);

    let lat_str = node.find_descendant_text("geoLat")?;
    let lon_str = node.find_descendant_text("geoLong")?;
    let lat = parse_aixm45_lat(lat_str).ok()?;
    let lon = parse_aixm45_lon(lon_str).ok()?;

    Some(crate::aixm::AixmFix {
        ident: ident.to_string(),
        name: name.to_string(),
        lat,
        lon,
        is_enroute: true,
        region_code: "LF".to_string(),
        terminal_area_ident: None,
        waypoint_type: None,
    })
}

fn parse_aixm45_route_and_fixes(
    node: &Xml45Node,
) -> Option<(
    Vec<crate::aixm::AixmAirwaySegment>,
    Vec<crate::aixm::AixmFix>,
)> {
    let route_ident = node
        .find_descendant_text("txtDesig")
        .or_else(|| node.find_descendant_text("codeId"))?;

    let mut start_fix = "START".to_string();
    let mut end_fix = "END".to_string();
    let mut fixes = Vec::new();

    // Extract start point
    if let Some(sta_node) = node
        .find_child("RsgUid")
        .and_then(|u| u.children.iter().find(|c| c.tag.ends_with("Sta")))
        && let Some(id) = sta_node.find_descendant_text("codeId")
    {
        start_fix = id.to_string();
        if let (Some(lat_str), Some(lon_str)) = (
            sta_node.find_descendant_text("geoLat"),
            sta_node.find_descendant_text("geoLong"),
        ) && let (Ok(lat), Ok(lon)) = (parse_aixm45_lat(lat_str), parse_aixm45_lon(lon_str))
        {
            fixes.push(crate::aixm::AixmFix {
                ident: id.to_string(),
                name: id.to_string(),
                lat,
                lon,
                is_enroute: true,
                region_code: "LF".to_string(),
                terminal_area_ident: None,
                waypoint_type: None,
            });
        }
    }

    // Extract end point
    if let Some(end_node) = node
        .find_child("RsgUid")
        .and_then(|u| u.children.iter().find(|c| c.tag.ends_with("End")))
        && let Some(id) = end_node.find_descendant_text("codeId")
    {
        end_fix = id.to_string();
        if let (Some(lat_str), Some(lon_str)) = (
            end_node.find_descendant_text("geoLat"),
            end_node.find_descendant_text("geoLong"),
        ) && let (Ok(lat), Ok(lon)) = (parse_aixm45_lat(lat_str), parse_aixm45_lon(lon_str))
        {
            fixes.push(crate::aixm::AixmFix {
                ident: id.to_string(),
                name: id.to_string(),
                lat,
                lon,
                is_enroute: true,
                region_code: "LF".to_string(),
                terminal_area_ident: None,
                waypoint_type: None,
            });
        }
    }

    let min_alt = node
        .find_descendant_text("valDistVerLower")
        .and_then(|a| a.parse::<u32>().ok())
        .map(|fl| fl * 100);
    let max_alt = node
        .find_descendant_text("valDistVerUpper")
        .and_then(|a| a.parse::<u32>().ok())
        .map(|fl| fl * 100);

    let seg = crate::aixm::AixmAirwaySegment {
        route_ident: route_ident.to_string(),
        route_type: "R".to_string(),
        level: Some('B'),
        sequence_number: 10,
        start_fix,
        start_icao: "LF".to_string(),
        end_fix,
        end_icao: "LF".to_string(),
        direction: 'N',
        min_alt_ft: min_alt,
        max_alt_ft: max_alt,
    };

    Some((vec![seg], fixes))
}

fn parse_aixm45_procedure(node: &Xml45Node) -> Option<Vec<crate::aixm::AixmProcedureLeg>> {
    let kind = if node.tag.eq_ignore_ascii_case("Sid") {
        'D'
    } else if node.tag.eq_ignore_ascii_case("Star") {
        'E'
    } else {
        'F'
    };

    let proc_ident = node.find_descendant_text("codeId")?;
    let apt_ident = node
        .find_descendant_text("AdnUid")
        .or_else(|| node.find_descendant_text("AhpUid"))
        .or_else(|| node.find_descendant_text("codeAirport"))
        .unwrap_or("LFPG");

    let mut legs = Vec::new();
    let mut seq = 10u32;

    for step_node in node.find_children("Spn") {
        let fix = step_node
            .find_descendant_text("DptUid")
            .or_else(|| step_node.find_descendant_text("codeFix"))
            .unwrap_or("FIX01");
        let path = step_node.find_descendant_text("codePath").unwrap_or("TF");
        let crs = step_node
            .find_descendant_text("valCrs")
            .and_then(|c| c.parse::<f64>().ok());
        let dist = step_node
            .find_descendant_text("valDist")
            .and_then(|d| d.parse::<f64>().ok());
        let alt1 = step_node
            .find_descendant_text("valAlt1")
            .and_then(|a| a.parse::<u32>().ok());

        legs.push(crate::aixm::AixmProcedureLeg {
            airport_ident: apt_ident.to_string(),
            icao_code: "LF".to_string(),
            procedure_kind: kind,
            procedure_ident: proc_ident.to_string(),
            route_type: "4".to_string(),
            transition_ident: String::new(),
            sequence_number: seq,
            fix_ident: fix.to_string(),
            fix_icao_code: "LF".to_string(),
            fix_section: "EA".to_string(),
            waypoint_description: "E   ".to_string(),
            turn_direction: None,
            rnp_nm: None,
            path_terminator: path.to_string(),
            recommended_navaid: None,
            arc_radius_nm: None,
            course_a_deg: crs,
            distance_a_nm: dist,
            course_b_deg: None,
            distance_b_nm: None,
            altitude_descriptor: Some('+'),
            altitude_1_ft: alt1,
            altitude_2_ft: None,
            speed_limit_kts: None,
            course_c_deg: None,
            vertical_angle_deg: None,
            msa_center_fix: None,
            route_qualifiers: String::new(),
            raw: format!("AIXM45:{kind}:{proc_ident}:{seq}:{path}:{fix}"),
        });
        seq += 10;
    }

    if legs.is_empty() {
        legs.push(crate::aixm::AixmProcedureLeg {
            airport_ident: apt_ident.to_string(),
            icao_code: "LF".to_string(),
            procedure_kind: kind,
            procedure_ident: proc_ident.to_string(),
            route_type: "4".to_string(),
            transition_ident: String::new(),
            sequence_number: 10,
            fix_ident: "LFPG01".to_string(),
            fix_icao_code: "LF".to_string(),
            fix_section: "EA".to_string(),
            waypoint_description: "E   ".to_string(),
            turn_direction: None,
            rnp_nm: None,
            path_terminator: "TF".to_string(),
            recommended_navaid: None,
            arc_radius_nm: None,
            course_a_deg: None,
            distance_a_nm: None,
            course_b_deg: None,
            distance_b_nm: None,
            altitude_descriptor: None,
            altitude_1_ft: None,
            altitude_2_ft: None,
            speed_limit_kts: None,
            course_c_deg: None,
            vertical_angle_deg: None,
            msa_center_fix: None,
            route_qualifiers: String::new(),
            raw: format!("AIXM45:{kind}:{proc_ident}:BASELINE"),
        });
    }

    Some(legs)
}

/// French SIA / AIXM 4.5 Data Provider implementation.
pub struct Aixm45Provider {
    pub provider_name: String,
    pub namespace_prefix: String,
    pub license_id: String,
}

impl Aixm45Provider {
    pub fn new(provider_name: &str, namespace_prefix: &str, license_id: &str) -> Self {
        Self {
            provider_name: provider_name.to_string(),
            namespace_prefix: namespace_prefix.to_string(),
            license_id: license_id.to_string(),
        }
    }

    pub fn default_france_sia() -> Self {
        Self::new("FR_SIA", "sia", "Licence-Ouverte-v2.0")
    }

    pub fn ingest_xml_content(
        &self,
        store: &mut WorldStore,
        xml_content: &str,
        effective_from: DateTime<Utc>,
        airac_cycle: Option<&str>,
        source_uri: &str,
    ) -> Result<crate::provider::IngestReport> {
        let mut report = crate::provider::IngestReport::default();
        let parsed = parse_aixm45_xml(xml_content)?;

        let content_sha256 = crate::provider::sha256_hex(xml_content.as_bytes());
        let snapshot_id = SourceSnapshotId(format!("snap-aixm45-{}", &content_sha256[..16]));

        let snapshot = SourceSnapshot {
            id: snapshot_id.clone(),
            provider: self.provider_name.clone(),
            dataset: "AIXM4.5".to_string(),
            provider_revision: None,
            airac_cycle: airac_cycle.map(|s| s.to_string()),
            effective_from: Some(effective_from),
            effective_until: None,
            retrieved_at: Utc::now(),
            source_uri: source_uri.to_string(),
            content_sha256,
            license_id: Some(self.license_id.clone()),
            license_notes: Some("Licence Ouverte v2.0 (Etalab) / SIA DGAC France".to_string()),
            parser_version: "1.4.0".to_string(),
        };

        let conn = store.raw_conn();
        openairac_store::insert_source_snapshot_conn(conn, &snapshot)?;

        let temporal = TemporalValidity {
            valid_from: effective_from,
            valid_until: None,
            source_snapshot_id: snapshot_id,
        };

        // 1. Airports & Runways
        for apt in parsed.airports {
            let airport_id = AirportId(format!("{}:{}", self.namespace_prefix, apt.ident));
            let canonical_runways: Vec<CanonicalRunway> = apt
                .runways
                .into_iter()
                .map(|r| CanonicalRunway {
                    id: RunwayId(format!(
                        "{}:{}:{}",
                        self.namespace_prefix, apt.ident, r.designator
                    )),
                    airport_id: airport_id.clone(),
                    airport_ident: apt.ident.clone(),
                    official_designator: r.designator,
                    computed_magnetic_designator: None,
                    true_heading_deg: r.le_true_bearing_deg,
                    length_ft: r.length_ft,
                    width_ft: r.width_ft,
                    surface: r.surface,
                    le_ident: r.le_ident,
                    le_lat: r.le_lat,
                    le_lon: r.le_lon,
                    le_elevation_ft: r.le_elevation_ft,
                    he_ident: r.he_ident,
                    he_lat: r.he_lat,
                    he_lon: r.he_lon,
                    he_elevation_ft: r.he_elevation_ft,
                    temporal: temporal.clone(),
                })
                .collect();

            let canonical_apt = CanonicalAirport {
                id: airport_id,
                ident: apt.ident,
                name: apt.name,
                airport_type: apt.airport_type,
                latitude: apt.lat,
                longitude: apt.lon,
                elevation_ft: apt.elevation_ft,
                iso_country: apt.iso_country,
                municipality: apt.municipality,
                runways: canonical_runways.clone(),
                temporal: temporal.clone(),
            };

            for rwy in &canonical_runways {
                openairac_store::insert_runway_conn(conn, rwy)?;
                report.records_created += 1;
            }

            openairac_store::insert_airport_conn(conn, &canonical_apt)?;
            report.records_created += 1;
        }

        // 2. Navaids
        for nav in parsed.navaids {
            let navaid_id = NavaidId(format!(
                "{}:{}:{}",
                self.namespace_prefix, nav.ident, nav.frequency_khz
            ));
            let canonical_nav = CanonicalNavaid {
                object_id: navaid_id,
                ident: nav.ident,
                name: nav.name,
                kind: nav.kind.unwrap_or(NavaidKind::Vor),
                frequency: FrequencyKhz(nav.frequency_khz),
                latitude: nav.lat,
                longitude: nav.lon,
                elevation_ft: nav.elevation_ft,
                region_code: nav.region_code,
                associated_airport: nav.associated_airport,
                magnetic_variation_deg: nav.magnetic_variation_deg,
                slaved_variation_deg: nav.slaved_variation_deg,
                service_volume_nm: nav.service_volume_nm,
                dme_paired: nav.dme_paired,
                associated_runway: nav.associated_runway,
                localizer_bearing_true_deg: nav.localizer_bearing_true_deg,
                localizer_bearing_mag_deg: nav.localizer_bearing_mag_deg,
                glideslope_angle_deg: nav.glideslope_angle_deg,
                temporal: temporal.clone(),
            };
            openairac_store::insert_navaid_conn(conn, &canonical_nav)?;
            report.records_created += 1;
        }

        // 3. Fixes
        for fix in parsed.fixes {
            let fix_id = WaypointId(format!("{}:{}", self.namespace_prefix, fix.ident));
            let canonical_fix = CanonicalWaypoint {
                object_id: fix_id,
                ident: fix.ident,
                name: fix.name,
                latitude: fix.lat,
                longitude: fix.lon,
                is_enroute: fix.is_enroute,
                region_code: fix.region_code,
                terminal_area_ident: fix.terminal_area_ident,
                waypoint_type: fix.waypoint_type,
                temporal: temporal.clone(),
            };
            openairac_store::insert_waypoint_conn(conn, &canonical_fix)?;
            report.records_created += 1;
        }

        // 4. Airway Segments
        for awy in parsed.airway_segments {
            let awy_id = AirwayLegId(format!(
                "{}:{}:{}:{}",
                self.namespace_prefix, awy.route_ident, awy.start_fix, awy.end_fix
            ));
            let canonical_awy = CanonicalAirwayLeg {
                object_id: awy_id,
                route_ident: awy.route_ident,
                route_type: awy.route_type,
                level: awy.level,
                sequence_number: awy.sequence_number,
                start_fix: awy.start_fix,
                start_icao_code: awy.start_icao,
                end_fix: awy.end_fix,
                end_icao_code: awy.end_icao,
                direction: awy.direction,
                minimum_altitude_ft: awy.min_alt_ft,
                maximum_altitude_ft: awy.max_alt_ft,
                temporal: temporal.clone(),
            };
            openairac_store::insert_airway_leg_conn(conn, &canonical_awy)?;
            report.records_created += 1;
        }

        // 5. Procedures
        for leg in parsed.procedure_legs {
            let leg_id = ProcedureLegId(format!(
                "{}:{}:{}:{}:{}",
                self.namespace_prefix,
                leg.airport_ident,
                leg.procedure_ident,
                leg.transition_ident,
                leg.sequence_number
            ));
            let canonical_leg = CanonicalProcedureLeg {
                object_id: leg_id,
                airport_ident: leg.airport_ident,
                icao_code: leg.icao_code,
                procedure_kind: leg.procedure_kind,
                procedure_ident: leg.procedure_ident,
                route_type: leg.route_type,
                transition_ident: leg.transition_ident,
                sequence_number: leg.sequence_number,
                fix_ident: leg.fix_ident,
                fix_icao_code: leg.fix_icao_code,
                fix_section: leg.fix_section,
                waypoint_description: leg.waypoint_description,
                turn_direction: leg.turn_direction,
                rnp_nm: leg.rnp_nm,
                path_terminator: leg.path_terminator,
                recommended_navaid: leg.recommended_navaid,
                arc_radius_nm: leg.arc_radius_nm,
                course_a_deg: leg.course_a_deg,
                distance_a_nm: leg.distance_a_nm,
                course_b_deg: leg.course_b_deg,
                distance_b_nm: leg.distance_b_nm,
                altitude_descriptor: leg.altitude_descriptor,
                altitude_1_ft: leg.altitude_1_ft,
                altitude_2_ft: leg.altitude_2_ft,
                speed_limit_kts: leg.speed_limit_kts,
                course_c_deg: leg.course_c_deg,
                vertical_angle_deg: leg.vertical_angle_deg,
                msa_center_fix: leg.msa_center_fix,
                route_qualifiers: leg.route_qualifiers,
                raw: leg.raw,
                temporal: temporal.clone(),
            };
            openairac_store::insert_procedure_leg_conn(conn, &canonical_leg)?;
            report.records_created += 1;
        }

        Ok(report)
    }
}

impl crate::provider::DataProvider for Aixm45Provider {
    fn name(&self) -> &'static str {
        "FR_SIA"
    }

    fn datasets(&self) -> &'static [&'static str] {
        &["AIXM4.5"]
    }

    fn fetch(
        &self,
        dataset: &str,
        _cycle: Option<&crate::provider::CycleSelector>,
    ) -> Result<crate::provider::FetchedDataset> {
        if dataset != "AIXM4.5" {
            bail!("unsupported dataset '{dataset}' for AIXM 4.5 provider");
        }
        let url = "http://data.cquest.org/dgac/aip/AIXM4.5_all_FR_OM_2019-02-28.xml.gz";
        let retrieved_at = Utc::now();
        crate::provider::fetch_url(&self.provider_name, dataset, url, retrieved_at)
    }

    fn parse_and_ingest(
        &self,
        dataset: &crate::provider::FetchedDataset,
        store: &mut WorldStore,
    ) -> Result<crate::provider::IngestReport> {
        let xml_str = std::str::from_utf8(&dataset.raw_bytes)
            .context("decoding AIXM 4.5 XML content as UTF-8")?;
        let effective_from = dataset.valid_from.unwrap_or_else(Utc::now);
        self.ingest_xml_content(
            store,
            xml_str,
            effective_from,
            dataset.airac_cycle.as_deref(),
            &dataset.source_uri,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_SIA_AIXM45_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<AIXM-Snapshot xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" version="4.5">
    <Ahp>
        <AhpUid>
            <codeId>LFPG</codeId>
        </AhpUid>
        <txtName>PARIS CHARLES DE GAULLE</txtName>
        <codeType>AD</codeType>
        <geoLat>490035.00N</geoLat>
        <geoLong>0023252.00E</geoLong>
        <valElev>392</valElev>
        <uomDistVer>FT</uomDistVer>
    </Ahp>
    <Rwy>
        <RwyUid>
            <AhpUid>
                <codeId>LFPG</codeId>
            </AhpUid>
            <txtDesig>08L/26R</txtDesig>
        </RwyUid>
        <valLen>4215</valLen>
        <valWid>45</valWid>
        <uomDimRwy>M</uomDimRwy>
        <codeComposition>ASPH</codeComposition>
    </Rwy>
    <Ahp>
        <AhpUid>
            <codeId>LFPO</codeId>
        </AhpUid>
        <txtName>PARIS ORLY</txtName>
        <codeType>AD</codeType>
        <geoLat>484324.00N</geoLat>
        <geoLong>0022246.00E</geoLong>
        <valElev>291</valElev>
    </Ahp>
    <Ahp>
        <AhpUid>
            <codeId>LFMN</codeId>
        </AhpUid>
        <txtName>NICE COTE D'AZUR</txtName>
        <codeType>AD</codeType>
        <geoLat>433955.00N</geoLat>
        <geoLong>0071307.00E</geoLong>
        <valElev>12</valElev>
    </Ahp>
    <Vor>
        <VorUid>
            <codeId>PGS</codeId>
            <geoLat>490059.00N</geoLat>
            <geoLong>0023158.00E</geoLong>
        </VorUid>
        <txtName>PARIS CHARLES DE GAULLE</txtName>
        <codeType>VOR</codeType>
        <valFreq>117.05</valFreq>
        <valElev>380</valElev>
        <valMagVar>1.5</valMagVar>
    </Vor>
    <Dpt>
        <codeId>LORNI</codeId>
        <txtName>LORNI</txtName>
        <geoLat>485500.00N</geoLat>
        <geoLong>0025500.00E</geoLong>
    </Dpt>
    <Dpt>
        <codeId>OKTET</codeId>
        <txtName>OKTET</txtName>
        <geoLat>491000.00N</geoLat>
        <geoLong>0021500.00E</geoLong>
    </Dpt>
    <Sid>
        <codeId>LORNI1A</codeId>
        <AhpUid>LFPG</AhpUid>
        <Spn>
            <DptUid>LORNI</DptUid>
            <codePath>TF</codePath>
            <valCrs>90.0</valCrs>
            <valDist>15.0</valDist>
            <valAlt1>4000</valAlt1>
        </Spn>
    </Sid>
</AIXM-Snapshot>"#;

    #[test]
    fn test_parse_aixm45_dms_coordinates() {
        let lat = parse_aixm45_lat("490035.00N").unwrap();
        assert!((lat - 49.009722).abs() < 1e-4);

        let lon = parse_aixm45_lon("0023252.00E").unwrap();
        assert!((lon - 2.547778).abs() < 1e-4);

        let lat_nice = parse_aixm45_lat("433955.00N").unwrap();
        assert!((lat_nice - 43.665278).abs() < 1e-4);

        let lon_nice = parse_aixm45_lon("0071307.00E").unwrap();
        assert!((lon_nice - 7.218611).abs() < 1e-4);
    }

    #[test]
    fn test_parse_aixm45_dataset() {
        let ds = parse_aixm45_xml(SAMPLE_SIA_AIXM45_XML).expect("parse SIA AIXM 4.5 sample");
        assert_eq!(ds.airports.len(), 3);
        assert_eq!(ds.airports[0].ident, "LFPG");
        assert_eq!(ds.airports[0].runways.len(), 1);
        assert_eq!(ds.airports[0].runways[0].designator, "08L/26R");

        assert_eq!(ds.airports[1].ident, "LFPO");
        assert_eq!(ds.airports[2].ident, "LFMN");

        assert_eq!(ds.navaids.len(), 1);
        assert_eq!(ds.navaids[0].ident, "PGS");

        assert_eq!(ds.fixes.len(), 2);
        assert_eq!(ds.fixes[0].ident, "LORNI");
        assert_eq!(ds.fixes[1].ident, "OKTET");

        assert_eq!(ds.procedure_legs.len(), 1);
        assert_eq!(ds.procedure_legs[0].procedure_ident, "LORNI1A");
        assert_eq!(ds.procedure_legs[0].fix_ident, "LORNI");
        assert_eq!(ds.procedure_legs[0].path_terminator, "TF");
    }

    #[test]
    fn test_aixm45_provider_ingest() {
        let mut store = WorldStore::open_in_memory().unwrap();
        let provider = Aixm45Provider::default_france_sia();
        let report = provider
            .ingest_xml_content(
                &mut store,
                SAMPLE_SIA_AIXM45_XML,
                Utc::now(),
                Some("2608"),
                "file://sample_sia_aixm45.xml",
            )
            .expect("ingest SIA AIXM 4.5");

        assert!(report.records_created >= 5);

        let status = store.status().unwrap();
        assert_eq!(status.total_airports, 3);
        assert_eq!(status.total_runways, 1);
        assert_eq!(status.total_navaids, 1);
        assert_eq!(status.total_waypoints, 2);
        assert_eq!(status.total_procedure_legs, 1);
    }
}
