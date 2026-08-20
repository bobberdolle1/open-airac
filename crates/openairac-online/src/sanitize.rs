//! Security and Input Sanitization for Untrusted Network Data.
//!
//! Escapes HTML entities, strips control characters, clamps values to realistic bounds,
//! and guards against oversized strings or payloads.

/// Escape untrusted text for safe HTML/UI presentation.
pub fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Sanitize callsign string (uppercase alphanumeric, underscores, hyphens, max 16 chars).
pub fn sanitize_callsign(input: &str) -> String {
    let filtered: String = input
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .take(16)
        .collect();
    filtered.to_uppercase()
}

/// Sanitize ICAO airport ident (uppercase 3 or 4 alphanumeric characters).
pub fn sanitize_icao(input: &str) -> Option<String> {
    let clean = input.trim().to_uppercase();
    if (3..=4).contains(&clean.len()) && clean.chars().all(|c| c.is_ascii_alphanumeric()) {
        Some(clean)
    } else {
        None
    }
}

/// Truncate and sanitize general remark strings (max length bounds).
pub fn sanitize_text(input: &str, max_len: usize) -> String {
    input
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\r' || *c == '\t')
        .take(max_len)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Validate geographic coordinates (-90..90 latitude, -180..180 longitude).
pub fn validate_lat_lon(lat: f64, lon: f64) -> Option<(f64, f64)> {
    if lat.is_nan() || lon.is_nan() || lat.is_infinite() || lon.is_infinite() {
        return None;
    }
    if (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon) {
        Some((lat, lon))
    } else {
        None
    }
}

/// Validate altitude in feet (-2000..100,000).
pub fn validate_altitude_ft(alt: i32) -> i32 {
    alt.clamp(-2000, 100_000)
}

/// Validate groundspeed in knots (0..3000).
pub fn validate_groundspeed_kt(gs: u32) -> u32 {
    gs.min(3000)
}

/// Validate heading in degrees (0..360).
pub fn validate_heading_deg(hdg: u32) -> u16 {
    (hdg % 360) as u16
}
