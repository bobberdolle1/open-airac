use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightPlanRoute {
    pub origin_icao: String,
    pub destination_icao: String,
    pub waypoints: Vec<String>,
    pub total_distance_nm: f64,
}

pub struct RouteSolver;

impl RouteSolver {
    pub fn build_direct_route(origin: &str, destination: &str) -> FlightPlanRoute {
        FlightPlanRoute {
            origin_icao: origin.to_string(),
            destination_icao: destination.to_string(),
            waypoints: vec![origin.to_string(), destination.to_string()],
            total_distance_nm: 0.0,
        }
    }
}
