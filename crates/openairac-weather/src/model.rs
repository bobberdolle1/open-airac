//! Canonical Domain Models for OpenAIRAC Weather Layer.
//!
//! Provides structured models for METAR observations, TAF forecasts, international
//! SIGMET polygons, PIREPs, and flight categories.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Aviation Flight Categories (FAA / ICAO definition).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum FlightCategory {
    /// Visual Flight Rules (Ceiling > 3,000 ft and Visibility > 5 SM)
    Vfr,
    /// Marginal VFR (Ceiling 1,000–3,000 ft or Visibility 3–5 SM)
    Mvfr,
    /// Instrument Flight Rules (Ceiling 500–1,000 ft or Visibility 1–3 SM)
    Ifr,
    /// Low IFR (Ceiling < 500 ft or Visibility < 1 SM)
    Lifr,
    /// Category unknown or insufficient data
    Unknown,
}

impl FlightCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            FlightCategory::Vfr => "VFR",
            FlightCategory::Mvfr => "MVFR",
            FlightCategory::Ifr => "IFR",
            FlightCategory::Lifr => "LIFR",
            FlightCategory::Unknown => "UNKNOWN",
        }
    }

    pub fn from_str_lenient(s: &str) -> Self {
        match s.trim().to_uppercase().as_str() {
            "VFR" => FlightCategory::Vfr,
            "MVFR" => FlightCategory::Mvfr,
            "IFR" => FlightCategory::Ifr,
            "LIFR" => FlightCategory::Lifr,
            _ => FlightCategory::Unknown,
        }
    }

    pub fn badge_color_hex(&self) -> &'static str {
        match self {
            FlightCategory::Vfr => "#28a745",     // Green
            FlightCategory::Mvfr => "#007bff",    // Blue
            FlightCategory::Ifr => "#dc3545",     // Red
            FlightCategory::Lifr => "#6f42c1",    // Magenta
            FlightCategory::Unknown => "#6c757d", // Gray
        }
    }

    /// Compute category from ceiling (ft AGL) and visibility (statute miles).
    pub fn compute(ceiling_ft: Option<u32>, vis_sm: Option<f64>) -> Self {
        let c = ceiling_ft.unwrap_or(10_000);
        let v = vis_sm.unwrap_or(10.0);

        if c < 500 || v < 1.0 {
            FlightCategory::Lifr
        } else if c < 1000 || v < 3.0 {
            FlightCategory::Ifr
        } else if c <= 3000 || v <= 5.0 {
            FlightCategory::Mvfr
        } else {
            FlightCategory::Vfr
        }
    }
}

/// Weather staleness / freshness rating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeatherStaleness {
    /// Observation is recent and current (< 30 minutes)
    Fresh,
    /// Observation is aging but valid (30–60 minutes)
    Aging,
    /// Observation is older than standard report cycle (60–120 minutes)
    Stale,
    /// Observation is expired (> 120 minutes)
    Expired,
    /// Observation time unknown
    Unknown,
}

impl WeatherStaleness {
    pub fn evaluate_metar(obs_time: DateTime<Utc>, now: DateTime<Utc>) -> Self {
        let age_mins = (now - obs_time).num_minutes();
        if age_mins <= 30 {
            WeatherStaleness::Fresh
        } else if age_mins <= 60 {
            WeatherStaleness::Aging
        } else if age_mins <= 120 {
            WeatherStaleness::Stale
        } else {
            WeatherStaleness::Expired
        }
    }
}

/// Cloud layer observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudLayer {
    /// Coverage code (FEW, SCT, BKN, OVC, VV)
    pub cover: String,
    /// Base height in feet AGL
    pub base_ft: Option<u32>,
}

/// Structured METAR observation report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetarReport {
    pub station_id: String,
    pub observation_time: DateTime<Utc>,
    pub report_time: Option<DateTime<Utc>>,
    pub raw_text: String,
    pub flight_category: FlightCategory,
    pub temp_c: Option<f64>,
    pub dewpoint_c: Option<f64>,
    pub wind_dir_deg: Option<u32>,
    pub wind_speed_kts: Option<u32>,
    pub wind_gust_kts: Option<u32>,
    pub wind_variable: bool,
    pub visibility_sm: Option<f64>,
    pub altimeter_hpa: Option<f64>,
    pub altimeter_inhg: Option<f64>,
    pub clouds: Vec<CloudLayer>,
    pub weather_phenomena: Vec<String>,
    pub fetch_time: DateTime<Utc>,
    pub provider_id: String,
    pub is_stale: bool,
}

impl MetarReport {
    pub fn age_minutes(&self, now: DateTime<Utc>) -> i64 {
        (now - self.observation_time).num_minutes().max(0)
    }

    pub fn staleness(&self, now: DateTime<Utc>) -> WeatherStaleness {
        WeatherStaleness::evaluate_metar(self.observation_time, now)
    }
}

/// A specific forecast period inside a TAF report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TafForecastPeriod {
    pub valid_from: DateTime<Utc>,
    pub valid_to: DateTime<Utc>,
    /// Change indicator (FM, TEMPO, PROB30, BECMG)
    pub change_type: Option<String>,
    pub wind_dir_deg: Option<u32>,
    pub wind_speed_kts: Option<u32>,
    pub wind_gust_kts: Option<u32>,
    pub visibility_sm: Option<f64>,
    pub flight_category: FlightCategory,
    pub clouds: Vec<CloudLayer>,
    pub weather_phenomena: Vec<String>,
    pub raw_period: String,
}

/// Terminal Aerodrome Forecast (TAF) report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TafReport {
    pub station_id: String,
    pub issue_time: DateTime<Utc>,
    pub valid_from: DateTime<Utc>,
    pub valid_to: DateTime<Utc>,
    pub raw_text: String,
    pub forecast_periods: Vec<TafForecastPeriod>,
    pub fetch_time: DateTime<Utc>,
    pub provider_id: String,
    pub is_stale: bool,
}

impl TafReport {
    /// Select the TAF forecast period matching a given Estimated Time of Arrival (ETA).
    pub fn forecast_at_eta(&self, eta: DateTime<Utc>) -> Option<&TafForecastPeriod> {
        self.forecast_periods
            .iter()
            .find(|p| eta >= p.valid_from && eta < p.valid_to)
            .or_else(|| self.forecast_periods.first())
    }
}

/// Hazard types for Significant Meteorological Information (SIGMET).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SigmetHazard {
    Convective,
    Turbulence,
    Icing,
    VolcanicAsh,
    TropicalCyclone,
    Radioactive,
    Other(String),
}

impl SigmetHazard {
    pub fn from_str_lenient(s: &str) -> Self {
        match s.trim().to_uppercase().as_str() {
            "TS" | "CONVECTIVE" | "THUNDERSTORM" => SigmetHazard::Convective,
            "TURB" | "TURBULENCE" | "SEV TURB" => SigmetHazard::Turbulence,
            "ICE" | "ICING" | "SEV ICE" => SigmetHazard::Icing,
            "VA" | "VOLCANIC ASH" => SigmetHazard::VolcanicAsh,
            "TC" | "TROPICAL CYCLONE" => SigmetHazard::TropicalCyclone,
            "RDOACT" | "RADIOACTIVE" => SigmetHazard::Radioactive,
            other => SigmetHazard::Other(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            SigmetHazard::Convective => "Thunderstorm / Convective",
            SigmetHazard::Turbulence => "Severe Turbulence",
            SigmetHazard::Icing => "Severe Icing",
            SigmetHazard::VolcanicAsh => "Volcanic Ash",
            SigmetHazard::TropicalCyclone => "Tropical Cyclone",
            SigmetHazard::Radioactive => "Radioactive Cloud",
            SigmetHazard::Other(s) => s.as_str(),
        }
    }
}

/// International or domestic SIGMET polygon advisory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sigmet {
    pub id: String,
    pub fir_id: String,
    pub fir_name: Option<String>,
    pub hazard: SigmetHazard,
    pub qualifier: Option<String>,
    pub valid_from: DateTime<Utc>,
    pub valid_to: DateTime<Utc>,
    pub base_altitude_ft: Option<u32>,
    pub top_altitude_ft: Option<u32>,
    /// Polygon boundary coordinates: (longitude, latitude)
    pub polygon: Vec<(f64, f64)>,
    pub movement_dir_deg: Option<u32>,
    pub movement_speed_kts: Option<u32>,
    pub raw_text: String,
    pub provider_id: String,
}

impl Sigmet {
    pub fn is_active_at(&self, time: DateTime<Utc>) -> bool {
        time >= self.valid_from && time <= self.valid_to
    }
}

/// Pilot Weather Report (PIREP).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PirepReport {
    pub obs_time: DateTime<Utc>,
    pub aircraft_type: Option<String>,
    pub latitude: f64,
    pub longitude: f64,
    pub flight_level: Option<u32>,
    pub turbulence: Option<String>,
    pub icing: Option<String>,
    pub temp_c: Option<f64>,
    pub raw_text: String,
}

/// Weather station metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeatherStation {
    pub ident: String,
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub elevation_ft: Option<f64>,
    pub country: Option<String>,
    pub has_metar: bool,
    pub has_taf: bool,
}
