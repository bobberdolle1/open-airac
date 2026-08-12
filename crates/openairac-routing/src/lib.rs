use anyhow::{Result, anyhow};
use geo::{Bearing, Distance, Geodesic, Point};
use serde::{Deserialize, Serialize};

/// Geographic Coordinate (WGS84)
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Coordinate {
    pub latitude_deg: f64,
    pub longitude_deg: f64,
}

impl Coordinate {
    pub fn new(latitude_deg: f64, longitude_deg: f64) -> Result<Self> {
        if !(-90.0..=90.0).contains(&latitude_deg) {
            return Err(anyhow!("Latitude {:.4}° is invalid", latitude_deg));
        }
        if !(-180.0..=180.0).contains(&longitude_deg) {
            return Err(anyhow!("Longitude {:.4}° is invalid", longitude_deg));
        }
        Ok(Self {
            latitude_deg,
            longitude_deg,
        })
    }

    pub fn to_geo_point(&self) -> Point<f64> {
        Point::new(self.longitude_deg, self.latitude_deg)
    }
}

/// Direct Geodesic Route calculation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectRoute {
    pub origin: Coordinate,
    pub destination: Coordinate,
    pub distance_meters: f64,
    pub distance_nm: f64,
    pub initial_bearing_deg: f64,
}

impl DirectRoute {
    /// Calculate direct geodesic route between two geographic coordinates
    pub fn between(origin: Coordinate, destination: Coordinate) -> Self {
        let p1 = origin.to_geo_point();
        let p2 = destination.to_geo_point();

        let distance_meters = Geodesic::distance(p1, p2);
        let distance_nm = distance_meters / 1852.0;

        let bearing = Geodesic::bearing(p1, p2);
        let initial_bearing_deg = (bearing + 360.0) % 360.0;

        Self {
            origin,
            destination,
            distance_meters,
            distance_nm,
            initial_bearing_deg,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // KSFO (37.6188, -122.3750) -> KJFK (40.6398, -73.7789)
    #[test]
    fn test_ksfo_to_kjfk_geodesic_distance() {
        let origin = Coordinate::new(37.6188, -122.3750).unwrap();
        let destination = Coordinate::new(40.6398, -73.7789).unwrap();

        let route = DirectRoute::between(origin, destination);
        // Known geodesic distance between SFO and JFK is approx 2244 NM
        assert!((route.distance_nm - 2244.0).abs() < 20.0);
        // Initial heading is approx 65-75 degrees
        assert!((route.initial_bearing_deg - 67.0).abs() < 10.0);
    }
}
