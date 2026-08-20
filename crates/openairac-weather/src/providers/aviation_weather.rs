//! NOAA / AviationWeather.gov Data API Weather Provider.
//!
//! Authoritative live weather provider consuming modern `/api/data/*` endpoints.

use crate::model::{
    CloudLayer, FlightCategory, MetarReport, PirepReport, Sigmet, SigmetHazard, TafForecastPeriod,
    TafReport,
};
use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;

pub struct AviationWeatherProvider {
    pub base_url: String,
}

impl Default for AviationWeatherProvider {
    fn default() -> Self {
        Self {
            base_url: "https://aviationweather.gov/api/data".to_string(),
        }
    }
}

impl AviationWeatherProvider {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse official AviationWeather.gov METAR JSON array.
    pub fn parse_metar_json(&self, json_str: &str) -> Result<Vec<MetarReport>> {
        let root: Value = serde_json::from_str(json_str)?;
        let items = match root {
            Value::Array(arr) => arr,
            Value::Object(obj) => vec![Value::Object(obj)],
            _ => bail!("Unexpected JSON structure for METAR list"),
        };

        let now = Utc::now();
        let mut reports = Vec::new();

        for item in items {
            let station_id = item["icaoId"].as_str().unwrap_or("").trim().to_uppercase();
            if station_id.is_empty() {
                continue;
            }

            let raw_text = item["rawOb"].as_str().unwrap_or("").trim().to_string();
            let obs_timestamp = item["obsTime"].as_i64();
            let obs_time = if let Some(ts) = obs_timestamp {
                Utc.timestamp_opt(ts, 0).single().unwrap_or(now)
            } else if let Some(rep) = item["reportTime"].as_str() {
                DateTime::parse_from_rfc3339(rep).ok().map(|d| d.with_timezone(&Utc)).unwrap_or(now)
            } else {
                now
            };

            let temp_c = item["temp"].as_f64();
            let dewp_c = item["dewp"].as_f64();
            let wdir = item["wdir"].as_u64().map(|v| v as u32);
            let wspd = item["wspd"].as_u64().map(|v| v as u32);
            let wgst = item["wgst"].as_u64().map(|v| v as u32);

            let vis_str = item["visib"].as_str().unwrap_or("");
            let visibility_sm = if vis_str.contains('+') {
                Some(10.0)
            } else {
                item["visib"].as_f64().or_else(|| vis_str.parse::<f64>().ok())
            };

            let altim_hpa = item["altim"].as_f64();
            let altim_inhg = altim_hpa.map(|h| h * 0.0295299830714);

            let fltcat_str = item["fltcat"].as_str().unwrap_or("");
            let flight_category = if !fltcat_str.is_empty() {
                FlightCategory::from_str_lenient(fltcat_str)
            } else {
                FlightCategory::compute(None, visibility_sm)
            };

            // Parse cloud layers if present
            let mut clouds = Vec::new();
            if let Some(clouds_arr) = item["clouds"].as_array() {
                for c in clouds_arr {
                    let cover = c["cover"].as_str().unwrap_or("").to_string();
                    let base = c["base"].as_u64().map(|b| b as u32);
                    if !cover.is_empty() {
                        clouds.push(CloudLayer {
                            cover,
                            base_ft: base,
                        });
                    }
                }
            }

            let mut phenomena = Vec::new();
            if let Some(wx) = item["wxString"].as_str() {
                if !wx.trim().is_empty() {
                    phenomena.push(wx.trim().to_string());
                }
            }

            let mut report = MetarReport {
                station_id,
                observation_time: obs_time,
                report_time: Some(obs_time),
                raw_text,
                flight_category,
                temp_c,
                dewpoint_c: dewp_c,
                wind_dir_deg: wdir,
                wind_speed_kts: wspd,
                wind_gust_kts: wgst,
                wind_variable: wdir.is_none() && wspd.unwrap_or(0) > 0,
                visibility_sm,
                altimeter_hpa: altim_hpa,
                altimeter_inhg: altim_inhg,
                clouds,
                weather_phenomena: phenomena,
                fetch_time: now,
                provider_id: "NOAA_AWC".to_string(),
                is_stale: false,
            };

            report.is_stale = report.staleness(now) == crate::model::WeatherStaleness::Stale
                || report.staleness(now) == crate::model::WeatherStaleness::Expired;

            reports.push(report);
        }

        Ok(reports)
    }

    /// Parse official AviationWeather.gov TAF JSON array.
    pub fn parse_taf_json(&self, json_str: &str) -> Result<Vec<TafReport>> {
        let root: Value = serde_json::from_str(json_str)?;
        let items = match root {
            Value::Array(arr) => arr,
            Value::Object(obj) => vec![Value::Object(obj)],
            _ => bail!("Unexpected JSON structure for TAF list"),
        };

        let now = Utc::now();
        let mut reports = Vec::new();

        for item in items {
            let station_id = item["icaoId"].as_str().unwrap_or("").trim().to_uppercase();
            if station_id.is_empty() {
                continue;
            }

            let raw_text = item["rawTAF"].as_str().unwrap_or("").trim().to_string();
            let issue_time = item["issueTime"].as_str().and_then(|s| {
                DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&Utc))
            }).unwrap_or(now);

            let valid_from = item["validTimeFrom"].as_i64()
                .and_then(|ts| Utc.timestamp_opt(ts, 0).single())
                .unwrap_or(issue_time);

            let valid_to = item["validTimeTo"].as_i64()
                .and_then(|ts| Utc.timestamp_opt(ts, 0).single())
                .unwrap_or_else(|| valid_from + chrono::Duration::hours(24));

            let mut forecast_periods = Vec::new();
            if let Some(fcsts) = item["fcsts"].as_array() {
                for f in fcsts {
                    let f_from = f["timeFrom"].as_i64()
                        .and_then(|ts| Utc.timestamp_opt(ts, 0).single())
                        .unwrap_or(valid_from);
                    let f_to = f["timeTo"].as_i64()
                        .and_then(|ts| Utc.timestamp_opt(ts, 0).single())
                        .unwrap_or(valid_to);

                    let change = f["change"].as_str().map(|s| s.to_string());
                    let wdir = f["wdir"].as_u64().map(|v| v as u32);
                    let wspd = f["wspd"].as_u64().map(|v| v as u32);
                    let wgst = f["wgst"].as_u64().map(|v| v as u32);
                    let vis = f["visib"].as_f64();

                    let mut clouds = Vec::new();
                    if let Some(c_arr) = f["clouds"].as_array() {
                        for c in c_arr {
                            let cover = c["cover"].as_str().unwrap_or("").to_string();
                            let base = c["base"].as_u64().map(|b| b as u32);
                            if !cover.is_empty() {
                                clouds.push(CloudLayer { cover, base_ft: base });
                            }
                        }
                    }

                    let mut wx = Vec::new();
                    if let Some(wxs) = f["wxString"].as_str() {
                        if !wxs.trim().is_empty() {
                            wx.push(wxs.trim().to_string());
                        }
                    }

                    let fltcat = FlightCategory::compute(None, vis);

                    forecast_periods.push(TafForecastPeriod {
                        valid_from: f_from,
                        valid_to: f_to,
                        change_type: change,
                        wind_dir_deg: wdir,
                        wind_speed_kts: wspd,
                        wind_gust_kts: wgst,
                        visibility_sm: vis,
                        flight_category: fltcat,
                        clouds,
                        weather_phenomena: wx,
                        raw_period: f["rawWx"].as_str().unwrap_or("").to_string(),
                    });
                }
            }

            reports.push(TafReport {
                station_id,
                issue_time,
                valid_from,
                valid_to,
                raw_text,
                forecast_periods,
                fetch_time: now,
                provider_id: "NOAA_AWC".to_string(),
                is_stale: now > valid_to,
            });
        }

        Ok(reports)
    }

    /// Parse International SIGMET GeoJSON FeatureCollection.
    pub fn parse_isigmet_geojson(&self, geojson_str: &str) -> Result<Vec<Sigmet>> {
        let root: Value = serde_json::from_str(geojson_str)?;
        let features = root["features"].as_array()
            .ok_or_else(|| anyhow!("Expected GeoJSON FeatureCollection with 'features' array"))?;

        let now = Utc::now();
        let mut sigmets = Vec::new();

        for (idx, feat) in features.iter().enumerate() {
            let props = &feat["properties"];
            let fir_id = props["firId"].as_str().unwrap_or("").to_string();
            let fir_name = props["firName"].as_str().map(|s| s.to_string());
            let hazard_str = props["hazard"].as_str().unwrap_or("OTHER");
            let qualifier = props["qualifier"].as_str().map(|s| s.to_string());

            let valid_from = props["validTimeFrom"].as_str()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&Utc)))
                .unwrap_or(now);

            let valid_to = props["validTimeTo"].as_str()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&Utc)))
                .unwrap_or_else(|| valid_from + chrono::Duration::hours(4));

            let base_alt = props["base"].as_u64().map(|a| a as u32);
            let top_alt = props["top"].as_u64().map(|a| a as u32);
            let raw_text = props["rawSigmet"].as_str().unwrap_or("").to_string();

            let geom = &feat["geometry"];
            let mut polygon = Vec::new();
            if let Some(coords_arr) = geom["coordinates"].as_array() {
                // Polygon coordinates: array of rings
                let ring = if coords_arr.first().and_then(|v| v.as_array()).is_some() {
                    coords_arr.first().and_then(|v| v.as_array()).unwrap()
                } else {
                    coords_arr
                };

                for pt in ring {
                    if let Some(pt_arr) = pt.as_array() {
                        if pt_arr.len() >= 2 {
                            let lon = pt_arr[0].as_f64().unwrap_or(0.0);
                            let lat = pt_arr[1].as_f64().unwrap_or(0.0);
                            polygon.push((lon, lat));
                        }
                    }
                }
            }

            let series_id = props["seriesId"].as_str().unwrap_or("01");
            let id = format!("{fir_id}:{series_id}:{idx}");

            sigmets.push(Sigmet {
                id,
                fir_id,
                fir_name,
                hazard: SigmetHazard::from_str_lenient(hazard_str),
                qualifier,
                valid_from,
                valid_to,
                base_altitude_ft: base_alt,
                top_altitude_ft: top_alt,
                polygon,
                movement_dir_deg: props["dir"].as_u64().map(|v| v as u32),
                movement_speed_kts: props["spd"].as_u64().map(|v| v as u32),
                raw_text,
                provider_id: "NOAA_AWC".to_string(),
            });
        }

        Ok(sigmets)
    }

    /// Parse US Domestic AIRMET/SIGMET GeoJSON FeatureCollection.
    pub fn parse_airsigmet_geojson(&self, geojson_str: &str) -> Result<Vec<Sigmet>> {
        let root: Value = serde_json::from_str(geojson_str)?;
        let features = root["features"].as_array()
            .ok_or_else(|| anyhow!("Expected GeoJSON FeatureCollection with 'features' array"))?;

        let now = Utc::now();
        let mut sigmets = Vec::new();

        for (idx, feat) in features.iter().enumerate() {
            let props = &feat["properties"];
            let air_sig_type = props["airSigmetType"].as_str().unwrap_or("SIGMET");
            let hazard_str = props["hazard"].as_str().unwrap_or("CONVECTIVE");

            let valid_from = props["validTimeFrom"].as_str()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&Utc)))
                .unwrap_or(now);

            let valid_to = props["validTimeTo"].as_str()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&Utc)))
                .unwrap_or_else(|| valid_from + chrono::Duration::hours(2));

            let top_alt = props["altitudeHi1"].as_u64().map(|a| a as u32);
            let base_alt = props["altitudeLow1"].as_u64().map(|a| a as u32);

            let geom = &feat["geometry"];
            let mut polygon = Vec::new();
            if let Some(coords_arr) = geom["coordinates"].as_array() {
                let ring = if coords_arr.first().and_then(|v| v.as_array()).is_some() {
                    coords_arr.first().and_then(|v| v.as_array()).unwrap()
                } else {
                    coords_arr
                };
                for pt in ring {
                    if let Some(pt_arr) = pt.as_array() {
                        if pt_arr.len() >= 2 {
                            let lon = pt_arr[0].as_f64().unwrap_or(0.0);
                            let lat = pt_arr[1].as_f64().unwrap_or(0.0);
                            polygon.push((lon, lat));
                        }
                    }
                }
            }

            let series_id = props["seriesId"].as_str().unwrap_or("CONV");
            let id = format!("US:{air_sig_type}:{series_id}:{idx}");

            sigmets.push(Sigmet {
                id,
                fir_id: "US_CONUS".to_string(),
                fir_name: Some("US Continental Airspace".to_string()),
                hazard: SigmetHazard::from_str_lenient(hazard_str),
                qualifier: Some(air_sig_type.to_string()),
                valid_from,
                valid_to,
                base_altitude_ft: base_alt,
                top_altitude_ft: top_alt,
                polygon,
                movement_dir_deg: props["movementDir"].as_u64().map(|v| v as u32),
                movement_speed_kts: props["movementSpd"].as_u64().map(|v| v as u32),
                raw_text: props["rawAirSigmet"].as_str().unwrap_or("").to_string(),
                provider_id: "NOAA_AWC".to_string(),
            });
        }

        Ok(sigmets)
    }

    /// Parse PIREP JSON array.
    pub fn parse_pirep_json(&self, json_str: &str) -> Result<Vec<PirepReport>> {
        let root: Value = serde_json::from_str(json_str)?;
        let items = match root {
            Value::Array(arr) => arr,
            Value::Object(obj) => vec![Value::Object(obj)],
            _ => bail!("Unexpected JSON structure for PIREP list"),
        };

        let now = Utc::now();
        let mut reports = Vec::new();

        for item in items {
            let lat = item["lat"].as_f64().unwrap_or(0.0);
            let lon = item["lon"].as_f64().unwrap_or(0.0);
            let obs_timestamp = item["obsTime"].as_i64();
            let obs_time = if let Some(ts) = obs_timestamp {
                Utc.timestamp_opt(ts, 0).single().unwrap_or(now)
            } else {
                now
            };

            let ac_type = item["acType"].as_str().map(|s| s.to_string());
            let flt_lvl = item["fltLvl"].as_u64().map(|v| v as u32);
            let turb = item["tbType"].as_str().or_else(|| item["tbInt"].as_str()).map(|s| s.to_string());
            let ice = item["icgType"].as_str().or_else(|| item["icgInt"].as_str()).map(|s| s.to_string());
            let temp = item["temp"].as_f64();
            let raw = item["rawOb"].as_str().unwrap_or("").to_string();

            reports.push(PirepReport {
                obs_time,
                aircraft_type: ac_type,
                latitude: lat,
                longitude: lon,
                flight_level: flt_lvl,
                turbulence: turb,
                icing: ice,
                temp_c: temp,
                raw_text: raw,
            });
        }

        Ok(reports)
    }

    #[cfg(feature = "online")]
    pub fn fetch_metars(&self, stations: &[&str]) -> Result<Vec<MetarReport>> {
        if stations.is_empty() {
            return Ok(Vec::new());
        }
        let ids = stations.join(",");
        let url = format!("{}/metar?ids={ids}&format=json", self.base_url);

        let client = reqwest::blocking::Client::builder()
            .user_agent("OpenAIRAC/1.7 (open aviation weather client)")
            .timeout(std::time::Duration::from_secs(15))
            .build()?;

        let resp = client.get(&url).send().with_context(|| format!("Fetching METARs from '{url}'"))?;
        if !resp.status().is_success() {
            bail!("HTTP error {} fetching METARs", resp.status());
        }
        let json_text = resp.text()?;
        self.parse_metar_json(&json_text)
    }

    #[cfg(feature = "online")]
    pub fn fetch_tafs(&self, stations: &[&str]) -> Result<Vec<TafReport>> {
        if stations.is_empty() {
            return Ok(Vec::new());
        }
        let ids = stations.join(",");
        let url = format!("{}/taf?ids={ids}&format=json", self.base_url);

        let client = reqwest::blocking::Client::builder()
            .user_agent("OpenAIRAC/1.7 (open aviation weather client)")
            .timeout(std::time::Duration::from_secs(15))
            .build()?;

        let resp = client.get(&url).send().with_context(|| format!("Fetching TAFs from '{url}'"))?;
        if !resp.status().is_success() {
            bail!("HTTP error {} fetching TAFs", resp.status());
        }
        let json_text = resp.text()?;
        self.parse_taf_json(&json_text)
    }

    #[cfg(feature = "online")]
    pub fn fetch_international_sigmets(&self) -> Result<Vec<Sigmet>> {
        let url = format!("{}/isigmet?format=geojson", self.base_url);
        let client = reqwest::blocking::Client::builder()
            .user_agent("OpenAIRAC/1.7 (open aviation weather client)")
            .timeout(std::time::Duration::from_secs(20))
            .build()?;

        let resp = client.get(&url).send().with_context(|| format!("Fetching International SIGMETs from '{url}'"))?;
        if !resp.status().is_success() {
            bail!("HTTP error {} fetching International SIGMETs", resp.status());
        }
        let json_text = resp.text()?;
        self.parse_isigmet_geojson(&json_text)
    }

    #[cfg(feature = "online")]
    pub fn fetch_us_airsigmets(&self) -> Result<Vec<Sigmet>> {
        let url = format!("{}/airsigmet?format=geojson", self.base_url);
        let client = reqwest::blocking::Client::builder()
            .user_agent("OpenAIRAC/1.7 (open aviation weather client)")
            .timeout(std::time::Duration::from_secs(20))
            .build()?;

        let resp = client.get(&url).send().with_context(|| format!("Fetching US AIRMET/SIGMETs from '{url}'"))?;
        if !resp.status().is_success() {
            bail!("HTTP error {} fetching US AIRMET/SIGMETs", resp.status());
        }
        let json_text = resp.text()?;
        self.parse_airsigmet_geojson(&json_text)
    }

    #[cfg(feature = "online")]
    pub fn fetch_pireps(&self, station: &str, distance_nm: u32) -> Result<Vec<PirepReport>> {
        let url = format!("{}/pirep?id={station}&distance={distance_nm}&format=json", self.base_url);
        let client = reqwest::blocking::Client::builder()
            .user_agent("OpenAIRAC/1.7 (open aviation weather client)")
            .timeout(std::time::Duration::from_secs(15))
            .build()?;

        let resp = client.get(&url).send().with_context(|| format!("Fetching PIREPs from '{url}'"))?;
        if !resp.status().is_success() {
            bail!("HTTP error {} fetching PIREPs", resp.status());
        }
        let json_text = resp.text()?;
        self.parse_pirep_json(&json_text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_METAR_JSON: &str = r#"[
        {
            "icaoId": "KJFK",
            "obsTime": 1787229000,
            "reportTime": "2026-08-20T12:30:00.000Z",
            "temp": 24,
            "dewp": 16,
            "wdir": 220,
            "wspd": 14,
            "wgst": 22,
            "visib": "10+",
            "altim": 1014,
            "fltcat": "VFR",
            "rawOb": "METAR KJFK 201230Z 22014G22KT 10SM FEW050 SCT250 24/16 A2994"
        },
        {
            "icaoId": "LFPG",
            "obsTime": 1787229000,
            "reportTime": "2026-08-20T12:30:00.000Z",
            "temp": 21,
            "dewp": 12,
            "wdir": 270,
            "wspd": 8,
            "visib": "10+",
            "altim": 1018,
            "fltcat": "VFR",
            "rawOb": "METAR LFPG 201230Z 27008KT 9999 BKN040 21/12 Q1018"
        }
    ]"#;

    #[test]
    fn test_parse_sample_metars() {
        let provider = AviationWeatherProvider::new();
        let reports = provider.parse_metar_json(SAMPLE_METAR_JSON).unwrap();
        assert_eq!(reports.len(), 2);

        let jfk = &reports[0];
        assert_eq!(jfk.station_id, "KJFK");
        assert_eq!(jfk.flight_category, FlightCategory::Vfr);
        assert_eq!(jfk.wind_dir_deg, Some(220));
        assert_eq!(jfk.wind_speed_kts, Some(14));
        assert_eq!(jfk.wind_gust_kts, Some(22));

        let lfpg = &reports[1];
        assert_eq!(lfpg.station_id, "LFPG");
        assert_eq!(lfpg.flight_category, FlightCategory::Vfr);
        assert_eq!(lfpg.temp_c, Some(21.0));
    }
}
