//! Route Corridor Geometry and Spatial Hazard Intersection Engine.
//!
//! Provides geodesic distance calculations, ray-casting polygon containment,
//! and route corridor intersection testing against SIGMET polygons and PIREP reports.

use crate::model::{PirepReport, Sigmet};
use serde::{Deserialize, Serialize};

const _NM_PER_DEGREE_LAT: f64 = 60.0;
const EARTH_RADIUS_NM: f64 = 3440.065;

/// Spatial corridor around an active flight plan route.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteCorridor {
    /// Ordered route waypoint positions: `(longitude, latitude)`
    pub waypoints: Vec<(f64, f64)>,
    /// Corridor half-width in nautical miles (default 50.0 NM each side)
    pub half_width_nm: f64,
}

impl Default for RouteCorridor {
    fn default() -> Self {
        Self {
            waypoints: Vec::new(),
            half_width_nm: 50.0,
        }
    }
}

impl RouteCorridor {
    pub fn new(waypoints: Vec<(f64, f64)>) -> Self {
        Self {
            waypoints,
            half_width_nm: 50.0,
        }
    }

    pub fn with_width(mut self, half_width_nm: f64) -> Self {
        self.half_width_nm = half_width_nm;
        self
    }

    /// Calculate Great-Circle distance in nautical miles between two (lon, lat) points.
    pub fn distance_nm(p1: (f64, f64), p2: (f64, f64)) -> f64 {
        let lat1 = p1.1.to_radians();
        let lon1 = p1.0.to_radians();
        let lat2 = p2.1.to_radians();
        let lon2 = p2.0.to_radians();

        let dlat = lat2 - lat1;
        let dlon = lon2 - lon1;

        let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
        let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());

        EARTH_RADIUS_NM * c
    }

    /// Calculate minimum distance from a point to a route line segment in nautical miles.
    pub fn point_to_segment_distance_nm(point: (f64, f64), seg_start: (f64, f64), seg_end: (f64, f64)) -> f64 {
        let seg_len = Self::distance_nm(seg_start, seg_end);
        if seg_len < 1e-4 {
            return Self::distance_nm(point, seg_start);
        }

        // Project point onto segment in flat-earth approximation for cross-track distance
        let px = point.0;
        let py = point.1;
        let x1 = seg_start.0;
        let y1 = seg_start.1;
        let x2 = seg_end.0;
        let y2 = seg_end.1;

        let dx = x2 - x1;
        let dy = y2 - y1;

        let t = (((px - x1) * dx + (py - y1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
        let proj_x = x1 + t * dx;
        let proj_y = y1 + t * dy;

        Self::distance_nm(point, (proj_x, proj_y))
    }

    /// Check if a (lon, lat) point lies inside a 2D polygon using standard ray casting.
    pub fn point_in_polygon(point: (f64, f64), polygon: &[(f64, f64)]) -> bool {
        if polygon.len() < 3 {
            return false;
        }

        let x = point.0;
        let y = point.1;
        let mut inside = false;

        let mut j = polygon.len() - 1;
        for i in 0..polygon.len() {
            let xi = polygon[i].0;
            let yi = polygon[i].1;
            let xj = polygon[j].0;
            let yj = polygon[j].1;

            let intersect = ((yi > y) != (yj > y)) && (x < (xj - xi) * (y - yi) / (yj - yi) + xi);
            if intersect {
                inside = !inside;
            }
            j = i;
        }

        inside
    }

    /// Check if two line segments intersect.
    pub fn segments_intersect(p1: (f64, f64), p2: (f64, f64), q1: (f64, f64), q2: (f64, f64)) -> bool {
        fn ccw(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> bool {
            (c.1 - a.1) * (b.0 - a.0) > (b.1 - a.1) * (c.0 - a.0)
        }
        (ccw(p1, q1, q2) != ccw(p2, q1, q2)) && (ccw(p1, p2, q1) != ccw(p1, p2, q2))
    }

    /// Test if this route corridor intersects a given polygon.
    pub fn intersects_polygon(&self, polygon: &[(f64, f64)]) -> bool {
        if self.waypoints.is_empty() || polygon.len() < 3 {
            return false;
        }

        // 1. Any route waypoint inside polygon?
        for wpt in &self.waypoints {
            if Self::point_in_polygon(*wpt, polygon) {
                return true;
            }
        }

        // 2. Any polygon vertex within corridor half-width of any route segment?
        for v in polygon {
            if self.distance_to_route(*v) <= self.half_width_nm {
                return true;
            }
        }

        // 3. Direct route line segment intersections with polygon edges
        for w_idx in 0..self.waypoints.len().saturating_sub(1) {
            let seg_p1 = self.waypoints[w_idx];
            let seg_p2 = self.waypoints[w_idx + 1];

            for p_idx in 0..polygon.len() {
                let next_idx = (p_idx + 1) % polygon.len();
                if Self::segments_intersect(seg_p1, seg_p2, polygon[p_idx], polygon[next_idx]) {
                    return true;
                }
            }
        }

        false
    }

    /// Calculate minimum distance from a point to the entire route.
    pub fn distance_to_route(&self, point: (f64, f64)) -> f64 {
        if self.waypoints.is_empty() {
            return f64::MAX;
        }
        if self.waypoints.len() == 1 {
            return Self::distance_nm(point, self.waypoints[0]);
        }

        let mut min_dist = f64::MAX;
        for i in 0..self.waypoints.len() - 1 {
            let d = Self::point_to_segment_distance_nm(point, self.waypoints[i], self.waypoints[i + 1]);
            if d < min_dist {
                min_dist = d;
            }
        }
        min_dist
    }

    /// Find all SIGMETs intersecting this route corridor.
    pub fn filter_intersecting_sigmets<'a>(&self, sigmets: &'a [Sigmet]) -> Vec<&'a Sigmet> {
        sigmets
            .iter()
            .filter(|s| !s.polygon.is_empty() && self.intersects_polygon(&s.polygon))
            .collect()
    }

    /// Find all PIREPs within the corridor radius.
    pub fn filter_nearby_pireps<'a>(&self, pireps: &'a [PirepReport]) -> Vec<&'a PirepReport> {
        pireps
            .iter()
            .filter(|p| self.distance_to_route((p.longitude, p.latitude)) <= self.half_width_nm)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SigmetHazard;
    use chrono::Utc;

    #[test]
    fn test_corridor_polygon_intersection() {
        // Route from KSFO to KJFK (simplified 3 points)
        let corridor = RouteCorridor::new(vec![
            (-122.375, 37.618), // KSFO
            (-87.904, 41.974),  // KORD
            (-73.778, 40.639),  // KJFK
        ]);

        // 1. Polygon over Chicago directly intersecting route
        let chicago_storm = vec![
            (-88.5, 41.5),
            (-87.5, 41.5),
            (-87.5, 42.5),
            (-88.5, 42.5),
        ];
        assert!(corridor.intersects_polygon(&chicago_storm));

        // 2. Polygon in Texas (far south, no intersection)
        let texas_storm = vec![
            (-98.0, 30.0),
            (-96.0, 30.0),
            (-96.0, 32.0),
            (-98.0, 32.0),
        ];
        assert!(!corridor.intersects_polygon(&texas_storm));
    }

    #[test]
    fn test_filter_sigmets_and_pireps() {
        let corridor = RouteCorridor::new(vec![(-74.0, 40.0), (-73.0, 41.0)]).with_width(30.0);

        let sig = Sigmet {
            id: "SIG-1".to_string(),
            fir_id: "KZNY".to_string(),
            fir_name: None,
            hazard: SigmetHazard::Convective,
            qualifier: Some("EMBD".to_string()),
            valid_from: Utc::now(),
            valid_to: Utc::now(),
            base_altitude_ft: None,
            top_altitude_ft: Some(45000),
            polygon: vec![(-74.2, 39.8), (-73.5, 39.8), (-73.5, 40.5), (-74.2, 40.5)],
            movement_dir_deg: None,
            movement_speed_kts: None,
            raw_text: "SIGMET 1".to_string(),
            provider_id: "NOAA".to_string(),
        };

        let sigs = vec![sig];
        let matches = corridor.filter_intersecting_sigmets(&sigs);
        assert_eq!(matches.len(), 1);
    }
}
