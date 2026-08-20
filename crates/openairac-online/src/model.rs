//! Real-Time Online Network Domain Models.
//!
//! Provides structured types for live pilots, ATC controllers, ATIS broadcasts,
//! servers, prefiled flight plans, online events, and network freshness state.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// Freshness state of an online network snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkFreshness {
    /// Received within 30 seconds (standard live polling cadence).
    Live,
    /// Received between 30 and 90 seconds ago (slight network delay).
    Delayed,
    /// Received between 90 and 300 seconds ago (stale, display warning, stop extrapolation).
    Stale,
    /// Older than 300 seconds or unreachable (offline).
    Offline,
}

impl NetworkFreshness {
    pub fn evaluate(generated_at: DateTime<Utc>, now: DateTime<Utc>) -> Self {
        let age_secs = (now - generated_at).num_seconds();
        if age_secs <= 35 {
            NetworkFreshness::Live
        } else if age_secs <= 90 {
            NetworkFreshness::Delayed
        } else if age_secs <= 300 {
            NetworkFreshness::Stale
        } else {
            NetworkFreshness::Offline
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Live => "LIVE",
            Self::Delayed => "DELAYED",
            Self::Stale => "STALE",
            Self::Offline => "OFFLINE",
        }
    }
}

/// Air Traffic Control facility classifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum FacilityType {
    Unknown = 0,
    Delivery = 1,
    Ground = 2,
    Tower = 3,
    Approach = 4,
    Center = 5,
    Fss = 6,
}

impl FacilityType {
    pub fn from_u8(val: u8) -> Self {
        match val {
            1 => Self::Delivery,
            2 => Self::Ground,
            3 => Self::Tower,
            4 => Self::Approach,
            5 => Self::Center,
            6 => Self::Fss,
            _ => Self::Unknown,
        }
    }

    pub fn from_callsign(callsign: &str) -> Self {
        let upper = callsign.to_uppercase();
        if upper.ends_with("_DEL") || upper.ends_with("_CLR") {
            Self::Delivery
        } else if upper.ends_with("_GND") {
            Self::Ground
        } else if upper.ends_with("_TWR") {
            Self::Tower
        } else if upper.ends_with("_APP") || upper.ends_with("_DEP") || upper.ends_with("_DIR") {
            Self::Approach
        } else if upper.ends_with("_CTR") || upper.ends_with("_ACC") || upper.ends_with("_UIR") {
            Self::Center
        } else if upper.ends_with("_FSS") || upper.ends_with("_RDO") {
            Self::Fss
        } else {
            Self::Unknown
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Delivery => "DEL",
            Self::Ground => "GND",
            Self::Tower => "TWR",
            Self::Approach => "APP",
            Self::Center => "CTR",
            Self::Fss => "FSS",
            Self::Unknown => "ATC",
        }
    }

    pub fn full_name(&self) -> &'static str {
        match self {
            Self::Delivery => "Clearance Delivery",
            Self::Ground => "Ground Control",
            Self::Tower => "Tower Control",
            Self::Approach => "Approach / Radar Departure",
            Self::Center => "Enroute Area Control Center (ACC/ARTCC)",
            Self::Fss => "Flight Service Station (FSS)",
            Self::Unknown => "Air Traffic Control",
        }
    }
}

/// Live connected pilot on the online network.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OnlinePilot {
    pub cid: u64,
    pub callsign: String,
    pub latitude: f64,
    pub longitude: f64,
    pub altitude_ft: i32,
    pub groundspeed_kt: u32,
    pub heading_deg: u16,
    pub transponder: Option<String>,
    pub aircraft_type: Option<String>,
    pub departure_icao: Option<String>,
    pub arrival_icao: Option<String>,
    pub alternate_icao: Option<String>,
    pub flight_rules: Option<String>,
    pub route: Option<String>,
    pub planned_altitude_ft: Option<i32>,
    pub planned_tas_kt: Option<u32>,
    pub remarks: Option<String>,
    pub logon_time: Option<DateTime<Utc>>,
    pub last_updated: Option<DateTime<Utc>>,
}

impl OnlinePilot {
    pub fn is_airborne(&self) -> bool {
        self.groundspeed_kt >= 40 || self.altitude_ft > 500
    }
}

/// Live connected Air Traffic Controller.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OnlineController {
    pub cid: u64,
    pub callsign: String,
    pub frequency: String,
    pub facility: u8,
    pub facility_type: FacilityType,
    pub rating: u8,
    pub visual_range_nm: u32,
    pub text_atis: Vec<String>,
    pub associated_airport: Option<String>,
    pub is_enroute: bool,
    pub logon_time: Option<DateTime<Utc>>,
    pub last_updated: Option<DateTime<Utc>>,
}

/// Live active ATIS broadcast.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OnlineAtis {
    pub cid: u64,
    pub callsign: String,
    pub frequency: String,
    pub atis_code: Option<char>,
    pub text_atis: Vec<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub airport_ident: String,
    pub logon_time: Option<DateTime<Utc>>,
    pub last_updated: Option<DateTime<Utc>>,
}

/// Online network server status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnlineServer {
    pub ident: String,
    pub hostname_or_ip: String,
    pub location: String,
    pub name: String,
    pub clients_connection_allowed: bool,
    pub client_count: Option<u32>,
}

/// Prefiled flight plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OnlinePrefile {
    pub cid: u64,
    pub callsign: String,
    pub aircraft_type: Option<String>,
    pub departure_icao: Option<String>,
    pub arrival_icao: Option<String>,
    pub alternate_icao: Option<String>,
    pub flight_rules: Option<String>,
    pub route: Option<String>,
    pub planned_altitude_ft: Option<i32>,
    pub planned_tas_kt: Option<u32>,
    pub remarks: Option<String>,
    pub last_updated: Option<DateTime<Utc>>,
}

/// Official VATSIM or community online event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnlineEvent {
    pub id: u64,
    pub name: String,
    pub event_type: Option<String>,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub airports: Vec<String>,
    pub routes: Vec<String>,
    pub organizers: Vec<String>,
    pub link: Option<String>,
    pub description: Option<String>,
}

impl OnlineEvent {
    pub fn is_active_at(&self, time: DateTime<Utc>) -> bool {
        time >= self.start_time && time <= self.end_time
    }

    pub fn is_upcoming_within(&self, now: DateTime<Utc>, window: Duration) -> bool {
        self.start_time > now && self.start_time <= now + window
    }

    pub fn matches_airport(&self, icao: &str) -> bool {
        let needle = icao.trim().to_uppercase();
        self.airports
            .iter()
            .any(|a| a.trim().to_uppercase() == needle)
    }
}

/// Complete instantaneous online network state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetworkSnapshot {
    pub provider_name: String,
    pub generated_at: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
    pub connected_clients: u32,
    pub pilots: Vec<OnlinePilot>,
    pub controllers: Vec<OnlineController>,
    pub atis: Vec<OnlineAtis>,
    pub servers: Vec<OnlineServer>,
    pub prefiles: Vec<OnlinePrefile>,
    pub events: Vec<OnlineEvent>,
    pub freshness: NetworkFreshness,
    pub age_seconds: u32,
}

impl NetworkSnapshot {
    pub fn new(
        provider_name: impl Into<String>,
        generated_at: DateTime<Utc>,
        received_at: DateTime<Utc>,
    ) -> Self {
        let age_secs = (received_at - generated_at).num_seconds().max(0) as u32;
        let freshness = NetworkFreshness::evaluate(generated_at, received_at);
        Self {
            provider_name: provider_name.into(),
            generated_at,
            received_at,
            connected_clients: 0,
            pilots: Vec::new(),
            controllers: Vec::new(),
            atis: Vec::new(),
            servers: Vec::new(),
            prefiles: Vec::new(),
            events: Vec::new(),
            freshness,
            age_seconds: age_secs,
        }
    }
}
