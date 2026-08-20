//! Official VATSIM Events API Ingestion.
//!
//! Parses active and upcoming events from `https://events.vatsim.net/api/v2/events`.

use crate::model::OnlineEvent;
use crate::sanitize::{sanitize_icao, sanitize_text};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;

pub const VATSIM_EVENTS_URL: &str = "https://events.vatsim.net/api/v2/events";

/// Parse VATSIM Events API v2 JSON response into `Vec<OnlineEvent>`.
pub fn parse_vatsim_events_json(raw_json: &str) -> Result<Vec<OnlineEvent>> {
    let root: Value = serde_json::from_str(raw_json).context("parsing VATSIM Events JSON")?;

    let events_arr = if let Some(arr) = root.as_array() {
        arr
    } else if let Some(arr) = root["data"].as_array() {
        arr
    } else {
        return Ok(Vec::new());
    };

    let mut out = Vec::with_capacity(events_arr.len());
    for item in events_arr {
        if let Some(ev) = parse_single_event(item) {
            out.push(ev);
        }
    }

    // Sort chronologically by start_time
    out.sort_by_key(|e| e.start_time);

    Ok(out)
}

fn parse_single_event(item: &Value) -> Option<OnlineEvent> {
    let id = item["id"].as_u64()?;
    let raw_name = item["name"].as_str()?;
    let name = sanitize_text(raw_name, 128);
    if name.is_empty() {
        return None;
    }

    let event_type = item["type"]["name"]
        .as_str()
        .or_else(|| item["type"].as_str())
        .map(|s| sanitize_text(s, 64));

    let start_str = item["start_time"]
        .as_str()
        .or_else(|| item["start"].as_str())?;
    let end_str = item["end_time"].as_str().or_else(|| item["end"].as_str())?;

    let start_time = DateTime::parse_from_rfc3339(start_str)
        .ok()?
        .with_timezone(&Utc);
    let end_time = DateTime::parse_from_rfc3339(end_str)
        .ok()?
        .with_timezone(&Utc);

    // Extract airports
    let mut airports = Vec::new();
    if let Some(apt_arr) = item["airports"].as_array() {
        for apt in apt_arr {
            if let Some(icao_str) = apt["icao"].as_str().or_else(|| apt.as_str())
                && let Some(clean_icao) = sanitize_icao(icao_str)
                && !airports.contains(&clean_icao)
            {
                airports.push(clean_icao);
            }
        }
    }
    // Extract routes
    let mut routes = Vec::new();
    if let Some(r_arr) = item["routes"].as_array() {
        for r in r_arr {
            if let Some(r_str) = r["route"].as_str().or_else(|| r.as_str()) {
                let sanitized = sanitize_text(r_str, 512);
                if !sanitized.is_empty() {
                    routes.push(sanitized);
                }
            }
        }
    }

    // Extract organizers
    let mut organizers = Vec::new();
    if let Some(org_arr) = item["organisers"]
        .as_array()
        .or_else(|| item["organizers"].as_array())
    {
        for org in org_arr {
            if let Some(o_str) = org["name"].as_str().or_else(|| org.as_str()) {
                let sanitized = sanitize_text(o_str, 64);
                if !sanitized.is_empty() {
                    organizers.push(sanitized);
                }
            }
        }
    }

    let link = item["link"].as_str().map(|s| sanitize_text(s, 256));
    let description = item["short_description"]
        .as_str()
        .or_else(|| item["description"].as_str())
        .map(|s| sanitize_text(s, 4096));

    Some(OnlineEvent {
        id,
        name,
        event_type,
        start_time,
        end_time,
        airports,
        routes,
        organizers,
        link,
        description,
    })
}
