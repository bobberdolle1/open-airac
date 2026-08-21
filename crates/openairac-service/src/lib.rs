//! OpenAIRAC service boundary: UI-independent world queries.
//!
//! Everything a user-facing layer (CLI, web UI, plugin) needs, behind one
//! synchronous, JSON-serializable API. No UI assumptions, no rendering,
//! no network: reads the canonical temporal store and the planning layers.

use anyhow::Result;
use chrono::{DateTime, Utc};
use openairac_integration::{FlightPlan, FlightPlanRequest, Planner};
use openairac_model::{
    CanonicalAirport, CanonicalEntityId, CanonicalNavaid, CanonicalWaypoint, NavaidKind,
    ReconciliationConflict, ReconciliationStats, ResolvedEntity, SourceEntityRef, StoreStatus,
};
use openairac_procedures::ProcedureKind;
use openairac_routing::Coordinate;
use openairac_store::WorldStore;
use serde::{Deserialize, Serialize};

/// Snapshot of the world as of one instant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldAt {
    pub as_of: DateTime<Utc>,
    pub airports: usize,
    pub runways: usize,
    pub navaids: usize,
    pub waypoints: usize,
    pub airway_legs: usize,
    pub procedure_legs: usize,
    pub migration_version: u32,
}

impl WorldAt {
    fn new(
        as_of: DateTime<Utc>,
        status: &StoreStatus,
        waypoints: usize,
        airway_legs: usize,
        procedure_legs: usize,
    ) -> Self {
        Self {
            as_of,
            airports: status.total_airports,
            runways: status.total_runways,
            navaids: status.total_navaids,
            waypoints,
            airway_legs,
            procedure_legs,
            migration_version: status.migration_version,
        }
    }
}

/// A named entity with a position, for proximity answers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NearbyEntity {
    pub ident: String,
    pub name: String,
    pub kind: String,
    pub latitude: f64,
    pub longitude: f64,
    pub distance_nm: f64,
}

/// Summary of one published airway.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirwaySummary {
    pub route_ident: String,
    pub route_type: String,
    pub level: Option<char>,
    pub legs: usize,
    pub first_fix: String,
    pub last_fix: String,
}

/// Summary of one published procedure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcedureSummary {
    pub airport_ident: String,
    pub kind: String,
    pub ident: String,
    pub legs: usize,
    pub transitions: Vec<String>,
}

/// Reconciliation bookkeeping counts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconciliationStatus {
    pub canonical_entities: usize,
    pub memberships: usize,
    pub conflicts: usize,
}

/// Per-provider coverage facts at one instant (v0.6).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCoverage {
    pub provider: String,
    pub coverage: String,
    pub temporal: String,
    pub update: String,
    pub authority_note: String,
    pub airports: usize,
    pub runways: usize,
    pub navaids: usize,
    pub waypoints: usize,
    pub airway_legs: usize,
    pub procedure_legs: usize,
    pub ils_associations: usize,
    pub snapshots: usize,
    /// Newest snapshot retrieval time (RFC3339).
    pub freshest_retrieved_at: Option<String>,
    /// Reconciliation conflicts involving this provider's entities.
    pub conflicts: usize,
}

/// Aggregated coverage report (v0.6).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageReport {
    pub as_of: DateTime<Utc>,
    pub providers: Vec<ProviderCoverage>,
    /// Airports per ISO country (from the store, all providers).
    pub airports_by_country: Vec<(String, usize)>,
}

/// Details of one runway for airport coverage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunwayDetail {
    pub designator: String,
    pub length_ft: u32,
    pub width_ft: Option<u32>,
    pub surface: Option<String>,
}

/// Source provenance record for coverage inspector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceProvenanceInfo {
    pub provider: String,
    pub dataset: String,
    pub airac_cycle: Option<String>,
    pub effective_from: Option<String>,
    pub license_id: String,
    pub redistribution: String,
}

/// Detailed coverage report for a specific airport.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirportCoverageReport {
    pub ident: String,
    pub name: String,
    pub airport_type: String,
    pub latitude: f64,
    pub longitude: f64,
    pub elevation_ft: Option<f64>,
    pub country: Option<String>,
    pub municipality: Option<String>,
    pub runways: Vec<RunwayDetail>,
    pub navaids_count: usize,
    pub sids_count: usize,
    pub stars_count: usize,
    pub approaches_count: usize,
    pub sids: Vec<String>,
    pub stars: Vec<String>,
    pub approaches: Vec<String>,
    pub sources: Vec<SourceProvenanceInfo>,
}

/// Detailed diagnostic health and continuity report for an airport's terminal data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirportDoctorReport {
    pub ident: String,
    pub name: Option<String>,
    pub status: String,
    pub has_airport_record: bool,
    pub runway_count: usize,
    pub procedures_found: usize,
    pub missing_elements: Vec<String>,
    pub validation_issues: Vec<openairac_procedures::ProcedureIssue>,
    pub is_flyable: bool,
}

/// Service entry point.
pub struct WorldQuery {
    store: WorldStore,
}

impl WorldQuery {
    pub fn open(path: &str) -> Result<Self> {
        let mut store = WorldStore::open(path)?;
        store.migrate()?;
        Ok(Self { store })
    }

    pub fn open_in_memory() -> Result<Self> {
        let mut store = WorldStore::open_in_memory()?;
        store.migrate()?;
        Ok(Self { store })
    }

    /// Wrap an already-open store.
    pub fn from_store(store: WorldStore) -> Self {
        Self { store }
    }

    /// Read access to the underlying store (ingestion happens elsewhere).
    pub fn store(&self) -> &WorldStore {
        &self.store
    }

    /// Counts of every entity class valid at `date`.
    pub fn world_at(&self, date: DateTime<Utc>) -> Result<WorldAt> {
        let status = self.store.status()?;
        let waypoints = self.store.query_waypoints_at(date)?.len();
        let airway_legs = self.store.query_airway_legs_at(date)?.len();
        let procedure_legs = self.store.query_procedure_legs_at(date)?.len();
        Ok(WorldAt::new(
            date,
            &status,
            waypoints,
            airway_legs,
            procedure_legs,
        ))
    }

    /// One airport by exact ICAO/IATA ident.
    pub fn airport(&self, ident: &str, date: DateTime<Utc>) -> Result<Option<CanonicalAirport>> {
        Ok(self
            .store
            .query_airports_at(date)?
            .into_iter()
            .find(|a| a.ident == ident))
    }

    /// Airports whose ident starts with `query` or whose name contains it
    /// (case-insensitive), ordered by ident.
    pub fn search_airports(
        &self,
        query: &str,
        date: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<CanonicalAirport>> {
        let q = query.to_uppercase();
        let mut found: Vec<CanonicalAirport> = self
            .store
            .query_airports_at(date)?
            .into_iter()
            .filter(|a| a.ident.starts_with(&q) || a.name.to_uppercase().contains(&q))
            .collect();
        found.sort_by(|a, b| a.ident.cmp(&b.ident));
        found.truncate(limit);
        Ok(found)
    }

    /// Airports and navaids within `radius_nm` of a coordinate, ordered by
    /// distance.
    pub fn nearby(
        &self,
        coordinate: Coordinate,
        radius_nm: f64,
        date: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<NearbyEntity>> {
        let mut entities = Vec::new();
        for airport in self.store.query_airports_at(date)? {
            let d = coordinate.distance_nm(&Coordinate {
                latitude_deg: airport.latitude,
                longitude_deg: airport.longitude,
            });
            if d <= radius_nm {
                entities.push(NearbyEntity {
                    ident: airport.ident,
                    name: airport.name,
                    kind: "airport".to_string(),
                    latitude: airport.latitude,
                    longitude: airport.longitude,
                    distance_nm: d,
                });
            }
        }
        for navaid in self.store.query_navaids_at(date)? {
            let d = coordinate.distance_nm(&Coordinate {
                latitude_deg: navaid.latitude,
                longitude_deg: navaid.longitude,
            });
            if d <= radius_nm {
                entities.push(NearbyEntity {
                    ident: navaid.ident,
                    name: navaid.name,
                    kind: match navaid.kind {
                        NavaidKind::Ndb => "NDB",
                        NavaidKind::Dme => "DME",
                        _ => "VOR",
                    }
                    .to_string(),
                    latitude: navaid.latitude,
                    longitude: navaid.longitude,
                    distance_nm: d,
                });
            }
        }
        entities.sort_by(|a, b| a.distance_nm.total_cmp(&b.distance_nm));
        entities.truncate(limit);
        Ok(entities)
    }

    /// All published airway route idents with leg counts and endpoints.
    pub fn airways(&self, date: DateTime<Utc>) -> Result<Vec<AirwaySummary>> {
        let legs = self.store.query_airway_legs_at(date)?;
        let mut summaries: Vec<AirwaySummary> = Vec::new();
        for leg in &legs {
            match summaries
                .iter_mut()
                .find(|s| s.route_ident == leg.route_ident)
            {
                Some(s) => {
                    s.legs += 1;
                    s.last_fix = leg.end_fix.clone();
                }
                None => summaries.push(AirwaySummary {
                    route_ident: leg.route_ident.clone(),
                    route_type: leg.route_type.clone(),
                    level: leg.level,
                    legs: 1,
                    first_fix: leg.start_fix.clone(),
                    last_fix: leg.end_fix.clone(),
                }),
            }
        }
        summaries.sort_by(|a, b| a.route_ident.cmp(&b.route_ident));
        Ok(summaries)
    }

    /// Published procedures for one airport (optionally filtered by kind),
    /// with transition idents.
    pub fn procedures(
        &self,
        airport_ident: &str,
        kind: Option<ProcedureKind>,
        date: DateTime<Utc>,
    ) -> Result<Vec<ProcedureSummary>> {
        let legs = self.store.query_procedure_legs_at(date)?;
        let mut summaries: Vec<ProcedureSummary> = Vec::new();
        for leg in &legs {
            if leg.airport_ident != airport_ident {
                continue;
            }
            if let Some(k) = kind
                && ProcedureKind::from_arinc(leg.procedure_kind) != Some(k)
            {
                continue;
            }
            match summaries.iter_mut().find(|s| {
                s.ident == leg.procedure_ident && s.kind == leg.procedure_kind.to_string()
            }) {
                Some(s) => {
                    s.legs += 1;
                    let t = leg.transition_ident.trim();
                    if !t.is_empty() && !s.transitions.iter().any(|x| x == t) {
                        s.transitions.push(t.to_string());
                    }
                }
                None => {
                    let mut transitions = Vec::new();
                    let t = leg.transition_ident.trim();
                    if !t.is_empty() {
                        transitions.push(t.to_string());
                    }
                    summaries.push(ProcedureSummary {
                        airport_ident: leg.airport_ident.clone(),
                        kind: leg.procedure_kind.to_string(),
                        ident: leg.procedure_ident.clone(),
                        legs: 1,
                        transitions,
                    });
                }
            }
        }
        summaries.sort_by(|a, b| a.ident.cmp(&b.ident));
        Ok(summaries)
    }

    /// Full flight planning through the integration layer.
    pub fn plan(&self, request: &FlightPlanRequest) -> Result<FlightPlan> {
        Planner::new(&self.store).plan(request)
    }

    /// Connected-component sizes of the airway graph at `date`, sorted
    /// descending. A healthy dataset has one dominant component; many
    /// large components indicate disconnected airspace (data quality).
    pub fn graph_components(&self, date: DateTime<Utc>) -> Result<Vec<usize>> {
        let (graph, _) = openairac_routing::AirwayGraph::build(
            &self.store.query_waypoints_at(date)?,
            &self.store.query_navaids_at(date)?,
            &self.store.query_airway_legs_at(date)?,
            date,
        );
        Ok(graph.disconnected_components())
    }

    /// Run multi-source reconciliation for the world valid at `as_of`.
    /// Deterministic and idempotent: re-running writes nothing new.
    pub fn reconcile(&self, as_of: DateTime<Utc>) -> Result<ReconciliationStats> {
        openairac_reconcile::Reconciler::new(&self.store).reconcile(as_of)
    }

    /// Reconciliation bookkeeping counts.
    pub fn reconciliation_status(&self) -> Result<ReconciliationStatus> {
        Ok(ReconciliationStatus {
            canonical_entities: self.store.query_canonical_identities()?.len(),
            memberships: self.store.query_memberships()?.len(),
            conflicts: self.store.query_reconciliation_conflicts()?.len(),
        })
    }

    /// Resolved canonical entity at `as_of` (fields + provenance +
    /// conflicts). Raw provider rows remain queryable unchanged.
    pub fn canonical_entity(
        &self,
        canonical_id: &CanonicalEntityId,
        as_of: DateTime<Utc>,
    ) -> Result<Option<ResolvedEntity>> {
        openairac_reconcile::resolved_entity(&self.store, canonical_id, as_of)
    }

    /// Canonical identities a source entity belongs to.
    pub fn aliases(&self, source: &SourceEntityRef) -> Result<Vec<CanonicalEntityId>> {
        let mut ids: Vec<CanonicalEntityId> = self
            .store
            .query_memberships()?
            .into_iter()
            .filter(|m| m.source == *source)
            .map(|m| m.canonical_id)
            .collect();
        ids.sort();
        ids.dedup();
        Ok(ids)
    }

    /// All recorded reconciliation conflicts.
    pub fn conflicts(&self) -> Result<Vec<ReconciliationConflict>> {
        self.store.query_reconciliation_conflicts()
    }

    /// Coverage report for the world valid at `as_of`.
    pub fn coverage_report(&self, as_of: DateTime<Utc>) -> Result<CoverageReport> {
        let mut providers = Vec::new();
        for manifest in openairac_model::PROVIDER_MANIFESTS {
            let ns = format!("{}:", manifest.namespace);
            let airports = self
                .store
                .query_airports_at(as_of)?
                .into_iter()
                .filter(|a| a.id.0.starts_with(&ns))
                .count();
            let runways = self
                .store
                .raw_conn()
                .query_row(
                    "SELECT COUNT(*) FROM runways WHERE id LIKE ?1
                     AND valid_from <= ?2 AND (valid_until IS NULL OR valid_until > ?2)",
                    [format!("{ns}%"), as_of.to_rfc3339()],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap_or(0) as usize;
            let navaids = self
                .store
                .query_navaids_at(as_of)?
                .into_iter()
                .filter(|n| n.object_id.0.starts_with(&ns))
                .count();
            let waypoints = self
                .store
                .query_waypoints_at(as_of)?
                .into_iter()
                .filter(|w| w.object_id.0.starts_with(&ns))
                .count();
            let airway_legs = self
                .store
                .query_airway_legs_at(as_of)?
                .into_iter()
                .filter(|l| l.object_id.0.starts_with(&ns))
                .count();
            let procedure_legs = self
                .store
                .query_procedure_legs_at(as_of)?
                .into_iter()
                .filter(|l| l.object_id.0.starts_with(&ns))
                .count();
            let ils_associations = self
                .store
                .raw_conn()
                .query_row(
                    "SELECT COUNT(*) FROM ils_associations
                     WHERE icao_code = 'K2' OR icao_code = 'K1' OR icao_code = 'K3'",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap_or(0) as usize;
            let conflicts = self
                .store
                .query_reconciliation_conflicts()?
                .into_iter()
                .filter(|c| c.ref_a.starts_with(&ns) || c.ref_b.starts_with(&ns))
                .count();
            let (snapshots, freshest) = {
                let snaps = self.store.query_source_snapshots()?;
                let ours: Vec<_> = snaps
                    .iter()
                    .filter(|s| s.provider == manifest.name)
                    .collect();
                (
                    ours.len(),
                    ours.iter()
                        .map(|s| s.retrieved_at)
                        .max()
                        .map(|t| t.to_rfc3339()),
                )
            };
            providers.push(ProviderCoverage {
                provider: manifest.name.to_string(),
                coverage: manifest.capabilities.coverage.as_str().to_string(),
                temporal: manifest.capabilities.temporal.as_str().to_string(),
                update: manifest.capabilities.update.as_str().to_string(),
                authority_note: manifest.capabilities.authority_note.to_string(),
                airports,
                runways,
                navaids,
                waypoints,
                airway_legs,
                procedure_legs,
                ils_associations: if manifest.name == "FAA_CIFP" {
                    ils_associations
                } else {
                    0
                },
                snapshots,
                freshest_retrieved_at: freshest,
                conflicts,
            });
        }
        let mut countries: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        for airport in self.store.query_airports_at(as_of)? {
            if let Some(country) = airport.iso_country
                && !country.is_empty()
            {
                *countries.entry(country).or_default() += 1;
            }
        }
        Ok(CoverageReport {
            as_of,
            providers,
            airports_by_country: countries.into_iter().collect(),
        })
    }

    /// The current waypoint list (for diagnostics and exporters).
    pub fn waypoints(&self, date: DateTime<Utc>) -> Result<Vec<CanonicalWaypoint>> {
        self.store.query_waypoints_at(date)
    }

    /// Detailed coverage information for a specific airport (ident).
    pub fn airport_coverage(
        &self,
        ident: &str,
        as_of: DateTime<Utc>,
    ) -> Result<Option<AirportCoverageReport>> {
        let airport = match self.airport(ident, as_of)? {
            Some(a) => a,
            None => return Ok(None),
        };

        let runways = airport
            .runways
            .iter()
            .map(|r| RunwayDetail {
                designator: r.official_designator.clone(),
                length_ft: r.length_ft,
                width_ft: r.width_ft,
                surface: r.surface.clone(),
            })
            .collect();

        // Count nearby navaids associated with this airport
        let navaids_count = self
            .store
            .query_navaids_at(as_of)?
            .into_iter()
            .filter(|n| n.associated_airport.as_deref() == Some(ident))
            .count();

        // Query procedures
        let sids_list = self.procedures(ident, Some(ProcedureKind::Sid), as_of)?;
        let stars_list = self.procedures(ident, Some(ProcedureKind::Star), as_of)?;
        let app_list = self.procedures(ident, Some(ProcedureKind::Approach), as_of)?;

        let sids: Vec<String> = sids_list.iter().map(|p| p.ident.clone()).collect();
        let stars: Vec<String> = stars_list.iter().map(|p| p.ident.clone()).collect();
        let approaches: Vec<String> = app_list.iter().map(|p| p.ident.clone()).collect();

        // Source provenance
        let snapshots = self.store.query_source_snapshots()?;
        let mut sources = Vec::new();
        let mut seen_providers = std::collections::BTreeSet::new();
        for snap in snapshots {
            if seen_providers.insert(snap.provider.clone()) {
                let policy = openairac_model::get_provider_policy(&snap.provider);
                let license_id = snap
                    .license_id
                    .or_else(|| policy.map(|p| p.license_id.clone()))
                    .unwrap_or_else(|| "Unknown".to_string());
                let redistribution = policy
                    .map(|p| p.redistribution.as_str().to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                sources.push(SourceProvenanceInfo {
                    provider: snap.provider,
                    dataset: snap.dataset,
                    airac_cycle: snap.airac_cycle,
                    effective_from: snap.effective_from.map(|t| t.to_rfc3339()),
                    license_id,
                    redistribution,
                });
            }
        }

        Ok(Some(AirportCoverageReport {
            ident: airport.ident,
            name: airport.name,
            airport_type: airport.airport_type,
            latitude: airport.latitude,
            longitude: airport.longitude,
            elevation_ft: airport.elevation_ft,
            country: airport.iso_country,
            municipality: airport.municipality,
            runways,
            navaids_count,
            sids_count: sids.len(),
            stars_count: stars.len(),
            approaches_count: approaches.len(),
            sids,
            stars,
            approaches,
            sources,
        }))
    }

    /// Health check & diagnostic analysis for a specific airport.
    pub fn doctor_airport(&self, ident: &str, as_of: DateTime<Utc>) -> Result<AirportDoctorReport> {
        let airport = self.airport(ident, as_of)?;
        let mut missing_elements = Vec::new();
        let mut validation_issues = Vec::new();

        let (has_airport_record, airport_name, runway_count) = match &airport {
            Some(a) => (true, Some(a.name.clone()), a.runways.len()),
            None => {
                missing_elements.push(format!(
                    "Airport '{}' record missing from canonical database",
                    ident
                ));
                (false, None, 0)
            }
        };

        if has_airport_record && runway_count == 0 {
            missing_elements.push("No runways published for this airport".to_string());
        }

        // Fetch all procedure legs for this airport
        let all_legs: Vec<_> = self
            .store
            .query_procedure_legs_at(as_of)?
            .into_iter()
            .filter(|l| l.airport_ident == ident)
            .collect();

        let waypoints = self.store.query_waypoints_at(as_of)?;
        let fix_map: std::collections::BTreeMap<String, (f64, f64)> = waypoints
            .into_iter()
            .map(|w| (w.ident, (w.latitude, w.longitude)))
            .collect();

        let fix_lookup = |name: &str| fix_map.get(name).copied();

        // Group legs into distinct procedures
        let mut proc_groups: std::collections::BTreeMap<
            (char, String),
            Vec<openairac_model::CanonicalProcedureLeg>,
        > = std::collections::BTreeMap::new();
        for leg in all_legs {
            proc_groups
                .entry((leg.procedure_kind, leg.procedure_ident.clone()))
                .or_default()
                .push(leg);
        }

        let procedures_found = proc_groups.len();
        if has_airport_record && procedures_found == 0 {
            missing_elements.push(
                "No instrument terminal procedures (SID/STAR/Approach) found for this airport"
                    .to_string(),
            );
        }

        let validator = openairac_procedures::ProcedureValidator::new(fix_lookup);
        for ((kind_char, proc_ident), legs) in proc_groups {
            let kind = openairac_procedures::ProcedureKind::from_arinc(kind_char)
                .unwrap_or(openairac_procedures::ProcedureKind::Approach);
            if let Ok(proc) = openairac_procedures::Procedure::assemble(
                ident,
                kind,
                &proc_ident,
                legs,
                fix_lookup,
            ) {
                let rep = validator.validate_procedure(&proc);
                validation_issues.extend(rep.issues);
            }
        }

        let is_flyable = has_airport_record
            && runway_count > 0
            && !validation_issues
                .iter()
                .any(|i| i.severity == openairac_procedures::IssueSeverity::Error);

        let status = if !has_airport_record {
            "AIRPORT_MISSING".to_string()
        } else if !is_flyable {
            "DEFECTS_DETECTED".to_string()
        } else if !validation_issues.is_empty() || !missing_elements.is_empty() {
            "WARNINGS_DETECTED".to_string()
        } else {
            "HEALTHY".to_string()
        };

        Ok(AirportDoctorReport {
            ident: ident.to_string(),
            name: airport_name,
            status,
            has_airport_record,
            runway_count,
            procedures_found,
            missing_elements,
            validation_issues,
            is_flyable,
        })
    }

    /// The current navaid list.
    pub fn navaids(&self, date: DateTime<Utc>) -> Result<Vec<CanonicalNavaid>> {
        self.store.query_navaids_at(date)
    }
    /// List all registered aeronautical data providers in the central registry.
    pub fn list_providers(&self) -> Vec<openairac_model::ProviderDescriptor> {
        openairac_model::ProviderRegistryV2::default_registry()
            .list()
            .into_iter()
            .cloned()
            .collect()
    }

    /// Lookup a single provider descriptor by name or identifier.
    pub fn get_provider(&self, id: &str) -> Option<openairac_model::ProviderDescriptor> {
        let pid = openairac_model::ProviderId::new(id);
        openairac_model::ProviderRegistryV2::default_registry()
            .get(&pid)
            .cloned()
    }
}

pub const OPENAIRAC_CORE_VERSION: &str = "2.0.0";
pub const OPENAIRAC_PROTOCOL_VERSION: u32 = 2;

/// Handshake result for client application compatibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompatibilityReport {
    pub is_compatible: bool,
    pub core_version: String,
    pub protocol_version: u32,
    pub client_name: String,
    pub client_version: String,
    pub message: String,
}

pub fn check_client_compatibility(
    client_name: &str,
    client_version: &str,
    client_protocol: u32,
) -> CompatibilityReport {
    let is_compatible = client_protocol == OPENAIRAC_PROTOCOL_VERSION;
    let message = if is_compatible {
        format!(
            "{client_name} v{client_version} is fully compatible with OpenAIRAC Core v{OPENAIRAC_CORE_VERSION} (Protocol v{OPENAIRAC_PROTOCOL_VERSION})"
        )
    } else {
        format!(
            "Incompatible protocol: {client_name} requested Protocol v{client_protocol}, but OpenAIRAC Core v{OPENAIRAC_CORE_VERSION} requires Protocol v{OPENAIRAC_PROTOCOL_VERSION}"
        )
    };

    CompatibilityReport {
        is_compatible,
        core_version: OPENAIRAC_CORE_VERSION.to_string(),
        protocol_version: OPENAIRAC_PROTOCOL_VERSION,
        client_name: client_name.to_string(),
        client_version: client_version.to_string(),
        message,
    }
}

/// A published downloadable data bundle entry for initial setup / updates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BootstrapBundleEntry {
    pub id: String,
    pub title: String,
    pub airac_cycle: String,
    pub approximate_size_bytes: u64,
    pub sha256_hash: String,
    pub download_url: String,
    pub is_recommended: bool,
    pub description: String,
}

/// Index of available official OpenAIRAC data packages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BootstrapIndex {
    pub latest_airac: String,
    pub updated_at: DateTime<Utc>,
    pub bundles: Vec<BootstrapBundleEntry>,
}

impl Default for BootstrapIndex {
    fn default() -> Self {
        Self::default_index()
    }
}

impl BootstrapIndex {
    pub fn default_index() -> Self {
        Self {
            latest_airac: "2608".to_string(),
            updated_at: Utc::now(),
            bundles: vec![
                BootstrapBundleEntry {
                    id: "world-open".to_string(),
                    title: "World Open Navdata Bundle".to_string(),
                    airac_cycle: "2608".to_string(),
                    approximate_size_bytes: 42_000_000,
                    sha256_hash: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string(),
                    download_url: "https://github.com/bobberdolle1/open-airac/releases/download/v2.0.0/openairac-bundle-world-open-2608.tar.gz".to_string(),
                    is_recommended: true,
                    description: "Worldwide airports, runways, enroute fixes, navaids, airways, and official FAA/France SIA procedures.".to_string(),
                },
                BootstrapBundleEntry {
                    id: "us".to_string(),
                    title: "United States (FAA CIFP & AIXM)".to_string(),
                    airac_cycle: "2608".to_string(),
                    approximate_size_bytes: 28_000_000,
                    sha256_hash: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string(),
                    download_url: "https://github.com/bobberdolle1/open-airac/releases/download/v2.0.0/openairac-bundle-us-2608.tar.gz".to_string(),
                    is_recommended: false,
                    description: "Complete US nationwide coverage with full terminal SIDs, STARs, and IAPs.".to_string(),
                },
                BootstrapBundleEntry {
                    id: "europe-open".to_string(),
                    title: "Europe Open (DFS, SIA, OFM)".to_string(),
                    airac_cycle: "2608".to_string(),
                    approximate_size_bytes: 18_000_000,
                    sha256_hash: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string(),
                    download_url: "https://github.com/bobberdolle1/open-airac/releases/download/v2.0.0/openairac-bundle-europe-open-2608.tar.gz".to_string(),
                    is_recommended: false,
                    description: "European airports, runways, navaids, routes, and open geodata under open government licences.".to_string(),
                },
            ],
        }
    }
}

/// Sanitized path string for reports (replaces user home path components).
pub fn sanitize_path_for_report(path: &std::path::Path) -> String {
    let p_str = path.to_string_lossy().to_string();
    if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME"))
        && !home.is_empty()
        && p_str.starts_with(&home)
    {
        return format!("~{}", &p_str[home.len()..]);
    }
    p_str
}
#[cfg(test)]
mod tests {
    use super::*;
    use openairac_model::{
        AirportId, AirwayLegId, ProcedureLegId, SourceSnapshot, SourceSnapshotId, TemporalValidity,
        WaypointId,
    };

    fn temporal(t: DateTime<Utc>) -> TemporalValidity {
        TemporalValidity {
            valid_from: t,
            valid_until: None,
            source_snapshot_id: SourceSnapshotId("snap-test".to_string()),
        }
    }

    fn seeded(t: DateTime<Utc>) -> Result<WorldQuery> {
        let mut service = WorldQuery::open_in_memory()?;
        service.store.insert_source_snapshot(&SourceSnapshot {
            id: SourceSnapshotId("snap-test".to_string()),
            provider: "test".to_string(),
            dataset: "fixture".to_string(),
            provider_revision: None,
            airac_cycle: None,
            effective_from: None,
            effective_until: None,
            retrieved_at: t,
            source_uri: "memory:".to_string(),
            content_sha256: "0".repeat(64),
            license_id: None,
            license_notes: None,
            parser_version: "test".to_string(),
        })?;
        let airport = CanonicalAirport {
            id: AirportId("ourairports:1".to_string()),
            ident: "KSFO".to_string(),
            name: "San Francisco International".to_string(),
            airport_type: "large_airport".to_string(),
            latitude: 37.6188,
            longitude: -122.3750,
            elevation_ft: Some(13.0),
            iso_country: Some("US".to_string()),
            municipality: Some("San Francisco".to_string()),
            runways: Vec::new(),
            temporal: temporal(t),
        };
        service.store.insert_airport(&airport)?;
        let wp = CanonicalWaypoint {
            object_id: WaypointId("WP-W1".to_string()),
            ident: "W1".to_string(),
            name: "W1".to_string(),
            latitude: 37.7,
            longitude: -122.3,
            is_enroute: true,
            region_code: "K2".to_string(),
            terminal_area_ident: None,
            waypoint_type: Some(0x202057),
            temporal: temporal(t),
        };
        service.store.insert_waypoint(&wp)?;
        let navaid = CanonicalNavaid {
            object_id: openairac_model::NavaidId("ourairports:NAV-SFO".to_string()),
            ident: "SFO".to_string(),
            name: "SAN FRANCISCO VORTAC".to_string(),
            kind: NavaidKind::Vortac,
            frequency: openairac_model::FrequencyKhz(115800),
            latitude: 37.6195,
            longitude: -122.3738,
            elevation_ft: Some(12),
            region_code: Some("K2".to_string()),
            associated_airport: Some("KSFO".to_string()),
            magnetic_variation_deg: None,
            slaved_variation_deg: None,
            service_volume_nm: Some(130),
            dme_paired: false,
            associated_runway: None,
            localizer_bearing_true_deg: None,
            localizer_bearing_mag_deg: None,
            glideslope_angle_deg: None,
            temporal: temporal(t),
        };
        service.store.insert_navaid(&navaid)?;
        let leg = openairac_model::CanonicalAirwayLeg {
            object_id: AirwayLegId("LEG-V1".to_string()),
            route_ident: "V1".to_string(),
            route_type: "O".to_string(),
            level: Some('L'),
            sequence_number: 1,
            start_fix: "W1".to_string(),
            start_icao_code: "K2".to_string(),
            end_fix: "SFO".to_string(),
            end_icao_code: "K2".to_string(),
            direction: 'N',
            minimum_altitude_ft: None,
            maximum_altitude_ft: None,
            temporal: temporal(t),
        };
        service
            .store
            .transact(|conn| openairac_store::insert_airway_leg_conn(conn, &leg))?;
        let ple = openairac_model::CanonicalProcedureLeg {
            object_id: ProcedureLegId("PL-1".to_string()),
            airport_ident: "KSFO".to_string(),
            icao_code: "K2".to_string(),
            procedure_kind: 'D',
            procedure_ident: "CIITY3".to_string(),
            route_type: String::new(),
            transition_ident: String::new(),
            sequence_number: 10,
            fix_ident: "W1".to_string(),
            fix_icao_code: "K2".to_string(),
            fix_section: " ".to_string(),
            waypoint_description: "E ".to_string(),
            turn_direction: None,
            rnp_nm: None,
            path_terminator: "TF".to_string(),
            recommended_navaid: None,
            arc_radius_nm: None,
            course_a_deg: None,
            distance_a_nm: None,
            course_b_deg: None,
            distance_b_nm: None,
            altitude_descriptor: None,
            altitude_1_ft: None,
            altitude_2_ft: None,
            speed_limit_kts: None,
            course_c_deg: None,
            vertical_angle_deg: None,
            msa_center_fix: None,
            route_qualifiers: String::new(),
            raw: String::new(),
            temporal: temporal(t),
        };
        service
            .store
            .transact(|conn| openairac_store::insert_procedure_leg_conn(conn, &ple))?;
        Ok(service)
    }

    #[test]
    fn test_world_at_counts() {
        let t = Utc::now();
        let service = seeded(t).unwrap();
        let world = service.world_at(t).unwrap();
        assert_eq!(world.airports, 1);
        assert_eq!(world.navaids, 1);
        assert_eq!(world.waypoints, 1);
        assert_eq!(world.airway_legs, 1);
        assert_eq!(world.procedure_legs, 1);
        assert_eq!(world.migration_version, 13);
    }

    #[test]
    fn test_airport_and_search() {
        let t = Utc::now();
        let service = seeded(t).unwrap();
        let a = service.airport("KSFO", t).unwrap().expect("KSFO found");
        assert_eq!(a.name, "San Francisco International");
        assert!(service.airport("KJFK", t).unwrap().is_none());

        let found = service.search_airports("KS", t, 10).unwrap();
        assert_eq!(found.len(), 1);
        let by_name = service.search_airports("francisco", t, 10).unwrap();
        assert_eq!(by_name.len(), 1);
        assert!(service.search_airports("ZZZ", t, 10).unwrap().is_empty());
    }

    #[test]
    fn test_nearby_ordering() {
        let t = Utc::now();
        let service = seeded(t).unwrap();
        let center = Coordinate {
            latitude_deg: 37.6188,
            longitude_deg: -122.3750,
        };
        let nearby = service.nearby(center, 10.0, t, 10).unwrap();
        assert_eq!(nearby.len(), 2); // KSFO + SFO VORTAC
        assert_eq!(nearby[0].ident, "KSFO"); // center is the airport
        assert_eq!(nearby[0].kind, "airport");
        assert_eq!(nearby[1].ident, "SFO"); // VORTAC ~0.07 nm away
        assert_eq!(nearby[1].kind, "VOR");
    }

    #[test]
    fn test_airways_and_procedures() {
        let t = Utc::now();
        let service = seeded(t).unwrap();
        let airways = service.airways(t).unwrap();
        assert_eq!(airways.len(), 1);
        assert_eq!(airways[0].route_ident, "V1");
        assert_eq!(airways[0].first_fix, "W1");
        assert_eq!(airways[0].last_fix, "SFO");

        let procedures = service
            .procedures("KSFO", Some(ProcedureKind::Sid), t)
            .unwrap();
        assert_eq!(procedures.len(), 1);
        assert_eq!(procedures[0].ident, "CIITY3");
        assert!(
            service
                .procedures("KSFO", Some(ProcedureKind::Star), t)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn test_graph_components() {
        let t = Utc::now();
        let service = seeded(t).unwrap();
        // One leg W1 -> SFO: a single component of size 2.
        let components = service.graph_components(t).unwrap();
        assert_eq!(components, vec![2]);
    }

    #[test]
    fn test_coverage_report() {
        let t = Utc::now();
        let service = seeded(t).unwrap();
        let report = service.coverage_report(t).unwrap();
        assert_eq!(
            report.providers.len(),
            openairac_model::PROVIDER_MANIFESTS.len()
        );
        let oa = report
            .providers
            .iter()
            .find(|p| p.provider == "OurAirports")
            .unwrap();
        assert_eq!(oa.coverage, "worldwide");
        assert_eq!(oa.temporal, "continuous");
        assert_eq!(oa.airports, 1);
        assert_eq!(oa.navaids, 1);
        let faa = report
            .providers
            .iter()
            .find(|p| p.provider == "FAA_CIFP")
            .unwrap();
        assert_eq!(faa.coverage, "nationwide");
        assert_eq!(faa.temporal, "airac_cycle");
        assert_eq!(faa.airports, 0); // fixture has no FAA entities
        assert_eq!(report.airports_by_country.len(), 1);
        assert_eq!(report.airports_by_country[0].1, 1);
    }

    #[test]
    fn test_plan_requires_both_ends() {
        let t = Utc::now();
        let service = seeded(t).unwrap();
        let request = FlightPlanRequest {
            origin_airport: "KSFO".to_string(),
            destination_airport: "KJFK".to_string(),
            departure_time: t,
            cruise_altitude_ft: None,
            aircraft_capabilities: openairac_routing::AircraftCapabilities { rnav: true },
            sid_ident: None,
            sid_transition: None,
            star_ident: None,
            star_transition: None,
            approach_ident: None,
            exclusions: Vec::new(),
        };
        // KJFK does not exist in the fixture: fail closed with the airport.
        let err = service.plan(&request).unwrap_err();
        assert!(err.to_string().contains("KJFK"));
    }

    #[test]
    fn test_airport_coverage_and_doctor() {
        let t = Utc::now();
        let service = seeded(t).unwrap();

        // KSFO is in seeded store
        let cov = service
            .airport_coverage("KSFO", t)
            .unwrap()
            .expect("KSFO coverage");
        assert_eq!(cov.ident, "KSFO");
        assert_eq!(cov.name, "San Francisco International");
        assert_eq!(cov.runways.len(), 0);

        let doc = service.doctor_airport("KSFO", t).unwrap();
        assert_eq!(doc.ident, "KSFO");
        assert_eq!(doc.runway_count, 0);
        assert_eq!(doc.status, "DEFECTS_DETECTED"); // seeded KSFO has 0 runways -> not flyable

        // Unknown airport
        let doc_unknown = service.doctor_airport("Z999", t).unwrap();
        assert_eq!(doc_unknown.status, "AIRPORT_MISSING");
        assert!(!doc_unknown.has_airport_record);
    }

    #[test]
    fn test_compatibility_handshake() {
        let ok = check_client_compatibility("OpenAIRAC Map", "1.0.0", 2);
        assert!(ok.is_compatible);
        assert_eq!(ok.core_version, "2.0.0");
        assert_eq!(ok.protocol_version, 2);
        assert!(ok.message.contains("fully compatible"));

        let fail = check_client_compatibility("Legacy Client", "0.2.0", 1);
        assert!(!fail.is_compatible);
        assert!(fail.message.contains("Incompatible protocol"));
    }

    #[test]
    fn test_bootstrap_index() {
        let idx = BootstrapIndex::default_index();
        assert_eq!(idx.latest_airac, "2608");
        assert_eq!(idx.bundles.len(), 3);
        assert!(
            idx.bundles
                .iter()
                .any(|b| b.id == "world-open" && b.is_recommended)
        );
        assert!(
            idx.bundles
                .iter()
                .all(|b| b.download_url.starts_with("https://"))
        );
    }
}
