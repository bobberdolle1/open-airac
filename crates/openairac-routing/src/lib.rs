//! OpenAIRAC routing: geodesic primitives plus the canonical airway graph.
//!
//! Graph semantics:
//! * Nodes carry globally unambiguous identities (`ident` + ICAO region +
//!   entity class); an enroute fix and a navaid with the same ident/region
//!   are different nodes.
//! * Edges come from canonical airway legs only (the temporal store's
//!   `airway_legs` table). Legs whose endpoints are missing from the
//!   fix/navaid sets are skipped with diagnostics — the graph never
//!   invents nodes.
//! * Direction semantics verified against the FAA CIFP (cycle 2608) and
//!   convert424toxplane output: US airway records carry no directional
//!   restrictions (the FAA CIFP Readme states ATS routes do not contain
//!   them), so edges are bidirectional. `F`/`B` are honored when a source
//!   provides them.
//! * Temporal validity is enforced at construction: the caller supplies
//!   entities valid at the requested time; nothing invalid at that time
//!   can appear in the graph.
//! * Altitude semantics: an edge is usable for a cruise altitude only when
//!   the altitude lies within the segment's MEA/maximum-altitude band.
//!
//! No contraction hierarchies: plain Dijkstra/A* with a geodesic
//! heuristic. Geodesics stay separate from graph policy.

use anyhow::{Result, anyhow, bail};
use chrono::{DateTime, Utc};
use geo::{Bearing, Distance, Geodesic, Point};
use openairac_model::{CanonicalAirwayLeg, CanonicalNavaid, CanonicalWaypoint};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, VecDeque};

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

    /// Great-circle distance between two coordinates, nautical miles.
    pub fn distance_nm(&self, other: &Coordinate) -> f64 {
        Geodesic::distance(self.to_geo_point(), other.to_geo_point()) / 1852.0
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

// ---------------------------------------------------------------------------
// Routing graph
// ---------------------------------------------------------------------------

/// Class of a graph node (XPAWY1101 typing: 11 fix, 2 NDB, 3 VHF).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum NodeKind {
    Fix,
    Vhf,
    Ndb,
}

/// Globally unambiguous node identity: ident + ICAO region + class.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeId {
    pub ident: String,
    pub region: String,
    pub kind: NodeKind,
}

impl NodeId {
    pub fn fix(ident: &str, region: &str) -> Self {
        Self {
            ident: ident.trim().to_string(),
            region: region.to_string(),
            kind: NodeKind::Fix,
        }
    }

    pub fn navaid(ident: &str, region: &str, kind: NodeKind) -> Self {
        Self {
            ident: ident.trim().to_string(),
            region: region.to_string(),
            kind,
        }
    }

    /// Display form for diagnostics.
    pub fn display(&self) -> String {
        format!("{}/{}/{}", self.ident, self.region.trim(), kind_str(self.kind))
    }
}

fn kind_str(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Fix => "fix",
        NodeKind::Vhf => "vhf",
        NodeKind::Ndb => "ndb",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: NodeId,
    pub latitude: f64,
    pub longitude: f64,
    pub name: String,
}

impl GraphNode {
    fn coordinate(&self) -> Coordinate {
        Coordinate {
            latitude_deg: self.latitude,
            longitude_deg: self.longitude,
        }
    }
}

/// One directed airway edge. A canonical leg yields two edges unless a
/// directional restriction (`F`/`B`) applies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirwayEdge {
    pub from: NodeId,
    pub to: NodeId,
    pub route_ident: String,
    /// ARINC route type: `O` conventional, `R` RNAV.
    pub route_type: String,
    /// Published level: `H`/`L`.
    pub level: char,
    pub minimum_altitude_ft: Option<u32>,
    pub maximum_altitude_ft: Option<u32>,
    pub distance_nm: f64,
}

/// Aircraft capabilities affecting edge eligibility.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AircraftCapabilities {
    /// The aircraft can fly RNAV routes.
    pub rnav: bool,
}

/// A node or route to avoid during planning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Exclusion {
    Node(NodeId),
    Route(String),
}

/// A routing request against the airway graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteRequest {
    pub origin: NodeId,
    pub destination: NodeId,
    /// All graph entities must be valid at this instant.
    pub departure_time: DateTime<Utc>,
    /// Planned cruise altitude; edges whose altitude band does not contain
    /// it are ineligible.
    pub cruise_altitude_ft: Option<u32>,
    pub aircraft_capabilities: AircraftCapabilities,
    pub exclusions: Vec<Exclusion>,
}

/// One leg of a computed route (a traversed airway edge).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteLeg {
    pub from: NodeId,
    pub to: NodeId,
    pub route_ident: String,
    pub level: char,
    pub distance_nm: f64,
}

/// Structured routing outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteResult {
    pub success: bool,
    pub legs: Vec<RouteLeg>,
    pub nodes: Vec<GraphNode>,
    pub total_distance_nm: f64,
    pub diagnostics: Vec<String>,
}

/// The canonical airway graph for one instant in time.
pub struct AirwayGraph {
    nodes: HashMap<NodeId, GraphNode>,
    edges: Vec<AirwayEdge>,
    adjacency: HashMap<NodeId, Vec<usize>>,
    valid_at: DateTime<Utc>,
}

impl AirwayGraph {
    /// Build the graph from canonical entities valid at `time`.
    ///
    /// Returns the graph plus diagnostics for airway legs that were skipped
    /// (missing endpoints). Temporal filtering is the caller's job: pass
    /// entities queried at `time` — nothing else can enter the graph.
    pub fn build(
        fixes: &[CanonicalWaypoint],
        navaids: &[CanonicalNavaid],
        legs: &[CanonicalAirwayLeg],
        time: DateTime<Utc>,
    ) -> (Self, Vec<String>) {
        let mut nodes: HashMap<NodeId, GraphNode> = HashMap::new();
        for wp in fixes {
            let id = NodeId::fix(&wp.ident, &wp.region_code);
            nodes.insert(
                id.clone(),
                GraphNode {
                    id,
                    latitude: wp.latitude,
                    longitude: wp.longitude,
                    name: wp.name.clone(),
                },
            );
        }
        for nav in navaids {
            let Some(region) = nav.region_code.clone() else {
                continue;
            };
            let kind = match nav.kind {
                openairac_model::NavaidKind::Ndb => NodeKind::Ndb,
                _ => NodeKind::Vhf,
            };
            let id = NodeId::navaid(&nav.ident, &region, kind);
            nodes.entry(id.clone()).or_insert_with(|| GraphNode {
                id,
                latitude: nav.latitude,
                longitude: nav.longitude,
                name: nav.name.clone(),
            });
        }

        let mut edges: Vec<AirwayEdge> = Vec::new();
        let mut diagnostics = Vec::new();
        for leg in legs {
            let start = NodeId::fix(&leg.start_fix, &leg.start_icao_code);
            let end = NodeId::fix(&leg.end_fix, &leg.end_icao_code);
            let (Some(from), Some(to)) = (nodes.get(&start), nodes.get(&end)) else {
                diagnostics.push(format!(
                    "airway {}: endpoint {}/{} or {}/{} not in node set",
                    leg.route_ident, leg.start_fix, leg.start_icao_code, leg.end_fix, leg.end_icao_code
                ));
                continue;
            };
            let distance_nm = from.coordinate().distance_nm(&to.coordinate());
            let push = |a: &GraphNode, b: &GraphNode, edges: &mut Vec<AirwayEdge>| {
                edges.push(AirwayEdge {
                    from: a.id.clone(),
                    to: b.id.clone(),
                    route_ident: leg.route_ident.clone(),
                    route_type: leg.route_type.clone(),
                    level: leg.level.unwrap_or(' '),
                    minimum_altitude_ft: leg.minimum_altitude_ft,
                    maximum_altitude_ft: leg.maximum_altitude_ft,
                    distance_nm,
                });
            };
            match leg.direction {
                // Verified against FAA CIFP cycle 2608 + convert424toxplane:
                // US airway records carry no directional restrictions.
                'N' | ' ' | '\0' => {
                    push(from, to, &mut edges);
                    push(to, from, &mut edges);
                }
                'F' => push(from, to, &mut edges),
                'B' => push(to, from, &mut edges),
                other => {
                    diagnostics.push(format!(
                        "airway {}: unrecognized direction restriction '{other}' treated as bidirectional",
                        leg.route_ident
                    ));
                    push(from, to, &mut edges);
                    push(to, from, &mut edges);
                }
            }
        }

        let mut adjacency: HashMap<NodeId, Vec<usize>> = HashMap::new();
        for (i, edge) in edges.iter().enumerate() {
            adjacency.entry(edge.from.clone()).or_default().push(i);
        }

        (
            Self {
                nodes,
                edges,
                adjacency,
                valid_at: time,
            },
            diagnostics,
        )
    }

    pub fn valid_at(&self) -> DateTime<Utc> {
        self.valid_at
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn node(&self, id: &NodeId) -> Option<&GraphNode> {
        self.nodes.get(id)
    }

    /// Whether an edge is usable for the request's aircraft and altitude.
    fn edge_eligible(&self, edge: &AirwayEdge, request: &RouteRequest) -> bool {
        // Conventional routes require a ground-based capable aircraft? A
        // non-RNAV aircraft cannot fly RNAV routes.
        if edge.route_type == "R" && !request.aircraft_capabilities.rnav {
            return false;
        }
        if let Some(alt) = request.cruise_altitude_ft {
            if let Some(min) = edge.minimum_altitude_ft {
                if alt < min {
                    return false;
                }
            }
            if let Some(max) = edge.maximum_altitude_ft {
                if alt > max {
                    return false;
                }
            }
        }
        if request
            .exclusions
            .iter()
            .any(|e| matches!(e, Exclusion::Route(r) if r == &edge.route_ident))
        {
            return false;
        }
        if request
            .exclusions
            .iter()
            .any(|e| matches!(e, Exclusion::Node(n) if n == &edge.to))
        {
            return false;
        }
        true
    }

    /// Dijkstra/A* over the directed edge view with a geodesic heuristic.
    pub fn route(&self, request: &RouteRequest) -> RouteResult {
        let mut diagnostics = Vec::new();
        let origin = request.origin.clone();
        let destination = request.destination.clone();

        if request
            .exclusions
            .iter()
            .any(|e| matches!(e, Exclusion::Node(n) if n == &origin || n == &destination))
        {
            diagnostics.push("origin or destination is excluded".to_string());
            return RouteResult {
                success: false,
                legs: Vec::new(),
                nodes: Vec::new(),
                total_distance_nm: 0.0,
                diagnostics,
            };
        }

        let Some(start_node) = self.nodes.get(&origin) else {
            diagnostics.push(format!("origin {} not in graph", origin.display()));
            return RouteResult {
                success: false,
                legs: Vec::new(),
                nodes: Vec::new(),
                total_distance_nm: 0.0,
                diagnostics,
            };
        };
        let Some(end_node) = self.nodes.get(&destination) else {
            diagnostics.push(format!("destination {} not in graph", destination.display()));
            return RouteResult {
                success: false,
                legs: Vec::new(),
                nodes: Vec::new(),
                total_distance_nm: 0.0,
                diagnostics,
            };
        };

        // A* over edge indices.
        let goal = end_node.coordinate();
        let mut dist: HashMap<NodeId, f64> = HashMap::new();
        let mut prev: HashMap<NodeId, Option<(NodeId, usize)>> = HashMap::new();
        dist.insert(origin.clone(), 0.0);

        struct HeapItem(NodeId, f64);
        impl PartialEq for HeapItem {
            fn eq(&self, other: &Self) -> bool {
                self.1 == other.1
            }
        }
        impl Eq for HeapItem {}
        impl PartialOrd for HeapItem {
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                Some(self.cmp(other))
            }
        }
        impl Ord for HeapItem {
            fn cmp(&self, other: &Self) -> Ordering {
                // Reverse for min-heap.
                other.1.total_cmp(&self.1)
            }
        }

        let mut heap = BinaryHeap::new();
        heap.push(HeapItem(origin.clone(), 0.0));

        while let Some(HeapItem(current, _)) = heap.pop() {
            if current == destination {
                break;
            }
            let Some(d) = dist.get(&current).copied() else {
                continue;
            };
            let Some(out_edges) = self.adjacency.get(&current) else {
                continue;
            };
            for &edge_idx in out_edges {
                let edge = &self.edges[edge_idx];
                if !self.edge_eligible(edge, request) {
                    continue;
                }
                let Some(to_node) = self.nodes.get(&edge.to) else {
                    continue;
                };
                let nd = d + edge.distance_nm;
                if nd < dist.get(&edge.to).copied().unwrap_or(f64::INFINITY) {
                    dist.insert(edge.to.clone(), nd);
                    prev.insert(edge.to.clone(), Some((current.clone(), edge_idx)));
                    let heuristic = to_node.coordinate().distance_nm(&goal);
                    heap.push(HeapItem(edge.to.clone(), nd + heuristic));
                }
            }
        }

        let Some(total) = dist.get(&destination).copied() else {
            diagnostics.push(format!(
                "no route from {} to {} in the airway graph",
                origin.display(),
                destination.display()
            ));
            return RouteResult {
                success: false,
                legs: Vec::new(),
                nodes: Vec::new(),
                total_distance_nm: 0.0,
                diagnostics,
            };
        };

        // Reconstruct.
        let mut legs = Vec::new();
        let mut nodes = Vec::new();
        let mut cursor = destination.clone();
        while let Some(Some((from, edge_idx))) = prev.get(&cursor) {
            let edge = &self.edges[*edge_idx];
            legs.push(RouteLeg {
                from: edge.from.clone(),
                to: edge.to.clone(),
                route_ident: edge.route_ident.clone(),
                level: edge.level,
                distance_nm: edge.distance_nm,
            });
            cursor = from.clone();
        }
        legs.reverse();
        nodes.push(start_node.clone());
        for leg in &legs {
            if let Some(n) = self.nodes.get(&leg.to) {
                nodes.push(n.clone());
            }
        }

        RouteResult {
            success: true,
            legs,
            nodes,
            total_distance_nm: total,
            diagnostics,
        }
    }

    /// Connected components of the graph (BFS over the directed edges).
    /// Returns the number of components and their sizes, sorted descending.
    pub fn disconnected_components(&self) -> Vec<usize> {
        let mut visited: HashMap<NodeId, bool> = HashMap::new();
        let mut components = Vec::new();
        for start in self.nodes.keys() {
            if visited.get(start).copied().unwrap_or(false) {
                continue;
            }
            let mut size = 0usize;
            let mut queue = VecDeque::new();
            queue.push_back(start.clone());
            visited.insert(start.clone(), true);
            while let Some(current) = queue.pop_front() {
                size += 1;
                if let Some(out_edges) = self.adjacency.get(&current) {
                    for &edge_idx in out_edges {
                        let to = &self.edges[edge_idx].to;
                        if !visited.get(to).copied().unwrap_or(false) {
                            visited.insert(to.clone(), true);
                            queue.push_back(to.clone());
                        }
                    }
                }
            }
            components.push(size);
        }
        components.sort_by(|a, b| b.cmp(a));
        components
    }
}

/// Build a graph from canonical entities and fail closed when it is empty.
pub fn build_graph_or_bail(
    fixes: &[CanonicalWaypoint],
    navaids: &[CanonicalNavaid],
    legs: &[CanonicalAirwayLeg],
    time: DateTime<Utc>,
) -> Result<AirwayGraph> {
    let (graph, diagnostics) = AirwayGraph::build(fixes, navaids, legs, time);
    if graph.node_count() == 0 {
        bail!("airway graph is empty at {time}");
    }
    if !diagnostics.is_empty() {
        tracing::warn!(
            "{} airway legs skipped while building the graph",
            diagnostics.len()
        );
    }
    Ok(graph)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeDelta;
    use openairac_model::{
        AirwayLegId, NavaidId, NavaidKind, SourceSnapshotId, TemporalValidity, WaypointId,
    };

    fn temporal(t: DateTime<Utc>) -> TemporalValidity {
        TemporalValidity {
            valid_from: t,
            valid_until: None,
            source_snapshot_id: SourceSnapshotId("snap-test".to_string()),
        }
    }

    fn fix(ident: &str, region: &str, lat: f64, lon: f64, t: DateTime<Utc>) -> CanonicalWaypoint {
        CanonicalWaypoint {
            object_id: WaypointId(format!("WP-{ident}-{region}")),
            ident: ident.to_string(),
            name: ident.to_string(),
            latitude: lat,
            longitude: lon,
            is_enroute: true,
            region_code: region.to_string(),
            terminal_area_ident: None,
            waypoint_type: Some(0x202057),
            temporal: temporal(t),
        }
    }

    fn leg(
        route: &str,
        start: &str,
        start_region: &str,
        end: &str,
        end_region: &str,
        level: Option<char>,
        direction: char,
        min_ft: Option<u32>,
        max_ft: Option<u32>,
        t: DateTime<Utc>,
    ) -> CanonicalAirwayLeg {
        CanonicalAirwayLeg {
            object_id: AirwayLegId(format!("LEG-{route}-{start}-{end}")),
            route_ident: route.to_string(),
            route_type: "O".to_string(),
            level,
            sequence_number: 2,
            start_fix: start.to_string(),
            start_icao_code: start_region.to_string(),
            end_fix: end.to_string(),
            end_icao_code: end_region.to_string(),
            direction,
            minimum_altitude_ft: min_ft,
            maximum_altitude_ft: max_ft,
            temporal: temporal(t),
        }
    }

    fn request(origin: NodeId, destination: NodeId, t: DateTime<Utc>) -> RouteRequest {
        RouteRequest {
            origin,
            destination,
            departure_time: t,
            cruise_altitude_ft: None,
            aircraft_capabilities: AircraftCapabilities { rnav: true },
            exclusions: Vec::new(),
        }
    }

    #[test]
    fn test_direct_route_ksfo_to_kjfk() {
        let origin = Coordinate::new(37.6188, -122.3750).unwrap();
        let destination = Coordinate::new(40.6398, -73.7789).unwrap();
        let route = DirectRoute::between(origin, destination);
        assert!((route.distance_nm - 2244.0).abs() < 20.0);
        assert!((route.initial_bearing_deg - 67.0).abs() < 10.0);
    }

    #[test]
    fn test_graph_build_and_route() {
        let t = Utc::now();
        let fixes = vec![
            fix("AAA", "K1", 40.0, -80.0, t),
            fix("BBB", "K1", 41.0, -81.0, t),
            fix("CCC", "K1", 42.0, -82.0, t),
        ];
        let legs = vec![
            leg("V1", "AAA", "K1", "BBB", "K1", Some('L'), 'N', None, None, t),
            leg("V2", "BBB", "K1", "CCC", "K1", Some('L'), 'N', None, None, t),
        ];
        let (graph, diagnostics) = AirwayGraph::build(&fixes, &[], &legs, t);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(graph.node_count(), 3);
        assert_eq!(graph.edge_count(), 4); // bidirectional

        let result = graph.route(&request(
            NodeId::fix("AAA", "K1"),
            NodeId::fix("CCC", "K1"),
            t,
        ));
        assert!(result.success);
        assert_eq!(result.legs.len(), 2);
        assert_eq!(result.legs[0].route_ident, "V1");
        assert_eq!(result.legs[1].route_ident, "V2");
        assert_eq!(result.nodes.len(), 3);
    }

    #[test]
    fn test_direction_restriction_honored() {
        let t = Utc::now();
        let fixes = vec![fix("AAA", "K1", 40.0, -80.0, t), fix("BBB", "K1", 41.0, -81.0, t)];
        let legs = vec![leg("V9", "AAA", "K1", "BBB", "K1", Some('L'), 'F', None, None, t)];
        let (graph, _) = AirwayGraph::build(&fixes, &[], &legs, t);
        assert_eq!(graph.edge_count(), 1);

        let forward = graph.route(&request(NodeId::fix("AAA", "K1"), NodeId::fix("BBB", "K1"), t));
        assert!(forward.success);

        let backward = graph.route(&request(NodeId::fix("BBB", "K1"), NodeId::fix("AAA", "K1"), t));
        assert!(!backward.success);
    }

    #[test]
    fn test_mea_and_cruise_altitude_filter() {
        let t = Utc::now();
        let fixes = vec![
            fix("AAA", "K1", 40.0, -80.0, t),
            fix("BBB", "K1", 41.0, -81.0, t),
            fix("CCC", "K1", 42.0, -82.0, t),
        ];
        let legs = vec![
            // low airway: usable below 10000 ft
            leg("V1", "AAA", "K1", "BBB", "K1", Some('L'), 'N', Some(3000), Some(10000), t),
            // high airway: usable at/above 18000 ft
            leg("J1", "BBB", "K1", "CCC", "K1", Some('H'), 'N', Some(18000), Some(45000), t),
        ];
        let (graph, _) = AirwayGraph::build(&fixes, &[], &legs, t);

        let mut low_req = request(NodeId::fix("AAA", "K1"), NodeId::fix("CCC", "K1"), t);
        low_req.cruise_altitude_ft = Some(6000);
        let low = graph.route(&low_req);
        assert!(!low.success, "6000 ft cannot use the J1 segment");

        let mut high_req = request(NodeId::fix("AAA", "K1"), NodeId::fix("CCC", "K1"), t);
        high_req.cruise_altitude_ft = Some(20000);
        let high = graph.route(&high_req);
        assert!(!high.success, "20000 ft cannot use the V1 segment");

        // No cruise altitude: any path is allowed.
        let any = graph.route(&request(NodeId::fix("AAA", "K1"), NodeId::fix("CCC", "K1"), t));
        assert!(any.success);
    }

    #[test]
    fn test_exclusions() {
        let t = Utc::now();
        let fixes = vec![
            fix("AAA", "K1", 40.0, -80.0, t),
            fix("BBB", "K1", 41.0, -81.0, t),
            fix("CCC", "K1", 42.0, -82.0, t),
            fix("DDD", "K1", 43.0, -83.0, t),
        ];
        let legs = vec![
            leg("V1", "AAA", "K1", "BBB", "K1", Some('L'), 'N', None, None, t),
            leg("V2", "BBB", "K1", "CCC", "K1", Some('L'), 'N', None, None, t),
            leg("V2", "CCC", "K1", "DDD", "K1", Some('L'), 'N', None, None, t),
            leg("V3", "BBB", "K1", "DDD", "K1", Some('L'), 'N', None, None, t),
        ];
        let (graph, _) = AirwayGraph::build(&fixes, &[], &legs, t);

        let mut req = request(NodeId::fix("AAA", "K1"), NodeId::fix("DDD", "K1"), t);
        req.exclusions.push(Exclusion::Route("V2".to_string()));
        let result = graph.route(&req);
        assert!(result.success);
        assert_eq!(result.legs.len(), 2);
        assert_eq!(result.legs[0].route_ident, "V1");
        assert_eq!(result.legs[1].route_ident, "V3");
    }

    #[test]
    fn test_rnav_capability_gate() {
        let t = Utc::now();
        let fixes = vec![fix("AAA", "K1", 40.0, -80.0, t), fix("BBB", "K1", 41.0, -81.0, t)];
        let mut rnaav_leg = leg("Q1", "AAA", "K1", "BBB", "K1", Some('H'), 'N', None, None, t);
        rnaav_leg.route_type = "R".to_string();
        let (graph, _) = AirwayGraph::build(&fixes, &[], &[rnaav_leg], t);

        let mut non_rnav = request(NodeId::fix("AAA", "K1"), NodeId::fix("BBB", "K1"), t);
        non_rnav.aircraft_capabilities.rnav = false;
        assert!(!graph.route(&non_rnav).success);

        let mut rnav_ok = request(NodeId::fix("AAA", "K1"), NodeId::fix("BBB", "K1"), t);
        rnav_ok.aircraft_capabilities.rnav = true;
        assert!(graph.route(&rnav_ok).success);
    }

    #[test]
    fn test_temporal_validity_is_respected() {
        let t0 = Utc::now();
        let t_future = t0 + TimeDelta::seconds(3600);

        // A fix that only becomes valid in the future must not be routable
        // through a graph built from `t0` entities.
        let fixes_t0 = vec![fix("AAA", "K1", 40.0, -80.0, t0)];
        let fixes_future = vec![fix("BBB", "K1", 41.0, -81.0, t_future)];
        let legs_t0 = vec![leg("V1", "AAA", "K1", "BBB", "K1", Some('L'), 'N', None, None, t0)];
        let (graph, diagnostics) = AirwayGraph::build(&fixes_t0, &[], &legs_t0, t0);
        // The leg's end fix is missing from the t0 node set -> skipped.
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(graph.node_count(), 1);

        // At the future time, both fixes exist.
        let mut all_fixes = fixes_t0.clone();
        all_fixes.extend(fixes_future);
        let (graph2, diagnostics2) = AirwayGraph::build(&all_fixes, &[], &legs_t0, t_future);
        assert!(diagnostics2.is_empty(), "{diagnostics2:?}");
        assert_eq!(graph2.node_count(), 2);
    }

    #[test]
    fn test_disconnected_components() {
        let t = Utc::now();
        let fixes = vec![
            fix("AAA", "K1", 40.0, -80.0, t),
            fix("BBB", "K1", 41.0, -81.0, t),
            fix("CCC", "K1", 42.0, -82.0, t),
            fix("DDD", "K1", 43.0, -83.0, t),
        ];
        let legs = vec![leg("V1", "AAA", "K1", "BBB", "K1", Some('L'), 'N', None, None, t)];
        let (graph, _) = AirwayGraph::build(&fixes, &[], &legs, t);
        let components = graph.disconnected_components();
        assert_eq!(components.len(), 3); // {AAA,BBB}, {CCC}, {DDD}
        assert_eq!(components[0], 2);
    }
}
