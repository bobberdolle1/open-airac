//! Integrated Preflight Flight Briefing Engine.
//!
//! Combines Departure & Destination METAR/TAF, TAF-at-ETA, Route Corridor SIGMETs,
//! PIREPs, Charts status, and Navdata status into a comprehensive preflight briefing.

use crate::model::{MetarReport, PirepReport, Sigmet, TafForecastPeriod, TafReport};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AirportWeatherBriefing {
    pub icao: String,
    pub metar: Option<MetarReport>,
    pub taf: Option<TafReport>,
    pub taf_at_eta: Option<TafForecastPeriod>,
    pub charts_count: usize,
    pub navdata_procedures_available: bool,
    pub navdata_note: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlightBriefing {
    pub departure_icao: String,
    pub destination_icao: String,
    pub alternate_icaos: Vec<String>,
    pub planned_departure_time: DateTime<Utc>,
    pub estimated_time_enroute_minutes: u32,
    pub estimated_time_of_arrival: DateTime<Utc>,
    pub departure: AirportWeatherBriefing,
    pub destination: AirportWeatherBriefing,
    pub alternates: Vec<AirportWeatherBriefing>,
    pub route_sigmets: Vec<Sigmet>,
    pub route_pireps: Vec<PirepReport>,
    pub navdata_cycle: String,
    pub charts_cycle: String,
    pub generated_at: DateTime<Utc>,
}

impl FlightBriefing {
    /// Format briefing as human-readable plain text.
    pub fn format_text(&self) -> String {
        let mut out = String::new();
        out += "================================================================================\n";
        out += &format!(
            "OPENAIRAC FLIGHT BRIEFING — {} → {}\n",
            self.departure_icao, self.destination_icao
        );
        out += &format!(
            "Generated: {} | Navdata Cycle: {} | Charts Cycle: {}\n",
            self.generated_at.format("%Y-%m-%d %H:%M:%SZ"),
            self.navdata_cycle,
            self.charts_cycle
        );
        out += "================================================================================\n\n";

        // 1. Departure
        out += &format!("1. DEPARTURE AIRPORT: {}\n", self.departure_icao);
        out += &format!("   Charts Available:   {}\n", self.departure.charts_count);
        out += &format!("   Navdata Procedures: {}\n", if self.departure.navdata_procedures_available { "YES" } else { "NO" });
        if !self.departure.navdata_note.is_empty() {
            out += &format!("   Navdata Notice:     {}\n", self.departure.navdata_note);
        }
        if let Some(m) = &self.departure.metar {
            out += &format!("   METAR ({}): {}\n", m.flight_category.as_str(), m.raw_text);
            out += &format!(
                "   Conditions: Wind {}/{} kt, Temp {}°C, Vis {} SM, Alt {} hPa\n",
                m.wind_dir_deg.map(|d| d.to_string()).unwrap_or_else(|| "VRB".to_string()),
                m.wind_speed_kts.unwrap_or(0),
                m.temp_c.unwrap_or(0.0),
                m.visibility_sm.unwrap_or(10.0),
                m.altimeter_hpa.unwrap_or(1013.2)
            );
        } else {
            out += "   METAR: Not available\n";
        }
        if let Some(t) = &self.departure.taf {
            out += &format!("   TAF: {}\n", t.raw_text);
        }
        out += "\n";

        // 2. Destination
        out += &format!("2. DESTINATION AIRPORT: {} (ETA: {})\n", self.destination_icao, self.estimated_time_of_arrival.format("%H:%MZ"));
        out += &format!("   Charts Available:   {}\n", self.destination.charts_count);
        out += &format!("   Navdata Procedures: {}\n", if self.destination.navdata_procedures_available { "YES" } else { "NO" });
        if !self.destination.navdata_note.is_empty() {
            out += &format!("   Navdata Notice:     {}\n", self.destination.navdata_note);
        }
        if let Some(m) = &self.destination.metar {
            out += &format!("   Current METAR ({}): {}\n", m.flight_category.as_str(), m.raw_text);
        }
        if let Some(eta_fcst) = &self.destination.taf_at_eta {
            out += &format!("   Forecast at ETA ({}): {}\n", eta_fcst.flight_category.as_str(), eta_fcst.raw_period);
        } else if let Some(t) = &self.destination.taf {
            out += &format!("   TAF: {}\n", t.raw_text);
        }
        out += "\n";

        // 3. Route Hazards
        out += &format!("3. ENROUTE HAZARDS (Route Corridor: 50 NM Width)\n");
        out += &format!("   Active Intersecting SIGMETs: {}\n", self.route_sigmets.len());
        for s in &self.route_sigmets {
            out += &format!("     - [{}] FIR: {}, Valid: {} to {}\n", s.hazard.as_str(), s.fir_id, s.valid_from.format("%H:%MZ"), s.valid_to.format("%H:%MZ"));
            out += &format!("       Raw: {}\n", s.raw_text);
        }
        out += &format!("   Recent PIREPs along Route:  {}\n", self.route_pireps.len());
        for p in &self.route_pireps {
            out += &format!("     - [{}] Type: {}, FL: {:?}, Turb: {:?}, Ice: {:?}\n", p.obs_time.format("%H:%MZ"), p.aircraft_type.as_deref().unwrap_or("?"), p.flight_level, p.turbulence, p.icing);
        }
        out += "\n";

        out += "================================================================================\n";
        out += "End of OpenAIRAC Flight Briefing\n";
        out
    }

    /// Format briefing as secure, HTML-escaped rich document.
    pub fn format_html(&self) -> String {
        fn escape(s: &str) -> String {
            s.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;")
                .replace('\'', "&#39;")
        }

        let mut html = String::new();
        html += "<div style='font-family: sans-serif; padding: 12px;'>";
        html += &format!(
            "<h2 style='margin-bottom: 4px;'>OpenAIRAC Flight Briefing: {} &rarr; {}</h2>",
            escape(&self.departure_icao), escape(&self.destination_icao)
        );
        html += &format!(
            "<div style='color: #666; font-size: 12px; margin-bottom: 12px;'>Generated: {} | AIRAC: {} | Charts: {}</div>",
            self.generated_at.format("%Y-%m-%d %H:%M:%SZ"),
            escape(&self.navdata_cycle),
            escape(&self.charts_cycle)
        );

        // Departure Block
        html += "<div style='border: 1px solid #ccc; border-radius: 4px; padding: 8px; margin-bottom: 12px;'>";
        html += &format!("<h3>Departure: {}</h3>", escape(&self.departure_icao));
        if let Some(m) = &self.departure.metar {
            html += &format!(
                "<p><b>METAR</b> <span style='background: {}; color: white; padding: 2px 6px; border-radius: 3px;'>{}</span>: <code>{}</code></p>",
                m.flight_category.badge_color_hex(),
                m.flight_category.as_str(),
                escape(&m.raw_text)
            );
        }
        if let Some(t) = &self.departure.taf {
            html += &format!("<p><b>TAF</b>: <code>{}</code></p>", escape(&t.raw_text));
        }
        html += &format!("<p style='font-size: 12px; color: #555;'>Charts: {} | Navdata: {}</p>", self.departure.charts_count, escape(&self.departure.navdata_note));
        html += "</div>";

        // Destination Block
        html += "<div style='border: 1px solid #ccc; border-radius: 4px; padding: 8px; margin-bottom: 12px;'>";
        html += &format!("<h3>Destination: {} (ETA {})</h3>", escape(&self.destination_icao), self.estimated_time_of_arrival.format("%H:%MZ"));
        if let Some(m) = &self.destination.metar {
            html += &format!(
                "<p><b>Current METAR</b> <span style='background: {}; color: white; padding: 2px 6px; border-radius: 3px;'>{}</span>: <code>{}</code></p>",
                m.flight_category.badge_color_hex(),
                m.flight_category.as_str(),
                escape(&m.raw_text)
            );
        }
        if let Some(eta_fcst) = &self.destination.taf_at_eta {
            html += &format!(
                "<p><b>Forecast at ETA</b> <span style='background: {}; color: white; padding: 2px 6px; border-radius: 3px;'>{}</span>: <code>{}</code></p>",
                eta_fcst.flight_category.badge_color_hex(),
                eta_fcst.flight_category.as_str(),
                escape(&eta_fcst.raw_period)
            );
        }
        html += &format!("<p style='font-size: 12px; color: #555;'>Charts: {} | Navdata: {}</p>", self.destination.charts_count, escape(&self.destination.navdata_note));
        html += "</div>";

        // Hazards Block
        html += "<div style='border: 1px solid #e0a800; background: #fffdf5; border-radius: 4px; padding: 8px; margin-bottom: 12px;'>";
        html += &format!("<h3>Route Hazards ({} Intersecting SIGMETs, {} PIREPs)</h3>", self.route_sigmets.len(), self.route_pireps.len());
        for s in &self.route_sigmets {
            html += &format!(
                "<p style='margin: 4px 0;'><b>SIGMET ({})</b> FIR: {} (Valid {} - {}):<br/><code>{}</code></p>",
                escape(s.hazard.as_str()),
                escape(&s.fir_id),
                s.valid_from.format("%H:%MZ"),
                s.valid_to.format("%H:%MZ"),
                escape(&s.raw_text)
            );
        }
        html += "</div>";

        html += "</div>";
        html
    }
}
