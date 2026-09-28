//! Intent-based state mutation: the single source of truth for *how* an edit
//! changes [`AppState`].
//!
//! The UI dispatches a [`MutationIntent`] (a verb: "remove these profiles", "move
//! these profiles into this group", "set the active profile") rather than
//! computing and shipping a whole new state. [`apply_mutation`] applies that verb
//! to the state in place — pure, no I/O, so the Android daemon applies it exactly
//! as the tests exercise it. Cross-cutting *invariants* (a dangling `active_id` is
//! nulled, a deleted group's profiles are pruned) are enforced after the verb by
//! the backend's write-side middleware chain, not here, so each intent only
//! expresses its own intended change.
//!
//! Id generation and i18n stay on the caller: intents that create entities carry
//! the new id (and any localized text) as fields, so this module needs no `uid()`
//! and no locale.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::profile::Profile;
use crate::state::{AdvancedSettings, AppState, AssetFile, BASE_GROUP_ID, RoutingRule};

/// Merge vs replace, shared by list-import and backup-restore intents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum ImportMode {
    Merge,
    Replace,
}

/// A single state-changing verb dispatched by the UI. The tag `kind` selects the
/// variant; fields are its inputs. Applied by [`apply_mutation`].
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MutationIntent {
    // ---- profiles ----
    /// Add a profile (front) or replace the one with the same `meta.id`.
    UpsertProfile {
        profile: Box<Profile>,
    },
    /// Remove every profile whose id is listed.
    RemoveProfiles {
        ids: Vec<String>,
    },
    /// Copy the profile `id` into a fresh one (`new_id`, `remarks`), inserted right
    /// after the source.
    #[serde(rename_all = "camelCase")]
    CloneProfile {
        id: String,
        new_id: String,
        remarks: String,
    },
    /// Move the listed profiles into `group_id`.
    #[serde(rename_all = "camelCase")]
    MoveProfiles {
        ids: Vec<String>,
        group_id: String,
    },
    /// Prepend a batch of profiles (share-link / file import).
    AddProfiles {
        profiles: Vec<Profile>,
    },
    /// Drop duplicate endpoints within the scope, always keeping the active one.
    #[serde(rename_all = "camelCase")]
    DeduplicateProfiles {
        #[serde(default)]
        active_id: Option<String>,
        #[serde(default)]
        group_id: Option<String>,
    },

    // ---- groups ----
    AddGroup {
        id: String,
        name: String,
    },
    RenameGroup {
        id: String,
        name: String,
    },
    RemoveGroup {
        id: String,
    },
    /// Reorder by index; `g-main` stays pinned at 0.
    ReorderGroups {
        from: u32,
        to: u32,
    },

    // ---- routing rules ----
    /// Add or replace a routing rule (by `id`).
    UpsertRoutingRule {
        rule: Box<RoutingRule>,
    },
    RemoveRoutingRule {
        id: String,
    },
    ReorderRoutingRules {
        from: u32,
        to: u32,
    },
    /// Append (merge) or replace the routing-rule list with `rules`.
    ImportRoutingRules {
        rules: Vec<RoutingRule>,
        mode: ImportMode,
    },

    // ---- asset files ----
    /// Add or replace an asset entry (by `id`).
    UpsertAssetFile {
        asset: Box<AssetFile>,
    },
    RemoveAssetFile {
        id: String,
    },

    // ---- settings / active ----
    /// Replace the whole settings block (the UI builds the next one from the prev).
    SetSettings {
        settings: Box<AdvancedSettings>,
    },
    /// Set (or clear) the active profile id.
    SetActive {
        #[serde(default)]
        id: Option<String>,
    },

    // ---- bulk ----
    /// Restore a backup, merging into or replacing the current state. Replace keeps
    /// the current profiles (backups carry none).
    ImportBackup {
        incoming: Box<AppState>,
        mode: ImportMode,
    },
    /// Replace the whole persisted state wholesale (profiles included). The bulk
    /// escape hatch for one-time client migrations on hydrate; not the per-edit path.
    ReplaceState {
        state: Box<AppState>,
    },
}

/// Apply one [`MutationIntent`] to `state` in place. Pure: no I/O. Invariants that
/// span the edit (dangling `active_id`, orphaned-group profiles) are left to the
/// write-side middleware chain, which runs after this.
pub fn apply_mutation(state: &mut AppState, intent: &MutationIntent) {
    match intent {
        MutationIntent::UpsertProfile { profile } => {
            upsert_profile_front(&mut state.profiles, (**profile).clone());
        }
        MutationIntent::RemoveProfiles { ids } => {
            let remove: HashSet<&str> = ids.iter().map(String::as_str).collect();
            state
                .profiles
                .retain(|p| !remove.contains(p.meta().id.as_str()));
        }
        MutationIntent::CloneProfile {
            id,
            new_id,
            remarks,
        } => {
            if let Some(idx) = state.profiles.iter().position(|p| p.meta().id == *id) {
                let mut copy = state.profiles[idx].clone();
                let m = copy.meta_mut();
                m.id = new_id.clone();
                m.remarks = remarks.clone();
                state.profiles.insert(idx + 1, copy);
            }
        }
        MutationIntent::MoveProfiles { ids, group_id } => {
            let selected: HashSet<&str> = ids.iter().map(String::as_str).collect();
            for p in state.profiles.iter_mut() {
                if selected.contains(p.meta().id.as_str()) {
                    p.meta_mut().group_id = group_id.clone();
                }
            }
        }
        MutationIntent::AddProfiles { profiles } => {
            let mut next = profiles.clone();
            next.append(&mut state.profiles);
            state.profiles = next;
        }
        MutationIntent::DeduplicateProfiles {
            active_id,
            group_id,
        } => {
            let (kept, _) = deduplicate_profiles_scoped(
                &state.profiles,
                active_id.as_deref(),
                group_id.as_deref(),
            );
            state.profiles = kept;
        }

        MutationIntent::AddGroup { id, name } => {
            state.groups.push(crate::state::Group {
                id: id.clone(),
                name: name.clone(),
            });
        }
        MutationIntent::RenameGroup { id, name } => {
            if let Some(g) = state.groups.iter_mut().find(|g| g.id == *id) {
                g.name = name.clone();
            }
        }
        MutationIntent::RemoveGroup { id } => {
            // The base group can never be removed.
            if id == BASE_GROUP_ID {
                return;
            }
            state.groups.retain(|g| g.id != *id);
            // The group's profiles go with it; the orphaned-group middleware would
            // also catch any stragglers, but prune here so the intent is complete.
            state.profiles.retain(|p| p.meta().group_id != *id);
        }
        MutationIntent::ReorderGroups { from, to } => {
            // g-main stays pinned at index 0: never move it, never drop above it.
            let pinned = usize::from(
                state
                    .groups
                    .first()
                    .map(|g| g.id == BASE_GROUP_ID)
                    .unwrap_or(false),
            );
            let from = *from as usize;
            if from < pinned {
                return;
            }
            move_item_by_index(&mut state.groups, from, (*to as usize).max(pinned));
        }

        MutationIntent::UpsertRoutingRule { rule } => {
            upsert_by_id(&mut state.routing_rules, (**rule).clone());
        }
        MutationIntent::RemoveRoutingRule { id } => {
            state.routing_rules.retain(|r| r.id != *id);
        }
        MutationIntent::ReorderRoutingRules { from, to } => {
            move_item_by_index(&mut state.routing_rules, *from as usize, *to as usize);
        }
        MutationIntent::ImportRoutingRules { rules, mode } => match mode {
            ImportMode::Replace => state.routing_rules = rules.clone(),
            ImportMode::Merge => state.routing_rules.extend(rules.iter().cloned()),
        },

        MutationIntent::UpsertAssetFile { asset } => {
            upsert_by_id(&mut state.asset_files, (**asset).clone());
        }
        MutationIntent::RemoveAssetFile { id } => {
            state.asset_files.retain(|a| a.id != *id);
        }

        MutationIntent::SetSettings { settings } => {
            state.settings = (**settings).clone();
        }
        MutationIntent::SetActive { id } => {
            state.active_id = id.clone();
        }

        MutationIntent::ImportBackup { incoming, mode } => match mode {
            ImportMode::Replace => {
                // Keep the current profiles (backups carry none); take everything
                // else from the backup. A now-dangling active_id is nulled by the
                // middleware that runs after this.
                let profiles = std::mem::take(&mut state.profiles);
                *state = (**incoming).clone();
                state.profiles = profiles;
            }
            ImportMode::Merge => {
                state.profiles.extend(incoming.profiles.iter().cloned());
                state.groups.extend(incoming.groups.iter().cloned());
                state
                    .routing_rules
                    .extend(incoming.routing_rules.iter().cloned());
                state
                    .asset_files
                    .extend(incoming.asset_files.iter().cloned());
                // Per-field override by the backup, matching the UI's prior merge.
                state.settings = incoming.settings.clone();
            }
        },
        MutationIntent::ReplaceState { state: replacement } => {
            *state = (**replacement).clone();
        }
    }
}

/// Add `profile` at the front, or replace the existing one with the same `meta.id`.
fn upsert_profile_front(profiles: &mut Vec<Profile>, profile: Profile) {
    if let Some(slot) = profiles
        .iter_mut()
        .find(|p| p.meta().id == profile.meta().id)
    {
        *slot = profile;
    } else {
        profiles.insert(0, profile);
    }
}

/// Trait for the `{ id }`-keyed entities (rules, assets) so one upsert serves
/// both: replace in place by id, else append.
trait HasId {
    fn entity_id(&self) -> &str;
}
impl HasId for RoutingRule {
    fn entity_id(&self) -> &str {
        &self.id
    }
}
impl HasId for AssetFile {
    fn entity_id(&self) -> &str {
        &self.id
    }
}

fn upsert_by_id<T: HasId>(items: &mut Vec<T>, item: T) {
    if let Some(slot) = items.iter_mut().find(|x| x.entity_id() == item.entity_id()) {
        *slot = item;
    } else {
        items.push(item);
    }
}

/// Move `items[from]` to index `to`, clamping out-of-range / no-op moves to nothing.
fn move_item_by_index<T>(items: &mut [T], from: usize, to: usize) {
    if from == to || from >= items.len() || to >= items.len() {
        return;
    }
    if from < to {
        items[from..=to].rotate_left(1);
    } else {
        items[to..=from].rotate_right(1);
    }
}

// ---- profile dedup (used by the DeduplicateProfiles intent) ----

/// Duplicate-identity comparison: protocol + endpoint + remarks.
pub fn same_profile_identity(a: &Profile, b: &Profile) -> bool {
    a.protocol() == b.protocol()
        && a.address() == b.address()
        && a.port() == b.port()
        && a.meta().remarks == b.meta().remarks
}

/// The content key a duplicate check compares: the profile minus its identity /
/// bookkeeping fields, so two profiles that differ only in id/group/remarks count
/// as the same endpoint.
fn profile_dedup_key(p: &Profile) -> String {
    let mut v = serde_json::to_value(p).expect("profile serializes");
    if let Some(meta) = v.get_mut("meta").and_then(|m| m.as_object_mut()) {
        for k in ["id", "remarks", "groupId", "via"] {
            meta.remove(k);
        }
    }
    v.to_string()
}

/// Drop duplicate endpoints, keeping the first (or the active one).
pub fn deduplicate_profiles(
    profiles: &[Profile],
    active_id: Option<&str>,
) -> (Vec<Profile>, usize) {
    let mut seen: HashMap<String, String> = HashMap::new(); // key -> kept profile id
    for p in profiles {
        let key = profile_dedup_key(p);
        let is_active = active_id == Some(p.meta().id.as_str());
        if !seen.contains_key(&key) || is_active {
            seen.insert(key, p.meta().id.clone());
        }
    }
    let kept: Vec<Profile> = profiles
        .iter()
        .filter(|p| {
            seen.get(&profile_dedup_key(p)).map(String::as_str) == Some(p.meta().id.as_str())
        })
        .cloned()
        .collect();
    let removed = profiles.len() - kept.len();
    (kept, removed)
}

/// Dedup only within `group_id` (or everything when it's `None`/`"all"`), keeping
/// profiles outside the scope untouched. Returns the surviving profiles and the ids
/// that were dropped.
pub fn deduplicate_profiles_scoped(
    profiles: &[Profile],
    active_id: Option<&str>,
    group_id: Option<&str>,
) -> (Vec<Profile>, HashSet<String>) {
    let affected: Vec<Profile> = match group_id {
        None | Some("all") => profiles.to_vec(),
        Some(g) => profiles
            .iter()
            .filter(|p| p.meta().group_id == g)
            .cloned()
            .collect(),
    };
    let (kept_affected, _) = deduplicate_profiles(&affected, active_id);
    let kept_ids: HashSet<&str> = kept_affected.iter().map(|p| p.meta().id.as_str()).collect();
    let removed_ids: HashSet<String> = affected
        .iter()
        .filter(|p| !kept_ids.contains(p.meta().id.as_str()))
        .map(|p| p.meta().id.clone())
        .collect();
    let kept = profiles
        .iter()
        .filter(|p| !removed_ids.contains(&p.meta().id))
        .cloned()
        .collect();
    (kept, removed_ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::share::parse_share_link;
    use crate::state::{Group, default_app_state};

    fn p(uri: &str) -> Profile {
        parse_share_link(uri, None).unwrap()
    }

    fn with_id(uri: &str, id: &str, group: &str) -> Profile {
        let mut prof = p(uri);
        prof.meta_mut().id = id.into();
        prof.meta_mut().group_id = group.into();
        prof
    }

    fn base() -> AppState {
        let mut s = default_app_state();
        s.groups.push(Group {
            id: "g2".into(),
            name: "Two".into(),
        });
        s
    }

    #[test]
    fn upsert_profile_adds_front_then_replaces() {
        let mut s = base();
        let mut a = with_id("trojan://pw@a.com:443#A", "a", "g-main");
        apply_mutation(
            &mut s,
            &MutationIntent::UpsertProfile {
                profile: Box::new(a.clone()),
            },
        );
        assert_eq!(s.profiles.len(), 1);
        // Same id replaces, doesn't duplicate.
        a.meta_mut().remarks = "A2".into();
        apply_mutation(
            &mut s,
            &MutationIntent::UpsertProfile {
                profile: Box::new(a),
            },
        );
        assert_eq!(s.profiles.len(), 1);
        assert_eq!(s.profiles[0].meta().remarks, "A2");
    }

    #[test]
    fn remove_profiles_drops_by_id() {
        let mut s = base();
        s.profiles = vec![
            with_id("trojan://pw@a.com:443#A", "a", "g-main"),
            with_id("trojan://pw@b.com:443#B", "b", "g-main"),
        ];
        apply_mutation(
            &mut s,
            &MutationIntent::RemoveProfiles {
                ids: vec!["a".into()],
            },
        );
        let ids: Vec<&str> = s.profiles.iter().map(|p| p.meta().id.as_str()).collect();
        assert_eq!(ids, vec!["b"]);
    }

    #[test]
    fn clone_profile_inserts_after() {
        let mut s = base();
        s.profiles = vec![
            with_id("trojan://pw@a.com:443#A", "a", "g-main"),
            with_id("trojan://pw@b.com:443#B", "b", "g-main"),
        ];
        apply_mutation(
            &mut s,
            &MutationIntent::CloneProfile {
                id: "a".into(),
                new_id: "a-copy".into(),
                remarks: "A (copy)".into(),
            },
        );
        let ids: Vec<&str> = s.profiles.iter().map(|p| p.meta().id.as_str()).collect();
        assert_eq!(ids, vec!["a", "a-copy", "b"]);
        let copy = &s.profiles[1];
        assert_eq!(copy.meta().remarks, "A (copy)");
    }

    #[test]
    fn move_profiles_changes_group() {
        let mut s = base();
        s.profiles = vec![
            with_id("trojan://pw@a.com:443#A", "a", "g-main"),
            with_id("trojan://pw@b.com:443#B", "b", "g-main"),
        ];
        apply_mutation(
            &mut s,
            &MutationIntent::MoveProfiles {
                ids: vec!["a".into()],
                group_id: "g2".into(),
            },
        );
        assert_eq!(s.profiles[0].meta().group_id, "g2");
        assert_eq!(s.profiles[1].meta().group_id, "g-main");
    }

    #[test]
    fn add_profiles_prepends() {
        let mut s = base();
        s.profiles = vec![with_id("trojan://pw@b.com:443#B", "b", "g-main")];
        apply_mutation(
            &mut s,
            &MutationIntent::AddProfiles {
                profiles: vec![with_id("trojan://pw@a.com:443#A", "a", "g-main")],
            },
        );
        let ids: Vec<&str> = s.profiles.iter().map(|p| p.meta().id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b"]);
    }

    #[test]
    fn dedup_keeps_active() {
        let mut s = base();
        let a = with_id("trojan://pw@e.com:443#A", "a", "g-main");
        let b = with_id("trojan://pw@e.com:443#B", "b", "g-main"); // dup endpoint of a
        s.profiles = vec![a, b];
        apply_mutation(
            &mut s,
            &MutationIntent::DeduplicateProfiles {
                active_id: Some("b".into()),
                group_id: None,
            },
        );
        assert_eq!(s.profiles.len(), 1);
        assert_eq!(s.profiles[0].meta().id, "b"); // active survived the dedup
    }

    #[test]
    fn dedup_scoped_only_touches_named_group() {
        let a = with_id("trojan://pw@e.com:443#A", "a", "g-main");
        let b = with_id("trojan://pw@e.com:443#B", "b", "g-main"); // dup of a, same group
        let c = with_id("trojan://pw@e.com:443#C", "c", "g2"); // dup but outside scope
        let (kept, removed) = deduplicate_profiles_scoped(&[a, b, c], None, Some("g-main"));
        let ids: Vec<&str> = kept.iter().map(|p| p.meta().id.as_str()).collect();
        assert_eq!(ids, vec!["a", "c"]);
        assert_eq!(removed.len(), 1);
        assert!(removed.contains("b"));
    }

    #[test]
    fn same_profile_identity_ignores_ids() {
        let mut a = with_id("trojan://pw@x.com:443#T", "a", "g-main");
        let b = with_id("trojan://pw@x.com:443#T", "b", "g2");
        assert!(same_profile_identity(&a, &b));
        a.meta_mut().remarks = "Other".into();
        assert!(!same_profile_identity(&a, &b));
    }

    #[test]
    fn group_add_rename_remove_prunes_profiles() {
        let mut s = base();
        s.profiles = vec![with_id("trojan://pw@a.com:443#A", "a", "g2")];
        apply_mutation(
            &mut s,
            &MutationIntent::AddGroup {
                id: "g3".into(),
                name: "Three".into(),
            },
        );
        assert!(s.groups.iter().any(|g| g.id == "g3"));
        apply_mutation(
            &mut s,
            &MutationIntent::RenameGroup {
                id: "g3".into(),
                name: "Tri".into(),
            },
        );
        assert_eq!(s.groups.iter().find(|g| g.id == "g3").unwrap().name, "Tri");
        // Removing g2 drops its profile too.
        apply_mutation(&mut s, &MutationIntent::RemoveGroup { id: "g2".into() });
        assert!(!s.groups.iter().any(|g| g.id == "g2"));
        assert!(s.profiles.is_empty());
        // The base group is protected.
        apply_mutation(
            &mut s,
            &MutationIntent::RemoveGroup {
                id: "g-main".into(),
            },
        );
        assert!(s.groups.iter().any(|g| g.id == "g-main"));
    }

    #[test]
    fn reorder_groups_pins_g_main() {
        let mut s = default_app_state(); // [g-main]
        for (i, name) in ["A", "B", "C"].iter().enumerate() {
            s.groups.push(Group {
                id: format!("g{i}"),
                name: (*name).into(),
            });
        }
        // groups: g-main, g0, g1, g2 → move g2 (idx 3) to front; clamps to idx 1.
        apply_mutation(&mut s, &MutationIntent::ReorderGroups { from: 3, to: 0 });
        let ids: Vec<&str> = s.groups.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(ids, vec!["g-main", "g2", "g0", "g1"]);
        // Trying to move g-main itself is a no-op.
        apply_mutation(&mut s, &MutationIntent::ReorderGroups { from: 0, to: 2 });
        assert_eq!(s.groups[0].id, "g-main");
    }

    #[test]
    fn import_routing_rules_merge_and_replace() {
        let mut s = base();
        let rule = |id: &str| RoutingRule {
            id: id.into(),
            remarks: id.into(),
            enabled: true,
            outbound_tag: "proxy".into(),
            domain: None,
            ip: None,
            port: None,
            network: None,
            protocol: None,
            process: None,
            package_name: None,
            source_ip: None,
        };
        s.routing_rules = vec![rule("r1")];
        apply_mutation(
            &mut s,
            &MutationIntent::ImportRoutingRules {
                rules: vec![rule("r2")],
                mode: ImportMode::Merge,
            },
        );
        let ids: Vec<&str> = s.routing_rules.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["r1", "r2"]);
        apply_mutation(
            &mut s,
            &MutationIntent::ImportRoutingRules {
                rules: vec![rule("r3")],
                mode: ImportMode::Replace,
            },
        );
        let ids: Vec<&str> = s.routing_rules.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["r3"]);
    }

    #[test]
    fn set_active_and_settings() {
        let mut s = base();
        apply_mutation(
            &mut s,
            &MutationIntent::SetActive {
                id: Some("x".into()),
            },
        );
        assert_eq!(s.active_id.as_deref(), Some("x"));
        let mut settings = s.settings.clone();
        settings.tun_mtu = 1280;
        apply_mutation(
            &mut s,
            &MutationIntent::SetSettings {
                settings: Box::new(settings),
            },
        );
        assert_eq!(s.settings.tun_mtu, 1280);
    }

    #[test]
    fn import_backup_replace_keeps_current_profiles() {
        let mut s = base();
        s.profiles = vec![with_id("trojan://pw@a.com:443#A", "a", "g-main")];
        let mut incoming = default_app_state();
        incoming.active_id = Some("ghost".into());
        incoming.groups.push(Group {
            id: "gx".into(),
            name: "X".into(),
        });
        apply_mutation(
            &mut s,
            &MutationIntent::ImportBackup {
                incoming: Box::new(incoming),
                mode: ImportMode::Replace,
            },
        );
        // Current profiles preserved, backup's groups adopted, dangling active left
        // for the middleware to null.
        assert_eq!(s.profiles.len(), 1);
        assert!(s.groups.iter().any(|g| g.id == "gx"));
        assert_eq!(s.active_id.as_deref(), Some("ghost"));
    }

    #[test]
    fn import_backup_merge_concats_lists() {
        let mut s = base();
        let mut incoming = default_app_state();
        incoming.groups.push(Group {
            id: "g9".into(),
            name: "Nine".into(),
        });
        let mut rule = incoming.settings.clone();
        rule.tun_mtu = 1280;
        incoming.settings = rule;
        apply_mutation(
            &mut s,
            &MutationIntent::ImportBackup {
                incoming: Box::new(incoming),
                mode: ImportMode::Merge,
            },
        );
        assert!(s.groups.iter().any(|g| g.id == "g9"));
        assert_eq!(s.settings.tun_mtu, 1280);
    }

    #[test]
    fn replace_state_swaps_wholesale() {
        let mut s = base();
        s.profiles = vec![with_id("trojan://pw@a.com:443#A", "a", "g-main")];
        let mut replacement = default_app_state();
        replacement.active_id = Some("z".into());
        apply_mutation(
            &mut s,
            &MutationIntent::ReplaceState {
                state: Box::new(replacement),
            },
        );
        assert!(s.profiles.is_empty());
        assert_eq!(s.active_id.as_deref(), Some("z"));
    }

    #[test]
    fn intent_wire_shape_is_kind_tagged() {
        let intent: MutationIntent = serde_json::from_value(serde_json::json!({
            "kind": "removeProfiles",
            "ids": ["a", "b"]
        }))
        .unwrap();
        assert!(matches!(intent, MutationIntent::RemoveProfiles { .. }));
        let v = serde_json::to_value(MutationIntent::SetActive { id: None }).unwrap();
        assert_eq!(v, serde_json::json!({ "kind": "setActive", "id": null }));
    }
}
