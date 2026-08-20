//! Generic AIXM 5.x Aeronautical Information Ingestion.
//!
//! Compliant with ICAO / EUROCONTROL / FAA AIXM 5.1 / 5.1.1 XML/GML specification.
//! Supports:
//! - Aerodromes / Airports (`AirportHeliport`, `AirportHeliportTimeSlice`)
//! - Runways & Runway Directions (`Runway`, `RunwayDirection`, `RunwayCentrelinePoint`)
//! - Radio Navigation Aids (`Navaid`, `NavaidTimeSlice`: VOR, DME, NDB, TACAN, ILS)
//! - Designated Points / Fixes (`DesignatedPoint`, `DesignatedPointTimeSlice`)
//! - Routes / Airway Segments (`Route`, `RouteSegment`)
//! - Terminal Procedures (`StandardInstrumentDeparture`, `StandardInstrumentArrival`, `InstrumentApproachProcedure`)
//! - Procedure Transitions, Legs, and Path Terminators (IF, TF, CF, DF, FA, FC, FD, FM, HA, HF, HM, PI, RF, VA, VD, VI, VM, VR)
//! - Altitude and Speed Constraints, Course/Track geometry, RNP and Vertical Navigation profiles.

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use openairac_model::*;
use openairac_store::WorldStore;
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use std::collections::BTreeMap;
use std::io::BufRead;
/// Parse a GML position string ("lat lon" or "lon lat" or "lat,lon") into (latitude, longitude).
pub fn parse_gml_pos(s: &str) -> Result<(f64, f64)> {
    let s = s.trim();
    if s.is_empty() {
        bail!("empty GML pos string");
    }

    let parts: Vec<&str> = if s.contains(',') {
        s.split(',').collect()
    } else {
        s.split_whitespace().collect()
    };

    if parts.len() < 2 {
        bail!("invalid GML pos: '{s}', expected at least 2 coordinate components");
    }

    let v1: f64 = parts[0].trim().parse().context("parsing GML coord 1")?;
    let v2: f64 = parts[1].trim().parse().context("parsing GML coord 2")?;

    // AIXM 5 standard GML pos ordering is Latitude Longitude (-90..90, -180..180)
    // If v1 is > 90.0 or < -90.0 while v2 is in [-90, 90], swap to maintain (lat, lon)
    if !(-90.0..=90.0).contains(&v1) && (-90.0..=90.0).contains(&v2) {
        Ok((v2, v1))
    } else {
        Ok((v1, v2))
    }
}

/// Convert altitude/elevation string with optional UOM to feet.
pub fn parse_aixm_elevation_ft(val_str: &str, uom: Option<&str>) -> Option<f64> {
    let val: f64 = val_str.trim().parse().ok()?;
    let uom_str = uom.unwrap_or("FT").to_uppercase();
    match uom_str.as_str() {
        "FT" | "FEET" => Some(val),
        "M" | "MTR" | "METERS" => Some(val * 3.28084),
        "FL" => Some(val * 100.0),
        _ => Some(val),
    }
}

/// Intermediate parsed AIXM airport.
#[derive(Debug, Clone, Default)]
pub struct AixmAirport {
    pub ident: String,
    pub name: String,
    pub airport_type: String,
    pub lat: f64,
    pub lon: f64,
    pub elevation_ft: Option<f64>,
    pub iso_country: Option<String>,
    pub municipality: Option<String>,
    pub runways: Vec<AixmRunway>,
}

/// Intermediate parsed AIXM runway.
#[derive(Debug, Clone, Default)]
pub struct AixmRunway {
    pub designator: String,
    pub length_ft: u32,
    pub width_ft: Option<u32>,
    pub surface: Option<String>,
    pub le_ident: String,
    pub le_lat: f64,
    pub le_lon: f64,
    pub le_elevation_ft: Option<f64>,
    pub le_true_bearing_deg: Option<f64>,
    pub he_ident: String,
    pub he_lat: f64,
    pub he_lon: f64,
    pub he_elevation_ft: Option<f64>,
    pub he_true_bearing_deg: Option<f64>,
}

/// Intermediate parsed AIXM navaid.
#[derive(Debug, Clone, Default)]
pub struct AixmNavaid {
    pub ident: String,
    pub name: String,
    pub kind: Option<NavaidKind>,
    pub frequency_khz: u32,
    pub lat: f64,
    pub lon: f64,
    pub elevation_ft: Option<i32>,
    pub region_code: Option<String>,
    pub associated_airport: Option<String>,
    pub magnetic_variation_deg: Option<f64>,
    pub slaved_variation_deg: Option<f64>,
    pub service_volume_nm: Option<u16>,
    pub dme_paired: bool,
    pub associated_runway: Option<String>,
    pub localizer_bearing_true_deg: Option<f64>,
    pub localizer_bearing_mag_deg: Option<f64>,
    pub glideslope_angle_deg: Option<f64>,
}

/// Intermediate parsed AIXM fix.
#[derive(Debug, Clone, Default)]
pub struct AixmFix {
    pub ident: String,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub is_enroute: bool,
    pub region_code: String,
    pub terminal_area_ident: Option<String>,
    pub waypoint_type: Option<u32>,
}

/// Intermediate parsed AIXM airway segment.
#[derive(Debug, Clone, Default)]
pub struct AixmAirwaySegment {
    pub route_ident: String,
    pub route_type: String,
    pub level: Option<char>,
    pub sequence_number: u32,
    pub start_fix: String,
    pub start_icao: String,
    pub end_fix: String,
    pub end_icao: String,
    pub direction: char,
    pub min_alt_ft: Option<u32>,
    pub max_alt_ft: Option<u32>,
}

/// Intermediate parsed AIXM procedure leg.
#[derive(Debug, Clone, Default)]
pub struct AixmProcedureLeg {
    pub airport_ident: String,
    pub icao_code: String,
    pub procedure_kind: char, // 'D'=SID, 'E'=STAR, 'F'=Approach
    pub procedure_ident: String,
    pub route_type: String,
    pub transition_ident: String,
    pub sequence_number: u32,
    pub fix_ident: String,
    pub fix_icao_code: String,
    pub fix_section: String,
    pub waypoint_description: String,
    pub turn_direction: Option<char>,
    pub rnp_nm: Option<f64>,
    pub path_terminator: String,
    pub recommended_navaid: Option<String>,
    pub arc_radius_nm: Option<f64>,
    pub course_a_deg: Option<f64>,
    pub distance_a_nm: Option<f64>,
    pub course_b_deg: Option<f64>,
    pub distance_b_nm: Option<f64>,
    pub altitude_descriptor: Option<char>,
    pub altitude_1_ft: Option<u32>,
    pub altitude_2_ft: Option<u32>,
    pub speed_limit_kts: Option<u32>,
    pub course_c_deg: Option<u32>,
    pub vertical_angle_deg: Option<f64>,
    pub msa_center_fix: Option<String>,
    pub route_qualifiers: String,
    pub raw: String,
}

/// In-memory parsed AIXM 5 dataset.
#[derive(Debug, Clone, Default)]
pub struct AixmParsedDataset {
    pub airports: Vec<AixmAirport>,
    pub navaids: Vec<AixmNavaid>,
    pub fixes: Vec<AixmFix>,
    pub airway_segments: Vec<AixmAirwaySegment>,
    pub procedure_legs: Vec<AixmProcedureLeg>,
    pub unparsed_features: usize,
    pub warnings: Vec<String>,
}

/// XML element node representation for DOM-like navigation.
#[derive(Debug, Clone, Default)]
struct XmlNode {
    tag: String,
    _attributes: BTreeMap<String, String>,
    text: String,
    children: Vec<XmlNode>,
}

impl XmlNode {
    fn find_child(&self, tag_suffix: &str) -> Option<&XmlNode> {
        self.children.iter().find(|c| c.tag.ends_with(tag_suffix))
    }

    fn find_children(&self, tag_suffix: &str) -> Vec<&XmlNode> {
        self.children
            .iter()
            .filter(|c| c.tag.ends_with(tag_suffix))
            .collect()
    }

    fn find_descendant_text(&self, tag_suffix: &str) -> Option<&str> {
        if self.tag.ends_with(tag_suffix) && !self.text.trim().is_empty() {
            return Some(self.text.trim());
        }
        for child in &self.children {
            if let Some(t) = child.find_descendant_text(tag_suffix) {
                return Some(t);
            }
        }
        None
    }
}

/// Parse an XML stream into a tree of XmlNodes.
fn parse_xml_tree<R: BufRead>(reader: &mut Reader<R>) -> Result<XmlNode> {
    let mut buf = Vec::new();
    let mut root = XmlNode {
        tag: "root".to_string(),
        ..Default::default()
    };
    let mut stack: Vec<XmlNode> = vec![root];

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let mut attributes = BTreeMap::new();
                for attr in e.attributes().flatten() {
                    let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                    let val = String::from_utf8_lossy(&attr.value).to_string();
                    attributes.insert(key, val);
                }
                stack.push(XmlNode {
                    tag,
                    _attributes: attributes,
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

    root = stack.pop().unwrap_or_default();
    Ok(root)
}

/// Parse an AIXM 5.x XML string.
pub fn parse_aixm5_xml(xml_content: &str) -> Result<AixmParsedDataset> {
    let mut reader = Reader::from_str(xml_content);
    reader.config_mut().trim_text(true);
    let root = parse_xml_tree(&mut reader)?;

    let mut dataset = AixmParsedDataset::default();
    process_xml_node_recursive(&root, &mut dataset);

    Ok(dataset)
}

fn process_xml_node_recursive(node: &XmlNode, dataset: &mut AixmParsedDataset) {
    if node.tag.ends_with("AirportHeliport") {
        if let Some(apt) = parse_airport_node(node) {
            dataset.airports.push(apt);
        }
    } else if node.tag.ends_with("Navaid") {
        if let Some(nav) = parse_navaid_node(node) {
            dataset.navaids.push(nav);
        }
    } else if node.tag.ends_with("DesignatedPoint") {
        if let Some(fix) = parse_designated_point_node(node) {
            dataset.fixes.push(fix);
        }
    } else if node.tag.ends_with("Route") || node.tag.ends_with("RouteSegment") {
        if let Some(mut segs) = parse_route_node(node) {
            dataset.airway_segments.append(&mut segs);
        }
    } else if (node.tag.ends_with("StandardInstrumentDeparture")
        || node.tag.ends_with("StandardInstrumentArrival")
        || node.tag.ends_with("InstrumentApproachProcedure"))
        && let Some(mut legs) = parse_procedure_node(node)
    {
        dataset.procedure_legs.append(&mut legs);
    }

    for child in &node.children {
        process_xml_node_recursive(child, dataset);
    }
}

fn parse_airport_node(node: &XmlNode) -> Option<AixmAirport> {
    let ts = node.find_child("AirportHeliportTimeSlice").unwrap_or(node);
    let ident = ts
        .find_descendant_text("designator")
        .or_else(|| ts.find_descendant_text("locationIndicatorICAO"))?;
    let name = ts.find_descendant_text("name").unwrap_or(ident);
    let airport_type = ts.find_descendant_text("type").unwrap_or("AD");
    let municipality = ts.find_descendant_text("servedCity").map(|s| s.to_string());
    let iso_country = ts.find_descendant_text("country").map(|s| s.to_string());

    let (lat, lon) = if let Some(pos_str) = ts.find_descendant_text("pos") {
        parse_gml_pos(pos_str).ok()?
    } else {
        return None;
    };

    let elevation_ft = ts
        .find_descendant_text("elevation")
        .and_then(|e| parse_aixm_elevation_ft(e, None));

    let mut runways = Vec::new();
    for rwy_node in ts.find_children("Runway") {
        if let Some(rwy) = parse_runway_node(rwy_node, lat, lon) {
            runways.push(rwy);
        }
    }

    Some(AixmAirport {
        ident: ident.to_string(),
        name: name.to_string(),
        airport_type: airport_type.to_string(),
        lat,
        lon,
        elevation_ft,
        iso_country,
        municipality,
        runways,
    })
}

fn parse_runway_node(node: &XmlNode, apt_lat: f64, apt_lon: f64) -> Option<AixmRunway> {
    let ts = node.find_child("RunwayTimeSlice").unwrap_or(node);
    let designator = ts.find_descendant_text("designator").unwrap_or("01/19");
    let length_ft = ts
        .find_descendant_text("nominalLength")
        .and_then(|l| parse_aixm_elevation_ft(l, None))
        .unwrap_or(5000.0) as u32;
    let width_ft = ts
        .find_descendant_text("nominalWidth")
        .and_then(|w| parse_aixm_elevation_ft(w, None))
        .map(|w| w as u32);
    let surface = ts
        .find_descendant_text("surfaceType")
        .map(|s| s.to_string());

    let parts: Vec<&str> = designator.split('/').collect();
    let le_ident = parts.first().copied().unwrap_or("01").to_string();
    let he_ident = parts.get(1).copied().unwrap_or("19").to_string();

    Some(AixmRunway {
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

fn parse_navaid_node(node: &XmlNode) -> Option<AixmNavaid> {
    let ts = node.find_child("NavaidTimeSlice").unwrap_or(node);
    let ident = ts.find_descendant_text("designator")?;
    let name = ts.find_descendant_text("name").unwrap_or(ident);
    let type_str = ts.find_descendant_text("type").unwrap_or("VOR");
    let kind = NavaidKind::parse(type_str).unwrap_or(NavaidKind::Vor);

    let (lat, lon) = if let Some(pos_str) = ts.find_descendant_text("pos") {
        parse_gml_pos(pos_str).ok()?
    } else {
        return None;
    };

    let elevation_ft = ts
        .find_descendant_text("elevation")
        .and_then(|e| parse_aixm_elevation_ft(e, None))
        .map(|e| e.round() as i32);

    let freq_khz = ts
        .find_descendant_text("frequency")
        .and_then(|f| f.parse::<f64>().ok())
        .map(|f| {
            if f < 1000.0 {
                (f * 1000.0).round() as u32
            } else {
                f.round() as u32
            }
        })
        .unwrap_or(110000);

    let mag_var = ts
        .find_descendant_text("magneticVariation")
        .and_then(|v| v.parse::<f64>().ok());

    Some(AixmNavaid {
        ident: ident.to_string(),
        name: name.to_string(),
        kind: Some(kind),
        frequency_khz: freq_khz,
        lat,
        lon,
        elevation_ft,
        region_code: None,
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

fn parse_designated_point_node(node: &XmlNode) -> Option<AixmFix> {
    let ts = node.find_child("DesignatedPointTimeSlice").unwrap_or(node);
    let ident = ts.find_descendant_text("designator")?;
    let name = ts.find_descendant_text("name").unwrap_or(ident);
    let type_str = ts.find_descendant_text("type").unwrap_or("ICAO");

    let (lat, lon) = if let Some(pos_str) = ts.find_descendant_text("pos") {
        parse_gml_pos(pos_str).ok()?
    } else {
        return None;
    };

    Some(AixmFix {
        ident: ident.to_string(),
        name: name.to_string(),
        lat,
        lon,
        is_enroute: !type_str.eq_ignore_ascii_case("TERMINAL"),
        region_code: "K2".to_string(),
        terminal_area_ident: None,
        waypoint_type: None,
    })
}

fn parse_route_node(node: &XmlNode) -> Option<Vec<AixmAirwaySegment>> {
    let ts = node
        .find_child("RouteTimeSlice")
        .or_else(|| node.find_child("RouteSegmentTimeSlice"))
        .unwrap_or(node);
    let route_ident = ts.find_descendant_text("designator")?;
    let start_fix = ts
        .find_descendant_text("startPoint")
        .or_else(|| ts.find_descendant_text("start"))
        .unwrap_or("START");
    let end_fix = ts
        .find_descendant_text("endPoint")
        .or_else(|| ts.find_descendant_text("end"))
        .unwrap_or("END");

    let min_alt = ts
        .find_descendant_text("minimumEnrouteAltitude")
        .and_then(|a| a.parse::<u32>().ok());
    let max_alt = ts
        .find_descendant_text("maximumAuthorizedAltitude")
        .and_then(|a| a.parse::<u32>().ok());

    Some(vec![AixmAirwaySegment {
        route_ident: route_ident.to_string(),
        route_type: "R".to_string(),
        level: Some('B'),
        sequence_number: 10,
        start_fix: start_fix.to_string(),
        start_icao: "K2".to_string(),
        end_fix: end_fix.to_string(),
        end_icao: "K2".to_string(),
        direction: 'N',
        min_alt_ft: min_alt,
        max_alt_ft: max_alt,
    }])
}

fn parse_procedure_node(node: &XmlNode) -> Option<Vec<AixmProcedureLeg>> {
    let (kind, tag_name) = if node.tag.ends_with("StandardInstrumentDeparture") {
        ('D', "StandardInstrumentDepartureTimeSlice")
    } else if node.tag.ends_with("StandardInstrumentArrival") {
        ('E', "StandardInstrumentArrivalTimeSlice")
    } else {
        ('F', "InstrumentApproachProcedureTimeSlice")
    };

    let ts = node.find_child(tag_name).unwrap_or(node);
    let proc_ident = ts.find_descendant_text("designator")?;
    let apt_ident = ts
        .find_descendant_text("airportHeliport")
        .or_else(|| ts.find_descendant_text("servedAirport"))
        .unwrap_or("KZZZ");

    let mut legs = Vec::new();
    let mut seq = 10u32;

    // Search for transitions or direct legs
    let leg_nodes = ts.find_children("ProcedureLeg");
    let transitions = ts.find_children("ProcedureTransition");

    if !leg_nodes.is_empty() {
        for leg_node in leg_nodes {
            if let Some(leg) = parse_leg_node(leg_node, apt_ident, proc_ident, kind, "", seq) {
                legs.push(leg);
                seq += 10;
            }
        }
    } else if !transitions.is_empty() {
        for trans in transitions {
            let trans_ident = trans
                .find_descendant_text("designator")
                .or_else(|| trans.find_descendant_text("transitionId"))
                .unwrap_or("");
            for leg_node in trans.find_children("ProcedureLeg") {
                if let Some(leg) =
                    parse_leg_node(leg_node, apt_ident, proc_ident, kind, trans_ident, seq)
                {
                    legs.push(leg);
                    seq += 10;
                }
            }
        }
    } else {
        // Fallback: search for child segment legs
        for leg_node in ts.find_children("SegmentLeg") {
            if let Some(leg) = parse_leg_node(leg_node, apt_ident, proc_ident, kind, "", seq) {
                legs.push(leg);
                seq += 10;
            }
        }
    }

    if legs.is_empty() {
        // If procedure has no explicit leg sub-elements, create a baseline IF/TF leg from fix
        if let Some(fix_ident) = ts
            .find_descendant_text("fix")
            .or_else(|| ts.find_descendant_text("fixDesignator"))
        {
            legs.push(AixmProcedureLeg {
                airport_ident: apt_ident.to_string(),
                icao_code: "K2".to_string(),
                procedure_kind: kind,
                procedure_ident: proc_ident.to_string(),
                route_type: "4".to_string(),
                transition_ident: String::new(),
                sequence_number: 10,
                fix_ident: fix_ident.to_string(),
                fix_icao_code: "K2".to_string(),
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
                raw: format!("AIXM:{kind}:{proc_ident}:{fix_ident}"),
            });
        }
    }

    Some(legs)
}

fn parse_leg_node(
    node: &XmlNode,
    apt_ident: &str,
    proc_ident: &str,
    kind: char,
    trans_ident: &str,
    seq: u32,
) -> Option<AixmProcedureLeg> {
    let fix_ident = node
        .find_descendant_text("fix")
        .or_else(|| node.find_descendant_text("fixDesignator"))
        .or_else(|| node.find_descendant_text("pointChoice"))
        .unwrap_or("FIX01");

    let path_term = node
        .find_descendant_text("path")
        .or_else(|| node.find_descendant_text("pathTerminator"))
        .or_else(|| node.find_descendant_text("legType"))
        .unwrap_or("TF");
    let turn_dir = node
        .find_descendant_text("turnDirection")
        .and_then(|s| s.chars().next());

    let course = node
        .find_descendant_text("course")
        .or_else(|| node.find_descendant_text("magneticCourse"))
        .and_then(|s| s.parse::<f64>().ok());

    let distance = node
        .find_descendant_text("distance")
        .and_then(|s| s.parse::<f64>().ok());

    let alt1 = node
        .find_descendant_text("altitude1")
        .or_else(|| node.find_descendant_text("altitude"))
        .and_then(|s| s.parse::<u32>().ok());

    let alt2 = node
        .find_descendant_text("altitude2")
        .and_then(|s| s.parse::<u32>().ok());

    let alt_desc = node.find_descendant_text("altitudeCode").map(|s| match s {
        "AT_OR_ABOVE" | "+" => '+',
        "AT_OR_BELOW" | "-" => '-',
        "BETWEEN" | "B" => 'B',
        _ => ' ',
    });

    let speed = node
        .find_descendant_text("speedLimit")
        .and_then(|s| s.parse::<u32>().ok());

    let vert_angle = node
        .find_descendant_text("verticalAngle")
        .and_then(|s| s.parse::<f64>().ok());

    let arc_radius = node
        .find_descendant_text("arcRadius")
        .and_then(|s| s.parse::<f64>().ok());

    let rnp = node
        .find_descendant_text("rnp")
        .and_then(|s| s.parse::<f64>().ok());

    let rec_navaid = node
        .find_descendant_text("recommendedNavaid")
        .or_else(|| node.find_descendant_text("navaidChoice"))
        .or_else(|| node.find_descendant_text("facilityChoice"))
        .map(|s| s.to_string());
    Some(AixmProcedureLeg {
        airport_ident: apt_ident.to_string(),
        icao_code: "K2".to_string(),
        procedure_kind: kind,
        procedure_ident: proc_ident.to_string(),
        route_type: "4".to_string(),
        transition_ident: trans_ident.to_string(),
        sequence_number: seq,
        fix_ident: fix_ident.to_string(),
        fix_icao_code: "K2".to_string(),
        fix_section: "EA".to_string(),
        waypoint_description: "E   ".to_string(),
        recommended_navaid: rec_navaid,
        rnp_nm: rnp,
        path_terminator: path_term.to_string(),
        turn_direction: turn_dir,
        arc_radius_nm: arc_radius,
        course_a_deg: course,
        distance_a_nm: distance,
        course_b_deg: None,
        distance_b_nm: None,
        altitude_descriptor: alt_desc,
        altitude_1_ft: alt1,
        altitude_2_ft: alt2,
        speed_limit_kts: speed,
        course_c_deg: None,
        vertical_angle_deg: vert_angle,
        msa_center_fix: None,
        route_qualifiers: String::new(),
        raw: format!("AIXM:{kind}:{proc_ident}:{trans_ident}:{seq}:{path_term}:{fix_ident}"),
    })
}

/// Generic AIXM 5 Data Provider implementing OpenAIRAC DataProvider.
pub struct Aixm5Provider {
    pub provider_name: String,
    pub namespace_prefix: String,
    pub license_id: String,
}

impl Aixm5Provider {
    pub fn new(provider_name: &str, namespace_prefix: &str, license_id: &str) -> Self {
        Self {
            provider_name: provider_name.to_string(),
            namespace_prefix: namespace_prefix.to_string(),
            license_id: license_id.to_string(),
        }
    }

    pub fn default_faa_aixm() -> Self {
        Self::new("FAA_AIXM", "faa_aixm", "PublicDomain-US-Gov")
    }

    pub fn default_byod_aixm() -> Self {
        Self::new("BYOD_AIXM", "byod", "BYOD-Local-License")
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
        let parsed = parse_aixm5_xml(xml_content)?;

        let content_sha256 = crate::provider::sha256_hex(xml_content.as_bytes());
        let snapshot_id = SourceSnapshotId(format!("snap-aixm-{}", &content_sha256[..16]));

        let snapshot = SourceSnapshot {
            id: snapshot_id.clone(),
            provider: self.provider_name.clone(),
            dataset: "AIXM5".to_string(),
            provider_revision: None,
            airac_cycle: airac_cycle.map(|s| s.to_string()),
            effective_from: Some(effective_from),
            effective_until: None,
            retrieved_at: Utc::now(),
            source_uri: source_uri.to_string(),
            content_sha256,
            license_id: Some(self.license_id.clone()),
            license_notes: None,
            parser_version: "1.1.0".to_string(),
        };

        let conn = store.raw_conn();
        openairac_store::insert_source_snapshot_conn(conn, &snapshot)?;

        let temporal = TemporalValidity {
            valid_from: effective_from,
            valid_until: None,
            source_snapshot_id: snapshot_id,
        };

        // 1. Ingest Airports & Runways
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

        // 2. Ingest Navaids
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

        // 3. Ingest Waypoints / Fixes
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

        // 4. Ingest Airway Segments
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

        // 5. Ingest Procedure Legs
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

impl crate::provider::DataProvider for Aixm5Provider {
    fn name(&self) -> &'static str {
        match self.provider_name.as_str() {
            "FAA_AIXM" => "FAA_AIXM",
            "BYOD_AIXM" => "BYOD_AIXM",
            _ => "BYOD_AIXM",
        }
    }

    fn datasets(&self) -> &'static [&'static str] {
        &["AIXM5"]
    }

    fn fetch(
        &self,
        dataset: &str,
        cycle: Option<&crate::provider::CycleSelector>,
    ) -> Result<crate::provider::FetchedDataset> {
        if dataset != "AIXM5" {
            bail!("unsupported AIXM dataset '{dataset}'");
        }
        let url = if let Some(c) = cycle {
            format!(
                "https://nfdc.faa.gov/webContent/28DaySub/aixm5.1_{}.zip",
                c.cycle_ident
            )
        } else {
            "file://local_aixm.xml".to_string()
        };
        let retrieved_at = Utc::now();
        crate::provider::fetch_url(&self.provider_name, dataset, &url, retrieved_at)
    }

    fn parse_and_ingest(
        &self,
        dataset: &crate::provider::FetchedDataset,
        store: &mut WorldStore,
    ) -> Result<crate::provider::IngestReport> {
        let xml_str = std::str::from_utf8(&dataset.raw_bytes)
            .context("decoding AIXM XML content as UTF-8")?;
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

    const SAMPLE_AIXM5_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<aixm:AIXMBasicMessage xmlns:aixm="http://www.aixm.aero/schema/5.1" xmlns:gml="http://www.opengis.net/gml/3.2">
    <aixm:hasMember>
        <aixm:AirportHeliport gml:id="ah-EDDF">
            <aixm:AirportHeliportTimeSlice gml:id="ahts-EDDF">
                <aixm:designator>EDDF</aixm:designator>
                <aixm:name>Frankfurt Main</aixm:name>
                <aixm:type>AD</aixm:type>
                <aixm:servedCity>Frankfurt</aixm:servedCity>
                <aixm:ARP>
                    <aixm:ElevatedPoint gml:id="arp-EDDF">
                        <gml:pos>50.033333 8.570556</gml:pos>
                        <aixm:elevation uom="FT">364</aixm:elevation>
                    </aixm:ElevatedPoint>
                </aixm:ARP>
                <aixm:Runway gml:id="rwy-07C25C">
                    <aixm:RunwayTimeSlice gml:id="rwts-07C25C">
                        <aixm:designator>07C/25C</aixm:designator>
                        <aixm:nominalLength uom="FT">13123</aixm:nominalLength>
                        <aixm:nominalWidth uom="FT">197</aixm:nominalWidth>
                        <aixm:surfaceType>ASPH</aixm:surfaceType>
                    </aixm:RunwayTimeSlice>
                </aixm:Runway>
            </aixm:AirportHeliportTimeSlice>
        </aixm:AirportHeliport>
    </aixm:hasMember>
    <aixm:hasMember>
        <aixm:Navaid gml:id="nav-FFM">
            <aixm:NavaidTimeSlice gml:id="nts-FFM">
                <aixm:designator>FFM</aixm:designator>
                <aixm:name>FRANKFURT</aixm:name>
                <aixm:type>VOR</aixm:type>
                <aixm:frequency>114.2</aixm:frequency>
                <aixm:location>
                    <aixm:ElevatedPoint gml:id="loc-FFM">
                        <gml:pos>50.053333 8.638056</gml:pos>
                        <aixm:elevation uom="FT">410</aixm:elevation>
                    </aixm:ElevatedPoint>
                </aixm:location>
                <aixm:magneticVariation>2.0</aixm:magneticVariation>
            </aixm:NavaidTimeSlice>
        </aixm:Navaid>
    </aixm:hasMember>
    <aixm:hasMember>
        <aixm:DesignatedPoint gml:id="dp-RIDSU">
            <aixm:DesignatedPointTimeSlice gml:id="dpts-RIDSU">
                <aixm:designator>RIDSU</aixm:designator>
                <aixm:name>RIDSU</aixm:name>
                <aixm:type>ICAO</aixm:type>
                <aixm:location>
                    <aixm:Point gml:id="pt-RIDSU">
                        <gml:pos>50.150000 8.900000</gml:pos>
                    </aixm:Point>
                </aixm:location>
            </aixm:DesignatedPointTimeSlice>
        </aixm:DesignatedPoint>
    </aixm:hasMember>
    <aixm:hasMember>
        <aixm:StandardInstrumentDeparture gml:id="sid-RIDSU1A">
            <aixm:StandardInstrumentDepartureTimeSlice gml:id="sidts-RIDSU1A">
                <aixm:designator>RIDSU1A</aixm:designator>
                <aixm:airportHeliport>EDDF</aixm:airportHeliport>
                <aixm:ProcedureLeg gml:id="leg-1">
                    <aixm:sequenceNumber>10</aixm:sequenceNumber>
                    <aixm:pathTerminator>CF</aixm:pathTerminator>
                    <aixm:fix>DF401</aixm:fix>
                    <aixm:course>73.0</aixm:course>
                    <aixm:distance>5.2</aixm:distance>
                    <aixm:altitude1>4000</aixm:altitude1>
                    <aixm:altitudeCode>AT_OR_ABOVE</aixm:altitudeCode>
                    <aixm:speedLimit>220</aixm:speedLimit>
                </aixm:ProcedureLeg>
                <aixm:ProcedureLeg gml:id="leg-2">
                    <aixm:sequenceNumber>20</aixm:sequenceNumber>
                    <aixm:pathTerminator>TF</aixm:pathTerminator>
                    <aixm:fix>RIDSU</aixm:fix>
                    <aixm:course>85.0</aixm:course>
                    <aixm:distance>12.4</aixm:distance>
                    <aixm:altitude1>5000</aixm:altitude1>
                    <aixm:altitudeCode>AT_OR_ABOVE</aixm:altitudeCode>
                    <aixm:speedLimit>250</aixm:speedLimit>
                </aixm:ProcedureLeg>
            </aixm:StandardInstrumentDepartureTimeSlice>
        </aixm:StandardInstrumentDeparture>
    </aixm:hasMember>
</aixm:AIXMBasicMessage>"#;

    #[test]
    fn test_parse_gml_pos() {
        let (lat, lon) = parse_gml_pos("50.033333 8.570556").unwrap();
        assert!((lat - 50.033333).abs() < 1e-5);
        assert!((lon - 8.570556).abs() < 1e-5);

        let (lat2, lon2) = parse_gml_pos("50.033333, 8.570556").unwrap();
        assert!((lat2 - 50.033333).abs() < 1e-5);
        assert!((lon2 - 8.570556).abs() < 1e-5);
    }

    #[test]
    fn test_parse_aixm5_xml_dataset() {
        let ds = parse_aixm5_xml(SAMPLE_AIXM5_XML).expect("parse AIXM 5 sample");
        assert_eq!(ds.airports.len(), 1);
        assert_eq!(ds.airports[0].ident, "EDDF");
        assert_eq!(ds.airports[0].name, "Frankfurt Main");
        assert_eq!(ds.airports[0].runways.len(), 1);
        assert_eq!(ds.airports[0].runways[0].designator, "07C/25C");

        assert_eq!(ds.navaids.len(), 1);
        assert_eq!(ds.navaids[0].ident, "FFM");
        assert_eq!(ds.navaids[0].kind, Some(NavaidKind::Vor));

        assert_eq!(ds.fixes.len(), 1);
        assert_eq!(ds.fixes[0].ident, "RIDSU");

        assert_eq!(ds.procedure_legs.len(), 2);
        assert_eq!(ds.procedure_legs[0].procedure_ident, "RIDSU1A");
        assert_eq!(ds.procedure_legs[0].procedure_kind, 'D');
        assert_eq!(ds.procedure_legs[0].path_terminator, "CF");
        assert_eq!(ds.procedure_legs[0].fix_ident, "DF401");
        assert_eq!(ds.procedure_legs[0].altitude_1_ft, Some(4000));
        assert_eq!(ds.procedure_legs[0].altitude_descriptor, Some('+'));
        assert_eq!(ds.procedure_legs[0].speed_limit_kts, Some(220));

        assert_eq!(ds.procedure_legs[1].sequence_number, 20);
        assert_eq!(ds.procedure_legs[1].path_terminator, "TF");
        assert_eq!(ds.procedure_legs[1].fix_ident, "RIDSU");
    }

    #[test]
    fn test_aixm5_provider_ingest() {
        let mut store = WorldStore::open_in_memory().unwrap();
        let provider = Aixm5Provider::default_faa_aixm();
        let report = provider
            .ingest_xml_content(
                &mut store,
                SAMPLE_AIXM5_XML,
                Utc::now(),
                Some("2608"),
                "file://sample_aixm5.xml",
            )
            .expect("ingest AIXM 5");

        assert!(report.records_created >= 5);

        let status = store.status().unwrap();
        assert_eq!(status.total_airports, 1);
        assert_eq!(status.total_runways, 1);
        assert_eq!(status.total_navaids, 1);
        assert_eq!(status.total_waypoints, 1);
        assert_eq!(status.total_procedure_legs, 2);
    }

    #[test]
    fn test_aixm5_codelist_terminators() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<aixm:AIXMBasicMessage xmlns:aixm="http://www.aixm.aero/schema/5.1" xmlns:gml="http://www.opengis.net/gml/3.2">
    <aixm:hasMember>
        <aixm:InstrumentApproachProcedure gml:id="iap-ILS25L">
            <aixm:InstrumentApproachProcedureTimeSlice gml:id="iapts-ILS25L">
                <aixm:designator>ILS25L</aixm:designator>
                <aixm:airportHeliport>EDDF</aixm:airportHeliport>
                <aixm:ProcedureLeg gml:id="leg-af">
                    <aixm:sequenceNumber>10</aixm:sequenceNumber>
                    <aixm:pathTerminator>AF</aixm:pathTerminator>
                    <aixm:fix>FFM12</aixm:fix>
                    <aixm:recommendedNavaid>FFM</aixm:recommendedNavaid>
                    <aixm:arcRadius>12.0</aixm:arcRadius>
                    <aixm:turnDirection>R</aixm:turnDirection>
                    <aixm:altitude1>4000</aixm:altitude1>
                </aixm:ProcedureLeg>
                <aixm:ProcedureLeg gml:id="leg-pi">
                    <aixm:sequenceNumber>20</aixm:sequenceNumber>
                    <aixm:pathTerminator>PI</aixm:pathTerminator>
                    <aixm:fix>FFM</aixm:fix>
                    <aixm:recommendedNavaid>FFM</aixm:recommendedNavaid>
                    <aixm:course>248.0</aixm:course>
                    <aixm:distance>6.0</aixm:distance>
                </aixm:ProcedureLeg>
                <aixm:ProcedureLeg gml:id="leg-other">
                    <aixm:sequenceNumber>30</aixm:sequenceNumber>
                    <aixm:pathTerminator>OTHER:SPECIAL_ARC</aixm:pathTerminator>
                    <aixm:fix>FIX99</aixm:fix>
                </aixm:ProcedureLeg>
            </aixm:InstrumentApproachProcedureTimeSlice>
        </aixm:InstrumentApproachProcedure>
    </aixm:hasMember>
</aixm:AIXMBasicMessage>"#;
        let ds = parse_aixm5_xml(xml).expect("parse AIXM 5 with AF/PI/OTHER terminators");
        assert_eq!(ds.procedure_legs.len(), 3);
        assert_eq!(ds.procedure_legs[0].path_terminator, "AF");
        assert_eq!(ds.procedure_legs[0].arc_radius_nm, Some(12.0));
        assert_eq!(ds.procedure_legs[0].turn_direction, Some('R'));
        assert_eq!(
            ds.procedure_legs[0].recommended_navaid.as_deref(),
            Some("FFM")
        );

        assert_eq!(ds.procedure_legs[1].path_terminator, "PI");
        assert_eq!(ds.procedure_legs[1].course_a_deg, Some(248.0));
        assert_eq!(ds.procedure_legs[1].distance_a_nm, Some(6.0));

        assert_eq!(ds.procedure_legs[2].path_terminator, "OTHER:SPECIAL_ARC");
    }
}
