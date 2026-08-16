//! Read-side resolved view: canonical entities with field provenance.
//!
//! Every resolved field remains traceable to its source; differing
//! values across members are exposed as conflicts, never silently
//! deleted. Resolved values are NEVER copied back over raw rows.

use anyhow::Result;
use chrono::{DateTime, Utc};
use openairac_model::*;
use openairac_store::WorldStore;

use crate::authority::provider_rank;

/// Fields exposed per entity table in the resolved view.
pub const FIELD_SELECTORS: &[(&str, &str)] = &[
    ("airports", "ident"),
    ("airports", "name"),
    ("airports", "latitude"),
    ("airports", "longitude"),
    ("airports", "elevation_ft"),
    ("navaids", "ident"),
    ("navaids", "name"),
    ("navaids", "kind"),
    ("navaids", "frequency_khz"),
    ("waypoints", "ident"),
    ("waypoints", "name"),
    ("waypoints", "latitude"),
    ("waypoints", "longitude"),
];

/// Extract (field, value) pairs from one source row.
fn field_values(
    table: &str,
    entity_id: &str,
    store: &WorldStore,
    as_of: DateTime<Utc>,
) -> Vec<(String, String)> {
    let mut values = Vec::new();
    match table {
        "airports" => {
            if let Ok(rows) = store.query_airports_at(as_of) {
                for a in rows.iter().filter(|a| a.id.0 == entity_id) {
                    values.push(("ident".into(), a.ident.clone()));
                    values.push(("name".into(), a.name.clone()));
                    values.push(("latitude".into(), a.latitude.to_string()));
                    values.push(("longitude".into(), a.longitude.to_string()));
                    values.push((
                        "elevation_ft".into(),
                        a.elevation_ft.map(|e| e.to_string()).unwrap_or_default(),
                    ));
                }
            }
        }
        "navaids" => {
            if let Ok(rows) = store.query_navaids_at(as_of) {
                for n in rows.iter().filter(|n| n.object_id.0 == entity_id) {
                    values.push(("ident".into(), n.ident.clone()));
                    values.push(("name".into(), n.name.clone()));
                    values.push(("kind".into(), format!("{:?}", n.kind)));
                    values.push(("frequency_khz".into(), n.frequency.0.to_string()));
                }
            }
        }
        "waypoints" => {
            if let Ok(rows) = store.query_waypoints_at(as_of) {
                for w in rows.iter().filter(|w| w.object_id.0 == entity_id) {
                    values.push(("ident".into(), w.ident.clone()));
                    values.push(("name".into(), w.name.clone()));
                    values.push(("latitude".into(), w.latitude.to_string()));
                    values.push(("longitude".into(), w.longitude.to_string()));
                }
            }
        }
        _ => {}
    }
    values
}

/// Build the resolved view of one canonical entity at `as_of`.
pub fn resolved_entity(
    store: &WorldStore,
    canonical_id: &CanonicalEntityId,
    as_of: DateTime<Utc>,
) -> Result<Option<ResolvedEntity>> {
    let identities = store.query_canonical_identities()?;
    let Some((_, entity_table, _, _)) =
        identities.iter().find(|(cid, _, _, _)| cid == canonical_id)
    else {
        return Ok(None);
    };
    let entity_table = entity_table.clone();

    let members: Vec<SourceMembership> = store
        .query_memberships()?
        .into_iter()
        .filter(|m| {
            m.canonical_id == *canonical_id
                && m.valid_from <= as_of
                && m.valid_until.map(|u| u > as_of).unwrap_or(true)
                && m.status == MembershipStatus::Active
        })
        // A member whose source row no longer exists at `as_of`
        // (tombstone/removal) is not part of the current resolved view;
        // its membership history remains untouched.
        .filter(|m| !field_values(&entity_table, &m.source.entity_id, store, as_of).is_empty())
        .collect();
    let member_refs: Vec<SourceEntityRef> = members.iter().map(|m| m.source.clone()).collect();

    let mut fields: Vec<ResolvedField> = Vec::new();
    for (_table, field) in FIELD_SELECTORS.iter().filter(|(t, _)| *t == entity_table) {
        // Gather each member's value for this field.
        let mut by_source: Vec<(SourceEntityRef, String)> = Vec::new();
        for member in &members {
            for (f, v) in field_values(&entity_table, &member.source.entity_id, store, as_of) {
                if f == *field {
                    by_source.push((member.source.clone(), v));
                }
            }
        }
        if by_source.is_empty() {
            continue;
        }
        // Select per authority policy; disagreements are conflicts.
        let selected = by_source
            .iter()
            .min_by_key(|(source, _)| provider_rank(&entity_table, field, &source.provider))
            .expect("non-empty");
        let distinct: std::collections::BTreeSet<&String> =
            by_source.iter().map(|(_, v)| v).collect();
        let conflicts: Vec<String> = if distinct.len() > 1 {
            by_source
                .iter()
                .filter(|(_, v)| *v != selected.1)
                .map(|(source, v)| format!("{} says '{v}'", source.display()))
                .collect()
        } else {
            Vec::new()
        };
        fields.push(ResolvedField {
            field: field.to_string(),
            value: selected.1.clone(),
            source: selected.0.clone(),
            conflicts,
        });
    }

    Ok(Some(ResolvedEntity {
        canonical_id: canonical_id.clone(),
        entity_table,
        members: member_refs,
        fields,
    }))
}
