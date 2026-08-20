//! Official VATSIM Data API v3 Provider.
//!
//! Ingests real-time network status, live pilots, ATC controllers, ATIS,
//! prefiles, and server information from `https://data.vatsim.net/v3/vatsim-data.json`.

use crate::model::{
    FacilityType, NetworkFreshness, NetworkSnapshot, OnlineAtis, OnlineController, OnlineEvent,
    OnlinePilot, OnlinePrefile, OnlineServer,
};
use crate::provider::OnlineNetworkProvider;
use crate::sanitize::{
    sanitize_callsign, sanitize_icao, sanitize_text, validate_altitude_ft, validate_groundspeed_kt,
    validate_heading_deg, validate_lat_lon,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;

pub const VATSIM_DATA_URL: &str = "https://data.vatsim.net/v3/vatsim-data.json";

/// VATSIM official online network data provider.
pub struct VatsimProvider {
    data_url: String,
    events_url: String,
}

impl Default for VatsimProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl VatsimProvider {
    pub fn new() -> Self {
        Self {
            data_url: VATSIM_DATA_URL.to_string(),
            events_url: super::vatsim_events::VATSIM_EVENTS_URL.to_string(),
        }
    }

    pub fn with_urls(data_url: impl Into<String>, events_url: impl Into<String>) -> Self {
        Self {
            data_url: data_url.into(),
            events_url: events_url.into(),
        }
    }

    /// Parse a raw VATSIM JSON string into a structured `NetworkSnapshot`.
    pub fn parse_vatsim_json(
        raw_json: &str,
        received_at: DateTime<Utc>,
    ) -> Result<NetworkSnapshot> {
        let v: Value = serde_json::from_str(raw_json).context("parsing VATSIM Data API JSON")?;

        let general_info = &v["general"];
        let gen_time_str = general_info["update_timestamp"]
            .as_str()
            .or_else(|| general_info["update"].as_str())
            .unwrap_or("");

        let generated_at = DateTime::parse_from_rfc3339(gen_time_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or(received_at);

        let connected_clients = general_info["connected_clients"].as_u64().unwrap_or(0) as u32;
        let mut snapshot = NetworkSnapshot::new("VATSIM", generated_at, received_at);
        snapshot.connected_clients = connected_clients;

        // 1. Parse Pilots
        if let Some(pilots_arr) = v["pilots"].as_array() {
            for p in pilots_arr {
                if let Some(pilot) = Self::parse_pilot(p) {
                    snapshot.pilots.push(pilot);
                }
            }
        }

        // 2. Parse Controllers
        if let Some(controllers_arr) = v["controllers"].as_array() {
            for c in controllers_arr {
                if let Some(controller) = Self::parse_controller(c) {
                    snapshot.controllers.push(controller);
                }
            }
        }

        // 3. Parse ATIS
        if let Some(atis_arr) = v["atis"].as_array() {
            for a in atis_arr {
                if let Some(atis) = Self::parse_atis(a) {
                    snapshot.atis.push(atis);
                }
            }
        }

        // 4. Parse Servers
        if let Some(servers_arr) = v["servers"].as_array() {
            for s in servers_arr {
                if let Some(srv) = Self::parse_server(s) {
                    snapshot.servers.push(srv);
                }
            }
        }

        // 5. Parse Prefiles
        if let Some(prefiles_arr) = v["prefiles"].as_array() {
            for pf in prefiles_arr {
                if let Some(prefile) = Self::parse_prefile(pf) {
                    snapshot.prefiles.push(prefile);
                }
            }
        }

        snapshot.freshness = NetworkFreshness::evaluate(generated_at, received_at);
        snapshot.age_seconds = (received_at - generated_at).num_seconds().max(0) as u32;

        Ok(snapshot)
    }

    fn parse_pilot(p: &Value) -> Option<OnlinePilot> {
        let cid = p["cid"].as_u64()?;
        let raw_callsign = p["callsign"].as_str()?;
        let callsign = sanitize_callsign(raw_callsign);
        if callsign.is_empty() {
            return None;
        }

        let raw_lat = p["latitude"].as_f64().unwrap_or(0.0);
        let raw_lon = p["longitude"].as_f64().unwrap_or(0.0);
        let (latitude, longitude) = validate_lat_lon(raw_lat, raw_lon)?;

        let altitude_ft = validate_altitude_ft(p["altitude"].as_i64().unwrap_or(0) as i32);
        let groundspeed_kt = validate_groundspeed_kt(p["groundspeed"].as_u64().unwrap_or(0) as u32);
        let heading_deg = validate_heading_deg(p["heading"].as_u64().unwrap_or(0) as u32);
        let transponder = p["transponder"].as_str().map(|s| sanitize_text(s, 8));

        let fp = &p["flight_plan"];
        let (
            aircraft_type,
            departure_icao,
            arrival_icao,
            alternate_icao,
            flight_rules,
            route,
            planned_altitude_ft,
            planned_tas_kt,
            remarks,
        ) = if fp.is_object() {
            let ac = fp["aircraft_short"]
                .as_str()
                .or_else(|| fp["aircraft"].as_str())
                .map(|s| sanitize_text(s, 16));
            let dep = fp["departure"].as_str().and_then(sanitize_icao);
            let arr = fp["arrival"].as_str().and_then(sanitize_icao);
            let alt_icao = fp["alternate"].as_str().and_then(sanitize_icao);
            let rules = fp["flight_rules"].as_str().map(|s| sanitize_text(s, 4));
            let rt = fp["route"].as_str().map(|s| sanitize_text(s, 2048));
            let plan_alt = fp["altitude"].as_str().and_then(Self::parse_altitude_str);
            let tas = fp["cruise_tas"]
                .as_str()
                .and_then(|s| s.parse::<u32>().ok())
                .or_else(|| fp["cruise_tas"].as_u64().map(|n| n as u32));
            let rem = fp["remarks"].as_str().map(|s| sanitize_text(s, 2048));
            (ac, dep, arr, alt_icao, rules, rt, plan_alt, tas, rem)
        } else {
            (None, None, None, None, None, None, None, None, None)
        };

        let logon_time = p["logon_time"]
            .as_str()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        let last_updated = p["last_updated"]
            .as_str()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        Some(OnlinePilot {
            cid,
            callsign,
            latitude,
            longitude,
            altitude_ft,
            groundspeed_kt,
            heading_deg,
            transponder,
            aircraft_type,
            departure_icao,
            arrival_icao,
            alternate_icao,
            flight_rules,
            route,
            planned_altitude_ft,
            planned_tas_kt,
            remarks,
            logon_time,
            last_updated,
        })
    }

    fn parse_controller(c: &Value) -> Option<OnlineController> {
        let cid = c["cid"].as_u64()?;
        let raw_callsign = c["callsign"].as_str()?;
        let callsign = sanitize_callsign(raw_callsign);
        if callsign.is_empty() {
            return None;
        }

        let frequency = sanitize_text(c["frequency"].as_str().unwrap_or(""), 12);
        let facility = c["facility"].as_u64().unwrap_or(0) as u8;
        let facility_type = FacilityType::from_u8(facility);
        let callsign_role_hint = FacilityType::from_callsign(&callsign);
        let is_consistent = facility_type == FacilityType::Unknown
            || callsign_role_hint == FacilityType::Unknown
            || facility_type == callsign_role_hint;

        let rating = c["rating"].as_u64().unwrap_or(0) as u8;
        let visual_range_nm = c["visual_range"].as_u64().unwrap_or(0) as u32;

        let mut text_atis = Vec::new();
        if let Some(lines) = c["text_atis"].as_array() {
            for line in lines {
                if let Some(s) = line.as_str() {
                    let sanitized = sanitize_text(s, 256);
                    if !sanitized.is_empty() {
                        text_atis.push(sanitized);
                    }
                }
            }
        }

        let logon_time = c["logon_time"]
            .as_str()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        let last_updated = c["last_updated"]
            .as_str()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        let (associated_airport, is_enroute) =
            Self::associate_controller_airport(&callsign, facility_type);

        Some(OnlineController {
            cid,
            callsign,
            frequency,
            facility,
            facility_type,
            callsign_role_hint,
            is_consistent,
            rating,
            visual_range_nm,
            text_atis,
            associated_airport,
            is_enroute,
            logon_time,
            last_updated,
        })
    }

    fn parse_atis(a: &Value) -> Option<OnlineAtis> {
        let cid = a["cid"].as_u64()?;
        let raw_callsign = a["callsign"].as_str()?;
        let callsign = sanitize_callsign(raw_callsign);
        if callsign.is_empty() {
            return None;
        }

        let frequency = sanitize_text(a["frequency"].as_str().unwrap_or(""), 12);
        let atis_code = a["atis_code"]
            .as_str()
            .and_then(|s| s.chars().next())
            .filter(|c| c.is_ascii_alphabetic())
            .map(|c| c.to_ascii_uppercase());

        let mut text_atis = Vec::new();
        if let Some(lines) = a["text_atis"].as_array() {
            for line in lines {
                if let Some(s) = line.as_str() {
                    let sanitized = sanitize_text(s, 512);
                    if !sanitized.is_empty() {
                        text_atis.push(sanitized);
                    }
                }
            }
        }

        let latitude = a["latitude"].as_f64().and_then(|lat| {
            if (-90.0..=90.0).contains(&lat) {
                Some(lat)
            } else {
                None
            }
        });
        let longitude = a["longitude"].as_f64().and_then(|lon| {
            if (-180.0..=180.0).contains(&lon) {
                Some(lon)
            } else {
                None
            }
        });

        // Extract airport identifier: e.g. "KJFK_ATIS" -> "KJFK", "EGLL_A_ATIS" -> "EGLL"
        let airport_ident = Self::extract_atis_airport(&callsign);

        let logon_time = a["logon_time"]
            .as_str()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        let last_updated = a["last_updated"]
            .as_str()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        Some(OnlineAtis {
            cid,
            callsign,
            frequency,
            atis_code,
            text_atis,
            latitude,
            longitude,
            airport_ident,
            logon_time,
            last_updated,
        })
    }

    fn parse_server(s: &Value) -> Option<OnlineServer> {
        let ident = sanitize_text(s["ident"].as_str().unwrap_or(""), 32);
        if ident.is_empty() {
            return None;
        }
        let hostname_or_ip = sanitize_text(s["hostname_or_ip"].as_str().unwrap_or(""), 128);
        let location = sanitize_text(s["location"].as_str().unwrap_or(""), 64);
        let name = sanitize_text(s["name"].as_str().unwrap_or(""), 64);
        let clients_connection_allowed = s["clients_connection_allowed"].as_bool().unwrap_or(true);
        let client_count = s["client_count_connections"].as_u64().map(|n| n as u32);

        Some(OnlineServer {
            ident,
            hostname_or_ip,
            location,
            name,
            clients_connection_allowed,
            client_count,
        })
    }

    fn parse_prefile(pf: &Value) -> Option<OnlinePrefile> {
        let cid = pf["cid"].as_u64()?;
        let raw_callsign = pf["callsign"].as_str()?;
        let callsign = sanitize_callsign(raw_callsign);
        if callsign.is_empty() {
            return None;
        }

        let fp = &pf["flight_plan"];
        let ac = fp["aircraft_short"]
            .as_str()
            .or_else(|| fp["aircraft"].as_str())
            .map(|s| sanitize_text(s, 16));
        let dep = fp["departure"].as_str().and_then(sanitize_icao);
        let arr = fp["arrival"].as_str().and_then(sanitize_icao);
        let alt_icao = fp["alternate"].as_str().and_then(sanitize_icao);
        let rules = fp["flight_rules"].as_str().map(|s| sanitize_text(s, 4));
        let rt = fp["route"].as_str().map(|s| sanitize_text(s, 2048));
        let plan_alt = fp["altitude"].as_str().and_then(Self::parse_altitude_str);
        let tas = fp["cruise_tas"]
            .as_str()
            .and_then(|s| s.parse::<u32>().ok())
            .or_else(|| fp["cruise_tas"].as_u64().map(|n| n as u32));
        let rem = fp["remarks"].as_str().map(|s| sanitize_text(s, 2048));

        let last_updated = pf["last_updated"]
            .as_str()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        Some(OnlinePrefile {
            cid,
            callsign,
            aircraft_type: ac,
            departure_icao: dep,
            arrival_icao: arr,
            alternate_icao: alt_icao,
            flight_rules: rules,
            route: rt,
            planned_altitude_ft: plan_alt,
            planned_tas_kt: tas,
            remarks: rem,
            last_updated,
        })
    }

    /// Helper to associate a controller callsign with a specific airport or enroute FIR.
    pub fn associate_controller_airport(
        callsign: &str,
        facility_type: FacilityType,
    ) -> (Option<String>, bool) {
        let parts: Vec<&str> = callsign.split('_').collect();
        if parts.is_empty() {
            return (None, false);
        }

        let prefix = parts[0].to_uppercase();

        // Enroute centers: e.g. LON_CTR, NY_CTR, ZNY_CTR, KZNY_CTR, BIRD_CTR, EDGG_CTR
        if facility_type == FacilityType::Center
            || parts
                .iter()
                .any(|&p| p == "CTR" || p == "ACC" || p == "UIR" || p == "FSS")
        {
            return (None, true);
        }

        // 4-letter ICAO match (e.g. KJFK_TWR, EGLL_APP, LFPG_GND, OMDB_DEL)
        if prefix.len() == 4 && prefix.chars().all(|c| c.is_ascii_alphanumeric()) {
            return (Some(prefix), false);
        }

        // 3-letter FAA style match (e.g. JFK_TWR -> KJFK, BOS_APP -> KBOS, ORD_GND -> KORD)
        if prefix.len() == 3 && prefix.chars().all(|c| c.is_ascii_alphabetic()) {
            return (Some(format!("K{prefix}")), false);
        }

        (None, false)
    }

    /// Extract airport ICAO from ATIS callsign (e.g. "KJFK_ATIS" -> "KJFK", "JFK_ATIS" -> "KJFK").
    pub fn extract_atis_airport(callsign: &str) -> String {
        let parts: Vec<&str> = callsign.split('_').collect();
        if !parts.is_empty() {
            let prefix = parts[0].to_uppercase();
            if prefix.len() == 4 {
                return prefix;
            } else if prefix.len() == 3 {
                return format!("K{prefix}");
            }
        }
        callsign.to_uppercase()
    }

    fn parse_altitude_str(s: &str) -> Option<i32> {
        let s = s.trim().to_uppercase();
        if let Some(rest) = s.strip_prefix("FL")
            && let Ok(fl) = rest.parse::<i32>()
        {
            return Some(fl * 100);
        }
        if let Ok(alt) = s.parse::<i32>() {
            return Some(alt);
        }
        None
    }
}

#[cfg(feature = "online")]
impl OnlineNetworkProvider for VatsimProvider {
    fn name(&self) -> &'static str {
        "VATSIM"
    }

    fn title(&self) -> &'static str {
        "Virtual Air Traffic Simulation Network (VATSIM)"
    }

    fn polling_interval_secs(&self) -> u32 {
        15
    }

    fn fetch_snapshot(&self) -> Result<NetworkSnapshot> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(12))
            .user_agent(
                "OpenAIRAC/1.8.0 (aviation-navdata-engine; contact: maintainers@openairac.org)",
            )
            .build()
            .context("building HTTP client for VATSIM")?;

        let resp = client
            .get(&self.data_url)
            .send()
            .with_context(|| format!("fetching VATSIM live feed from {}", self.data_url))?;

        let status = resp.status();
        if !status.is_success() {
            anyhow::bail!("VATSIM live feed returned HTTP status: {status}");
        }

        let body = resp
            .text()
            .context("reading VATSIM live feed response body")?;
        let now = Utc::now();
        Self::parse_vatsim_json(&body, now)
    }

    fn fetch_events(&self) -> Result<Vec<OnlineEvent>> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .user_agent("OpenAIRAC/1.8.0 (aviation-navdata-engine)")
            .build()
            .context("building HTTP client for VATSIM events")?;

        let resp = client
            .get(&self.events_url)
            .send()
            .with_context(|| format!("fetching VATSIM events from {}", self.events_url))?;

        if !resp.status().is_success() {
            anyhow::bail!("VATSIM events feed returned HTTP status: {}", resp.status());
        }

        let body = resp.text().context("reading VATSIM events body")?;
        super::vatsim_events::parse_vatsim_events_json(&body)
    }
}
