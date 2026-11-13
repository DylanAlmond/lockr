//! Symmetric merge of two copies of one vault.
//!
//! Merge is a pure function. It never reads a wall clock to decide a winner
//! and never needs a second round trip: either side computes the same result
//! from the same two copies. Ordering comes only from the Device version
//! vectors and per-field writer versions introduced in schema v3.
//!
//! An Item's lifecycle clock gates the merge first. A dominating copy is
//! adopted whole; concurrent copies are reconciled field by field. Two
//! concurrent writes to the same field are a [`Conflict`] and are reported
//! instead of being silently resolved. A Tombstone always wins, so a deleted
//! Item cannot resurrect from a stale live copy.
//!
//! A move is a lifecycle state of its own, not a deletion: the vault the item
//! left keeps a content-free record naming the destination. That record beats
//! the stale live copy a peer may still hold (so a move cannot duplicate the
//! identity across vaults), while a causally later live copy — a move back —
//! beats it.
//!
//! Conflict payloads follow the Safe View Pattern: they name the Item (or the
//! Vault), the field key, and both writers, but a secret field's value is
//! always withheld.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::Value;

use crate::{
    allowed_secret_keys, item_field_keys, ClockOrder, DeviceId, FieldVersion, ItemId, ItemKind,
    ItemSecrets, SecretField, Vault, VaultId, VaultItem, VersionVector,
};

/// Why two vault copies cannot be merged.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MergeError {
    /// The copies name different vaults. Merging them would silently
    /// relabel one vault's contents as the other's; replication must never
    /// be able to do that, so this is refused rather than resolved.
    #[error("cannot merge vaults with different ids: {left} and {right}")]
    VaultIdMismatch { left: VaultId, right: VaultId },
}

/// The result of merging two copies of a vault.
#[derive(Debug, Clone)]
pub enum MergeOutcome {
    /// The copies merged cleanly.
    Merged(Vault),

    /// At least one field was changed independently on both sides. `merged`
    /// carries every clean merge and a deterministic preview for contested
    /// fields; `conflicts` blocks persistence until the User resolves them.
    Conflicts(ConflictReport),
}

/// The conflicts found while merging, alongside the rest of the merge.
#[derive(Debug, Clone)]
pub struct ConflictReport {
    /// The vault with all clean merges applied. Contested fields hold a
    /// deterministic preview and must not be persisted until resolved.
    pub merged: Vault,

    /// One entry per contested field: Item fields ordered by Item and field
    /// key, followed by any Vault metadata fields.
    pub conflicts: Vec<Conflict>,
}

/// A field that changed concurrently on two Devices.
///
/// Safe to send to the UI: secret values are never present in `value`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Conflict {
    /// What the contested field belongs to: one Item, or the Vault itself.
    #[serde(flatten)]
    pub scope: ConflictScope,

    /// The wire field key, e.g. `password`, `display_name` or `name`.
    pub field_key: String,

    /// True when the contested field is a secret. In that case both sides'
    /// `value` is `None`, even though the backend holds both values.
    pub is_secret: bool,

    /// Both writers, ordered canonically so either Device reports the same
    /// conflict.
    pub sides: [ConflictSide; 2],
}

impl Conflict {
    /// The Item this conflict belongs to, or `None` for vault metadata.
    pub fn item_id(&self) -> Option<ItemId> {
        match &self.scope {
            ConflictScope::Item { item_id, .. } => Some(*item_id),
            ConflictScope::Vault => None,
        }
    }

    /// Whether this conflict is about the Vault's own name or colour.
    pub fn is_vault_metadata(&self) -> bool {
        self.scope == ConflictScope::Vault
    }
}

/// The owner of a contested field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum ConflictScope {
    Item {
        item_id: ItemId,

        /// Display name of the Item, for the prompt. It is a safe view field.
        #[serde(skip_serializing_if = "Option::is_none")]
        display_name: Option<String>,
    },

    Vault,
}

/// The provenance of one side of a [`Conflict`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ConflictSide {
    /// The Device that wrote this value.
    pub device: DeviceId,

    /// That Device's counter for this field.
    pub counter: u64,

    /// The non-secret value, as JSON. Always `None` for secret fields.
    pub value: Option<Value>,
}

/// A field value as it exists in one vault copy. Shared with Combine, which
/// needs the same field-key read/write surface as merge.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FieldValue {
    /// An optional text field; `None` means "not set".
    Text(Option<String>),

    /// A required text field.
    Plain(String),

    Bool(bool),

    Tags(Vec<String>),

    Number(u64),

    /// A secret value. It never leaves the backend through a `Conflict`.
    Secret(String),
}

impl FieldValue {
    pub(crate) fn conflict_value(&self) -> Option<Value> {
        match self {
            FieldValue::Secret(_) => None,
            FieldValue::Text(value) => {
                Some(value.clone().map(Value::String).unwrap_or(Value::Null))
            }
            FieldValue::Plain(value) => Some(Value::String(value.clone())),
            FieldValue::Bool(value) => Some(Value::Bool(*value)),
            FieldValue::Tags(values) => Some(
                serde_json::to_value(values)
                    .expect("a list of strings is always JSON-serialisable"),
            ),
            FieldValue::Number(value) => Some(Value::Number((*value).into())),
        }
    }
}

/// One field as observed on one side of the merge.
#[derive(Debug, Clone)]
struct FieldState {
    version: FieldVersion,
    value: Option<FieldValue>,
}

/// The clean result of reconciling one field, or the conflict it produced.
#[derive(Debug, Clone)]
struct FieldMerge {
    value: Option<FieldValue>,
    version: FieldVersion,
    has_version: bool,
    conflict: Option<[ConflictSide; 2]>,
}

/// The clean result of reconciling one vault metadata field, or the conflict
/// it produced.
struct VaultFieldMerge {
    value: String,
    version: FieldVersion,
    conflict: Option<[ConflictSide; 2]>,
}

/// Merge one of the Vault's own fields. Vault fields are simple values, so
/// this mirrors [`merge_field`] without the optional/secret variants.
fn merge_vault_field(
    left_value: &str,
    left_version: FieldVersion,
    left_clock: &VersionVector,
    right_value: &str,
    right_version: FieldVersion,
    right_clock: &VersionVector,
) -> VaultFieldMerge {
    if left_value == right_value {
        return VaultFieldMerge {
            value: left_value.to_string(),
            version: join_versions(left_version, right_version),
            conflict: None,
        };
    }

    let left_side = ConflictSide {
        device: left_version.device,
        counter: left_version.counter,
        value: Some(Value::String(left_value.to_string())),
    };
    let right_side = ConflictSide {
        device: right_version.device,
        counter: right_version.counter,
        value: Some(Value::String(right_value.to_string())),
    };

    match choose_side(left_clock, left_version, right_clock, right_version) {
        Some(true) => VaultFieldMerge {
            value: left_value.to_string(),
            version: left_version,
            conflict: None,
        },
        Some(false) => VaultFieldMerge {
            value: right_value.to_string(),
            version: right_version,
            conflict: None,
        },
        None => {
            let left_is_first = side_sort_key(&left_side) <= side_sort_key(&right_side);
            let (value, version, first, second) = if left_is_first {
                (left_value.to_string(), left_version, left_side, right_side)
            } else {
                (
                    right_value.to_string(),
                    right_version,
                    right_side,
                    left_side,
                )
            };

            VaultFieldMerge {
                value,
                version,
                conflict: Some([first, second]),
            }
        }
    }
}

/// One Item reconciled with its optional secret entry.
struct MergedItem {
    item: VaultItem,
    secrets: Option<ItemSecrets>,
    conflicts: Vec<Conflict>,
}

/// Merge two copies of the same vault.
///
/// Refuses copies that name different vault ids: a merge is only meaningful
/// for two copies of one identity, and replication must never relabel one
/// vault under another's id.
///
/// The function is commutative and associative for clean merges, and reports
/// the same conflict set whichever way round its arguments are passed.
pub fn merge_vaults(left: &Vault, right: &Vault) -> Result<MergeOutcome, MergeError> {
    if left.id != right.id {
        return Err(MergeError::VaultIdMismatch {
            left: left.id,
            right: right.id,
        });
    }

    // The common idempotent case: hand back the input untouched, preserving
    // its item order exactly. Compare in place so secrets are compared by
    // reference, never serialised into a temporary plaintext buffer.
    if left == right {
        return Ok(MergeOutcome::Merged(left.clone()));
    }

    let left_items = index_items(left);
    let right_items = index_items(right);
    let left_secrets = index_secrets(left);
    let right_secrets = index_secrets(right);

    let ids: BTreeSet<ItemId> = left_items
        .keys()
        .chain(right_items.keys())
        .copied()
        .collect();

    let mut items = Vec::with_capacity(ids.len());
    let mut secrets = Vec::new();
    let mut conflicts = Vec::new();

    for id in ids {
        let merged = match (left_items.get(&id), right_items.get(&id)) {
            (Some(left_item), Some(right_item)) => merge_items(
                left_item,
                left_secrets.get(&id).copied(),
                right_item,
                right_secrets.get(&id).copied(),
            ),
            (Some(item), None) => MergedItem {
                item: (*item).clone(),
                secrets: left_secrets.get(&id).map(|entry| canonical_secrets(entry)),
                conflicts: Vec::new(),
            },
            (None, Some(item)) => MergedItem {
                item: (*item).clone(),
                secrets: right_secrets.get(&id).map(|entry| canonical_secrets(entry)),
                conflicts: Vec::new(),
            },
            (None, None) => unreachable!("the id came from one of the two maps"),
        };

        items.push(merged.item);
        if let Some(entry) = merged.secrets {
            secrets.push(entry);
        }
        conflicts.extend(merged.conflicts);
    }

    // Vault metadata merges per field, exactly like an item's fields. A
    // rename on one Device therefore survives a sync with a Device that never
    // touched it, and a rename concurrent with a recolour keeps both.
    let name = merge_vault_field(
        &left.name,
        left.name_clock,
        &left.clock,
        &right.name,
        right.name_clock,
        &right.clock,
    );
    let color = merge_vault_field(
        &left.color,
        left.color_clock,
        &left.clock,
        &right.color,
        right.color_clock,
        &right.clock,
    );
    for (field_key, sides) in [("name", &name.conflict), ("color", &color.conflict)] {
        if let Some(sides) = sides {
            conflicts.push(Conflict {
                scope: ConflictScope::Vault,
                field_key: field_key.to_string(),
                is_secret: false,
                sides: sides.clone(),
            });
        }
    }

    let mut vault_clock = left.clock.clone();
    vault_clock.merge(&right.clock);

    let merged = Vault {
        schema_version: left.schema_version.max(right.schema_version),
        id: left.id,
        name: name.value,
        color: color.value,
        clock: vault_clock,
        name_clock: name.version,
        color_clock: color.version,
        items,
        secrets,
        // A decision is a set of pairs, so reconciling two Devices is a
        // union. Dismissing on either side silences both.
        duplicate_dismissals: union_dismissals(
            &left.duplicate_dismissals,
            &right.duplicate_dismissals,
        ),
    };

    if conflicts.is_empty() {
        Ok(MergeOutcome::Merged(merged))
    } else {
        Ok(MergeOutcome::Conflicts(ConflictReport {
            merged,
            conflicts,
        }))
    }
}

/// Set union of two Devices' Duplicate dismissals, in canonical order.
fn union_dismissals(
    left: &[crate::DismissedDuplicate],
    right: &[crate::DismissedDuplicate],
) -> Vec<crate::DismissedDuplicate> {
    let mut union: Vec<crate::DismissedDuplicate> =
        Vec::with_capacity(left.len() + right.len());

    for pair in left.iter().chain(right.iter()) {
        if !union.iter().any(|existing| {
            existing.first == pair.first && existing.second == pair.second
        }) {
            union.push(*pair);
        }
    }

    union.sort_by_key(|pair| (pair.first, pair.second));
    union
}

/// The merge result with every contested field left at the *local* value.
///
/// [`ConflictReport::merged`] carries a deterministic preview for contested
/// fields so two Devices compute the same report. That preview must never be
/// written to disk: it would silently choose one side of a genuine Conflict.
/// This function keeps every clean merge and restores each contested field -
/// value and field clock - from `local`, so the saved vault still says exactly
/// what this Device said before the sync.
pub(crate) fn keep_local_for_conflicts(local: &Vault, report: &ConflictReport) -> Vault {
    let mut merged = report.merged.clone();

    for conflict in &report.conflicts {
        match &conflict.scope {
            ConflictScope::Item { item_id, .. } => {
                let Some(local_item) = local.items.iter().find(|item| item.id == *item_id) else {
                    continue;
                };
                let Some(position) = merged.items.iter().position(|item| item.id == *item_id)
                else {
                    continue;
                };

                // Kind is not a versioned field. A kind collision should not
                // occur for honest copies; keep the local item whole rather
                // than pretending one side's kind won a field merge.
                if conflict.field_key == "kind" {
                    let clock = merged.items[position].clock.clone();
                    let mut item = local_item.clone();
                    item.clock = clock;
                    merged.items[position] = item;
                    restore_local_secrets(&mut merged, local, *item_id);
                    continue;
                }

                if conflict.is_secret {
                    restore_local_secret_field(&mut merged, local, *item_id, &conflict.field_key);
                } else if let Some(value) = read_item_field(local_item, &conflict.field_key) {
                    write_item_field(&mut merged.items[position], &conflict.field_key, &value);
                }

                restore_local_field_clock(
                    &mut merged.items[position],
                    local_item,
                    &conflict.field_key,
                );
            }
            ConflictScope::Vault => match conflict.field_key.as_str() {
                "name" => {
                    merged.name = local.name.clone();
                    merged.name_clock = local.name_clock;
                }
                "color" => {
                    merged.color = local.color.clone();
                    merged.color_clock = local.color_clock;
                }
                _ => {}
            },
        }
    }

    merged
}

fn restore_local_field_clock(merged_item: &mut VaultItem, local_item: &VaultItem, field_key: &str) {
    match local_item.field_clocks.get(field_key).copied() {
        Some(version) => {
            merged_item.record_field(field_key, version);
        }
        None => {
            merged_item.field_clocks.remove(field_key);
        }
    }
}

fn restore_local_secrets(merged: &mut Vault, local: &Vault, item_id: ItemId) {
    merged.secrets.retain(|entry| entry.item_id != item_id);
    if let Some(entry) = local.secrets.iter().find(|entry| entry.item_id == item_id) {
        merged.secrets.push(canonical_secrets(entry));
    }
}

fn restore_local_secret_field(merged: &mut Vault, local: &Vault, item_id: ItemId, field_key: &str) {
    let local_field = local
        .secrets
        .iter()
        .find(|entry| entry.item_id == item_id)
        .and_then(|entry| entry.fields.iter().find(|field| field.key == field_key))
        .cloned();

    let Some(position) = merged
        .secrets
        .iter()
        .position(|entry| entry.item_id == item_id)
    else {
        if let Some(field) = local_field {
            merged.secrets.push(ItemSecrets {
                item_id,
                fields: vec![field],
            });
        }
        return;
    };

    merged.secrets[position]
        .fields
        .retain(|field| field.key != field_key);
    if let Some(field) = local_field {
        merged.secrets[position].fields.push(field);
    }
    if merged.secrets[position].fields.is_empty() {
        merged.secrets.remove(position);
    }
}

/// Apply the User's explicit choice for one contested field.
///
/// The chosen value is stamped as a new write by `device`, so it causally
/// dominates both conflicting values and travels to the peer on the next
/// sync. This is the only path that may change a contested field on disk.
///
/// Returns `false` when the scope/field does not exist or the value's type is
/// not valid for that field; nothing is mutated in that case.
pub(crate) fn apply_conflict_value(
    vault: &mut Vault,
    scope: &ConflictScope,
    field_key: &str,
    is_secret: bool,
    value: Option<&FieldValue>,
    device: DeviceId,
) -> bool {
    match scope {
        ConflictScope::Item { item_id, .. } => {
            let Some(position) = vault
                .items
                .iter()
                .position(|item| item.id == *item_id && !item.is_content_free())
            else {
                return false;
            };

            if !is_secret {
                let Some(value) = value else {
                    return false;
                };
                if read_item_field(&vault.items[position], field_key).is_none() {
                    return false;
                }
                if !write_item_field(&mut vault.items[position], field_key, value) {
                    return false;
                }
            }

            let counter = vault.items[position].clock.increment(device);
            let version = FieldVersion::new(device, counter);
            vault.items[position].record_field(field_key, version);

            if is_secret {
                apply_secret_value(vault, *item_id, field_key, value, version);
            }

            true
        }
        ConflictScope::Vault => {
            let Some(FieldValue::Plain(value)) = value else {
                return false;
            };
            let counter = vault.clock.increment(device);
            let version = FieldVersion::new(device, counter);
            match field_key {
                "name" => {
                    vault.name = value.clone();
                    vault.name_clock = version;
                }
                "color" => {
                    vault.color = value.clone();
                    vault.color_clock = version;
                }
                _ => return false,
            }
            true
        }
    }
}

fn apply_secret_value(
    vault: &mut Vault,
    item_id: ItemId,
    field_key: &str,
    value: Option<&FieldValue>,
    version: FieldVersion,
) {
    let secret = match value {
        Some(FieldValue::Secret(value)) => Some(value.clone()),
        _ => None,
    };

    match (
        vault
            .secrets
            .iter()
            .position(|entry| entry.item_id == item_id),
        secret,
    ) {
        (Some(position), Some(value)) => vault.secrets[position].set(field_key, value, version),
        (Some(position), None) => {
            vault.secrets[position].remove(field_key);
            if vault.secrets[position].fields.is_empty() {
                vault.secrets.remove(position);
            }
        }
        (None, Some(value)) => vault
            .secrets
            .push(ItemSecrets::new(item_id, field_key, value, version)),
        (None, None) => {}
    }
}

fn index_items(vault: &Vault) -> BTreeMap<ItemId, &VaultItem> {
    vault.items.iter().map(|item| (item.id, item)).collect()
}

fn index_secrets(vault: &Vault) -> BTreeMap<ItemId, &ItemSecrets> {
    vault
        .secrets
        .iter()
        .map(|entry| (entry.item_id, entry))
        .collect()
}

fn canonical_secrets(entry: &ItemSecrets) -> ItemSecrets {
    let mut fields = entry.fields.clone();
    fields.sort_by(|a, b| a.key.cmp(&b.key));
    ItemSecrets {
        item_id: entry.item_id,
        fields,
    }
}

fn merge_items(
    left: &VaultItem,
    left_secrets: Option<&ItemSecrets>,
    right: &VaultItem,
    right_secrets: Option<&ItemSecrets>,
) -> MergedItem {
    debug_assert_eq!(left.id, right.id, "merge_items needs the same identity");

    match (lifecycle(left), lifecycle(right)) {
        // Deletion is permanent for an identity. A Tombstone wins even
        // against a concurrent live edit or a move record, whose clocks are
        // absorbed so the next merge does not re-litigate the deletion.
        (ItemLifecycle::Deleted, _) | (_, ItemLifecycle::Deleted) => MergedItem {
            item: VaultItem::tombstone(
                left.id,
                merged_item_vault_id(left, right),
                joined_clock(left, right),
            ),
            secrets: None,
            conflicts: Vec::new(),
        },

        // Two move records for one identity: either the same event reaching
        // this vault twice, or two Devices moved the item out of it
        // concurrently. Pick the smaller destination so both sides compute
        // the same answer; the item itself lives on in that vault.
        (ItemLifecycle::Moved(left_destination), ItemLifecycle::Moved(right_destination)) => {
            MergedItem {
                item: VaultItem::move_record(
                    left.id,
                    merged_item_vault_id(left, right),
                    joined_clock(left, right),
                    left_destination.min(right_destination),
                ),
                secrets: None,
                conflicts: Vec::new(),
            }
        }

        // A move record beats the stale live copy it left behind. A live
        // copy only wins when it is causally *later*: that is a move back,
        // where the same identity was deliberately brought here again.
        (ItemLifecycle::Moved(destination), ItemLifecycle::Live) => {
            if right.clock.compare(&left.clock) == ClockOrder::Dominates {
                MergedItem {
                    item: right.clone(),
                    secrets: right_secrets.cloned(),
                    conflicts: Vec::new(),
                }
            } else {
                merged_move_record(left, right, destination)
            }
        }
        (ItemLifecycle::Live, ItemLifecycle::Moved(destination)) => {
            if left.clock.compare(&right.clock) == ClockOrder::Dominates {
                MergedItem {
                    item: left.clone(),
                    secrets: left_secrets.cloned(),
                    conflicts: Vec::new(),
                }
            } else {
                merged_move_record(left, right, destination)
            }
        }

        (ItemLifecycle::Live, ItemLifecycle::Live) => match left.clock.compare(&right.clock) {
            ClockOrder::Dominates => MergedItem {
                item: left.clone(),
                secrets: left_secrets.cloned(),
                conflicts: Vec::new(),
            },
            ClockOrder::Dominated => MergedItem {
                item: right.clone(),
                secrets: right_secrets.cloned(),
                conflicts: Vec::new(),
            },
            ClockOrder::Equal | ClockOrder::Concurrent => {
                merge_live_items(left, left_secrets, right, right_secrets)
            }
        },
    }
}

/// The lifecycle state a stored item represents. Move records are
/// deliberately distinct from Tombstones: "it is over there" must be able to
/// lose to a later move back, while "it is deleted" never can.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ItemLifecycle {
    Live,
    Deleted,
    Moved(VaultId),
}

fn lifecycle(item: &VaultItem) -> ItemLifecycle {
    if item.deleted {
        ItemLifecycle::Deleted
    } else if let Some(destination) = item.moved_to {
        ItemLifecycle::Moved(destination)
    } else if item.kind.is_none() {
        // Defensive: a content-free record that names no move is a
        // Tombstone written by an older/corrupt writer.
        ItemLifecycle::Deleted
    } else {
        ItemLifecycle::Live
    }
}

fn joined_clock(left: &VaultItem, right: &VaultItem) -> VersionVector {
    let mut clock = left.clock.clone();
    clock.merge(&right.clock);
    clock
}

fn merged_item_vault_id(left: &VaultItem, right: &VaultItem) -> VaultId {
    left.vault_id.min(right.vault_id)
}

fn merged_move_record(left: &VaultItem, right: &VaultItem, destination: VaultId) -> MergedItem {
    MergedItem {
        item: VaultItem::move_record(
            left.id,
            merged_item_vault_id(left, right),
            joined_clock(left, right),
            destination,
        ),
        secrets: None,
        conflicts: Vec::new(),
    }
}

fn merge_live_items(
    left: &VaultItem,
    left_secrets: Option<&ItemSecrets>,
    right: &VaultItem,
    right_secrets: Option<&ItemSecrets>,
) -> MergedItem {
    let left_kind = left
        .kind
        .as_ref()
        .expect("a live item always carries a kind");
    let right_kind = right
        .kind
        .as_ref()
        .expect("a live item always carries a kind");

    let display_name = canonical_display_name(&left.display_name, &right.display_name);

    // Kind is not a versioned field. An honest pair of copies cannot differ
    // here without one side changing kind, which the manager forbids.
    if kind_tag(left_kind) != kind_tag(right_kind) {
        let mut clock = left.clock.clone();
        clock.merge(&right.clock);
        let left_wins = kind_tag(left_kind) <= kind_tag(right_kind);
        let mut item = if left_wins {
            left.clone()
        } else {
            right.clone()
        };
        item.clock = clock.clone();

        let left_version = latest_clock_version(&left.clock);
        let right_version = latest_clock_version(&right.clock);
        let mut sides = [
            ConflictSide {
                device: left_version.device,
                counter: left_version.counter,
                value: Some(Value::String(kind_tag(left_kind).to_string())),
            },
            ConflictSide {
                device: right_version.device,
                counter: right_version.counter,
                value: Some(Value::String(kind_tag(right_kind).to_string())),
            },
        ];
        sides.sort_by(|a, b| side_sort_key(a).cmp(&side_sort_key(b)));

        return MergedItem {
            item,
            secrets: if left_wins {
                left_secrets.cloned()
            } else {
                right_secrets.cloned()
            },
            conflicts: vec![Conflict {
                scope: ConflictScope::Item {
                    item_id: left.id,
                    display_name,
                },
                field_key: "kind".to_string(),
                is_secret: false,
                sides,
            }],
        };
    }

    let mut merged = left.clone();
    merged.clock = {
        let mut clock = left.clock.clone();
        clock.merge(&right.clock);
        clock
    };
    merged.created_at = left.created_at.min(right.created_at);
    // Timestamps are display data only; taking the later one cannot influence
    // which value wins.
    merged.updated_at = left.updated_at.max(right.updated_at);
    merged.vault_id = if left.vault_id == right.vault_id {
        left.vault_id
    } else {
        left.vault_id.min(right.vault_id)
    };
    merged.deleted = false;
    merged.moved_to = None;
    merged.field_clocks = BTreeMap::new();

    let secret_keys = allowed_secret_keys(left_kind);
    let mut keys: BTreeSet<&'static str> = item_field_keys(left_kind).into_iter().collect();
    keys.extend(secret_keys.iter().copied());

    let mut conflicts = Vec::new();
    let mut secret_values: BTreeMap<&'static str, (String, FieldVersion)> = BTreeMap::new();

    for key in keys {
        let is_secret = secret_keys.contains(&key);
        let left_state = field_state(left, left_secrets, key, is_secret);
        let right_state = field_state(right, right_secrets, key, is_secret);

        let resolved = merge_field(
            &left.clock,
            &left_state,
            &right.clock,
            &right_state,
            is_secret,
        );

        if resolved.has_version {
            merged.record_field(key, resolved.version);
        }

        if let Some(sides) = &resolved.conflict {
            conflicts.push(Conflict {
                scope: ConflictScope::Item {
                    item_id: left.id,
                    display_name: display_name.clone(),
                },
                field_key: key.to_string(),
                is_secret,
                sides: sides.clone(),
            });
        }

        match resolved.value {
            Some(FieldValue::Secret(value)) if is_secret => {
                secret_values.insert(key, (value, resolved.version));
            }
            Some(value) if !is_secret => {
                write_item_field(&mut merged, key, &value);
            }
            _ => {}
        }
    }

    let secrets = if secret_values.is_empty() {
        None
    } else {
        Some(ItemSecrets {
            item_id: left.id,
            fields: secret_values
                .into_iter()
                .map(|(key, (value, clock))| SecretField {
                    key: key.to_string(),
                    value,
                    clock,
                })
                .collect(),
        })
    };

    MergedItem {
        item: merged,
        secrets,
        conflicts,
    }
}

fn canonical_display_name(left: &Option<String>, right: &Option<String>) -> Option<String> {
    match (left, right) {
        (Some(left), Some(right)) => Some(if left <= right {
            left.clone()
        } else {
            right.clone()
        }),
        (Some(left), None) => Some(left.clone()),
        (None, Some(right)) => Some(right.clone()),
        (None, None) => None,
    }
}

pub(crate) fn kind_tag(kind: &ItemKind) -> &'static str {
    match kind {
        ItemKind::Account(_) => "account",
        ItemKind::Card(_) => "card",
        ItemKind::Note(_) => "note",
        ItemKind::Identity(_) => "identity",
    }
}

fn latest_clock_version(clock: &VersionVector) -> FieldVersion {
    clock
        .iter()
        .max_by_key(|(device, counter)| (**counter, **device))
        .map(|(device, counter)| FieldVersion::new(*device, *counter))
        .unwrap_or_default()
}

fn field_state(
    item: &VaultItem,
    secrets: Option<&ItemSecrets>,
    key: &str,
    is_secret: bool,
) -> FieldState {
    let version = field_version(item, secrets, key);
    let value = if is_secret {
        secrets
            .and_then(|entry| entry.get(key))
            .map(|value| FieldValue::Secret(value.to_string()))
    } else {
        read_item_field(item, key)
    };

    FieldState { version, value }
}

/// The item's per-field map is authoritative because it also records a
/// removal; fall back to the secret's own clock only if the map is missing.
fn field_version(item: &VaultItem, secrets: Option<&ItemSecrets>, key: &str) -> FieldVersion {
    let from_item = item.field_clocks.get(key).copied();
    let from_secret = secrets
        .and_then(|entry| entry.fields.iter().find(|field| field.key == key))
        .map(|field| field.clock);

    match (from_item, from_secret) {
        (Some(item_version), Some(secret_version)) => {
            if item_version == secret_version {
                item_version
            } else if item_version.device == secret_version.device
                && secret_version.counter > item_version.counter
            {
                secret_version
            } else {
                item_version
            }
        }
        (Some(version), None) | (None, Some(version)) => version,
        (None, None) => FieldVersion::default(),
    }
}

/// Reconcile one field. `None` from [`choose_side`] means the two writes are
/// concurrent: that is exactly the Conflict definition.
fn merge_field(
    left_clock: &VersionVector,
    left: &FieldState,
    right_clock: &VersionVector,
    right: &FieldState,
    is_secret: bool,
) -> FieldMerge {
    if left.value == right.value {
        let version = join_versions(left.version, right.version);
        return FieldMerge {
            value: left.value.clone(),
            has_version: version != FieldVersion::default(),
            version,
            conflict: None,
        };
    }

    match choose_side(left_clock, left.version, right_clock, right.version) {
        Some(true) => FieldMerge {
            value: left.value.clone(),
            has_version: left.version != FieldVersion::default(),
            version: left.version,
            conflict: None,
        },
        Some(false) => FieldMerge {
            value: right.value.clone(),
            has_version: right.version != FieldVersion::default(),
            version: right.version,
            conflict: None,
        },
        None => {
            let left_side = conflict_side(left, is_secret);
            let right_side = conflict_side(right, is_secret);
            let left_is_first = side_sort_key(&left_side) <= side_sort_key(&right_side);
            let (first, second) = if left_is_first {
                (left_side, right_side)
            } else {
                (right_side, left_side)
            };
            let (value, version) = if left_is_first {
                (left.value.clone(), left.version)
            } else {
                (right.value.clone(), right.version)
            };

            FieldMerge {
                value,
                has_version: version != FieldVersion::default(),
                version,
                conflict: Some([first, second]),
            }
        }
    }
}

/// `Some(true)` means the left write is causally later, `Some(false)` the
/// right, and `None` means the two are concurrent.
fn choose_side(
    left_clock: &VersionVector,
    left: FieldVersion,
    right_clock: &VersionVector,
    right: FieldVersion,
) -> Option<bool> {
    if left == right {
        return None;
    }

    if left.device == right.device {
        return Some(left.counter > right.counter);
    }

    let left_saw_right = left_clock.get(&right.device) >= right.counter;
    let right_saw_left = right_clock.get(&left.device) >= left.counter;

    match (left_saw_right, right_saw_left) {
        (true, false) => Some(true),
        (false, true) => Some(false),
        // Neither side observed the other: concurrent. Both observing yet
        // still holding different versions indicates inconsistent state, so
        // treat it as a conflict too rather than picking arbitrarily.
        (false, false) | (true, true) => None,
    }
}

fn join_versions(left: FieldVersion, right: FieldVersion) -> FieldVersion {
    if left == right {
        left
    } else if left.device == right.device {
        if left.counter >= right.counter {
            left
        } else {
            right
        }
    } else if left == FieldVersion::default() {
        right
    } else if right == FieldVersion::default() {
        left
    } else if (left.device, left.counter) >= (right.device, right.counter) {
        left
    } else {
        right
    }
}

fn conflict_side(state: &FieldState, is_secret: bool) -> ConflictSide {
    ConflictSide {
        device: state.version.device,
        counter: state.version.counter,
        value: if is_secret {
            None
        } else {
            state.value.as_ref().and_then(FieldValue::conflict_value)
        },
    }
}

fn side_sort_key(side: &ConflictSide) -> (DeviceId, u64, String) {
    (
        side.device,
        side.counter,
        side.value
            .as_ref()
            .map(Value::to_string)
            .unwrap_or_default(),
    )
}

pub(crate) fn read_item_field(item: &VaultItem, key: &str) -> Option<FieldValue> {
    match key {
        "display_name" => Some(FieldValue::Text(item.display_name.clone())),
        "favourite" => Some(FieldValue::Bool(item.favourite)),
        "tags" => Some(FieldValue::Tags(item.tags.clone())),
        "icon" => Some(FieldValue::Text(item.icon.clone())),
        "color" => Some(FieldValue::Plain(item.color.clone())),
        _ => {
            let kind = item.kind.as_ref()?;
            match kind {
                ItemKind::Account(data) => match key {
                    "username" => Some(FieldValue::Plain(data.username.clone())),
                    "email" => Some(FieldValue::Text(data.email.clone())),
                    _ => None,
                },
                ItemKind::Card(data) => match key {
                    "cardholder_name" => Some(FieldValue::Plain(data.cardholder_name.clone())),
                    "expiry_month" => Some(FieldValue::Number(u64::from(data.expiry_month))),
                    "expiry_year" => Some(FieldValue::Number(u64::from(data.expiry_year))),
                    "brand" => Some(FieldValue::Text(data.brand.clone())),
                    _ => None,
                },
                ItemKind::Note(data) => match key {
                    "title" => Some(FieldValue::Text(data.title.clone())),
                    "body" => Some(FieldValue::Plain(data.body.clone())),
                    _ => None,
                },
                ItemKind::Identity(data) => match key {
                    "full_name" => Some(FieldValue::Plain(data.full_name.clone())),
                    "email" => Some(FieldValue::Text(data.email.clone())),
                    "phone" => Some(FieldValue::Text(data.phone.clone())),
                    "address_line1" => Some(FieldValue::Text(data.address_line1.clone())),
                    "address_line2" => Some(FieldValue::Text(data.address_line2.clone())),
                    "city" => Some(FieldValue::Text(data.city.clone())),
                    "postal_code" => Some(FieldValue::Text(data.postal_code.clone())),
                    "country" => Some(FieldValue::Text(data.country.clone())),
                    _ => None,
                },
            }
        }
    }
}

pub(crate) fn write_item_field(item: &mut VaultItem, key: &str, value: &FieldValue) -> bool {
    match key {
        "display_name" => {
            let FieldValue::Text(value) = value else {
                return false;
            };
            item.display_name = value.clone();
        }
        "favourite" => {
            let FieldValue::Bool(value) = value else {
                return false;
            };
            item.favourite = *value;
        }
        "tags" => {
            let FieldValue::Tags(value) = value else {
                return false;
            };
            item.tags = value.clone();
        }
        "icon" => {
            let FieldValue::Text(value) = value else {
                return false;
            };
            item.icon = value.clone();
        }
        "color" => {
            let FieldValue::Plain(value) = value else {
                return false;
            };
            item.color = value.clone();
        }
        _ => {
            let Some(kind) = item.kind.as_mut() else {
                return false;
            };
            match (kind, key, value) {
                (ItemKind::Account(data), "username", FieldValue::Plain(value)) => {
                    data.username = value.clone();
                }
                (ItemKind::Account(data), "email", FieldValue::Text(value)) => {
                    data.email = value.clone();
                }
                (ItemKind::Card(data), "cardholder_name", FieldValue::Plain(value)) => {
                    data.cardholder_name = value.clone();
                }
                (ItemKind::Card(data), "expiry_month", FieldValue::Number(value)) => {
                    data.expiry_month = *value as u8;
                }
                (ItemKind::Card(data), "expiry_year", FieldValue::Number(value)) => {
                    data.expiry_year = *value as u16;
                }
                (ItemKind::Card(data), "brand", FieldValue::Text(value)) => {
                    data.brand = value.clone();
                }
                (ItemKind::Note(data), "title", FieldValue::Text(value)) => {
                    data.title = value.clone();
                }
                (ItemKind::Note(data), "body", FieldValue::Plain(value)) => {
                    data.body = value.clone();
                }
                (ItemKind::Identity(data), "full_name", FieldValue::Plain(value)) => {
                    data.full_name = value.clone();
                }
                (ItemKind::Identity(data), "email", FieldValue::Text(value)) => {
                    data.email = value.clone();
                }
                (ItemKind::Identity(data), "phone", FieldValue::Text(value)) => {
                    data.phone = value.clone();
                }
                (ItemKind::Identity(data), "address_line1", FieldValue::Text(value)) => {
                    data.address_line1 = value.clone();
                }
                (ItemKind::Identity(data), "address_line2", FieldValue::Text(value)) => {
                    data.address_line2 = value.clone();
                }
                (ItemKind::Identity(data), "city", FieldValue::Text(value)) => {
                    data.city = value.clone();
                }
                (ItemKind::Identity(data), "postal_code", FieldValue::Text(value)) => {
                    data.postal_code = value.clone();
                }
                (ItemKind::Identity(data), "country", FieldValue::Text(value)) => {
                    data.country = value.clone();
                }
                _ => return false,
            }
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use uuid::Uuid;

    use crate::vault_manager::VaultManager;
    use crate::{
        AccountData, CardData, NewItemData, SecretInput, CURRENT_SCHEMA_VERSION,
        SECRET_FIELD_PASSWORD,
    };

    fn device(n: u128) -> DeviceId {
        Uuid::from_u128(n)
    }

    fn fixture_time(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    fn base_fixture() -> (Vault, ItemId, DeviceId) {
        let owner = device(1);
        let vault_id = Uuid::from_u128(0xAAAA);
        let item_id = Uuid::from_u128(0xBBBB);
        let now = fixture_time(1_700_000_000);

        let mut item = VaultItem {
            id: item_id,
            vault_id,
            display_name: Some("Octocat".to_string()),
            favourite: false,
            tags: vec!["work".to_string()],
            icon: Some("github".to_string()),
            color: "#6240BF".to_string(),
            created_at: now,
            updated_at: now,
            clock: VersionVector::from_version(owner, 1),
            field_clocks: BTreeMap::new(),
            deleted: false,
            moved_to: None,
            kind: Some(ItemKind::Account(AccountData {
                username: "octocat".to_string(),
                email: Some("octo@example.com".to_string()),
            })),
        };
        item.stamp_all_fields(owner, 1);
        item.record_field(SECRET_FIELD_PASSWORD, FieldVersion::new(owner, 1));

        let vault = Vault {
            schema_version: CURRENT_SCHEMA_VERSION,
            id: vault_id,
            name: "Test".to_string(),
            color: "#6240BF".to_string(),
            clock: VersionVector::from_version(owner, 1),
            name_clock: FieldVersion::new(owner, 1),
            color_clock: FieldVersion::new(owner, 1),
            items: vec![item],
            secrets: vec![ItemSecrets::new(
                item_id,
                SECRET_FIELD_PASSWORD,
                "hunter2".to_string(),
                FieldVersion::new(owner, 1),
            )],
            duplicate_dismissals: Vec::new(),
        };

        (vault, item_id, owner)
    }

    fn stored_item<'a>(vault: &'a Vault, item_id: ItemId) -> &'a VaultItem {
        vault
            .items
            .iter()
            .find(|item| item.id == item_id)
            .expect("fixture item exists")
    }

    fn bump(item: &mut VaultItem, writer: DeviceId) -> FieldVersion {
        let counter = item.clock.increment(writer);
        item.updated_at = Utc::now();
        FieldVersion::new(writer, counter)
    }

    fn write_display_name(vault: &mut Vault, item_id: ItemId, writer: DeviceId, value: &str) {
        let item = vault
            .items
            .iter_mut()
            .find(|item| item.id == item_id)
            .expect("fixture item exists");
        let version = bump(item, writer);
        item.display_name = Some(value.to_string());
        item.record_field("display_name", version);
    }

    fn write_password(vault: &mut Vault, item_id: ItemId, writer: DeviceId, value: &str) {
        let version = {
            let item = vault
                .items
                .iter_mut()
                .find(|item| item.id == item_id)
                .expect("fixture item exists");
            let version = bump(item, writer);
            item.record_field(SECRET_FIELD_PASSWORD, version);
            version
        };

        let entry = vault
            .secrets
            .iter_mut()
            .find(|entry| entry.item_id == item_id)
            .expect("fixture secret entry exists");
        entry.set(SECRET_FIELD_PASSWORD, value.to_string(), version);
    }

    fn write_favourite(vault: &mut Vault, item_id: ItemId, writer: DeviceId, value: bool) {
        let item = vault
            .items
            .iter_mut()
            .find(|item| item.id == item_id)
            .expect("fixture item exists");
        let version = bump(item, writer);
        item.favourite = value;
        item.record_field("favourite", version);
    }

    fn expect_merged(outcome: Result<MergeOutcome, MergeError>) -> Vault {
        match outcome.expect("the fixture copies name one vault") {
            MergeOutcome::Merged(vault) => vault,
            MergeOutcome::Conflicts(report) => {
                panic!("expected a clean merge, got {:?}", report.conflicts)
            }
        }
    }

    fn expect_conflicts(outcome: Result<MergeOutcome, MergeError>) -> ConflictReport {
        match outcome.expect("the fixture copies name one vault") {
            MergeOutcome::Merged(_) => panic!("expected conflicts, got a clean merge"),
            MergeOutcome::Conflicts(report) => report,
        }
    }

    #[test]
    fn test_disjoint_field_edits_merge_without_conflict() {
        let (base, item_id, owner) = base_fixture();
        let other = device(2);

        let mut left = base.clone();
        write_display_name(&mut left, item_id, owner, "Renamed");

        let mut right = base.clone();
        write_password(&mut right, item_id, other, "new-secret");

        let merged = expect_merged(merge_vaults(&left, &right));

        assert_eq!(
            stored_item(&merged, item_id).display_name.as_deref(),
            Some("Renamed")
        );
        assert_eq!(
            merged
                .secrets
                .iter()
                .find(|entry| entry.item_id == item_id)
                .and_then(|entry| entry.get(SECRET_FIELD_PASSWORD)),
            Some("new-secret")
        );

        // The same result whichever way round the copies are passed.
        let reverse = expect_merged(merge_vaults(&right, &left));
        assert_eq!(
            serde_json::to_value(&merged).unwrap(),
            serde_json::to_value(&reverse).unwrap()
        );
    }

    #[test]
    fn test_same_field_concurrent_edits_report_exactly_one_conflict() {
        let (base, item_id, owner) = base_fixture();
        let other = device(2);

        let mut left = base.clone();
        write_display_name(&mut left, item_id, owner, "Left name");

        let mut right = base.clone();
        write_display_name(&mut right, item_id, other, "Right name");

        let report = expect_conflicts(merge_vaults(&left, &right));
        assert_eq!(report.conflicts.len(), 1);

        let conflict = &report.conflicts[0];
        assert_eq!(conflict.item_id(), Some(item_id));
        assert_eq!(conflict.field_key, "display_name");
        assert!(!conflict.is_secret);
        assert_eq!(conflict.sides.len(), 2);

        let values: Vec<&str> = conflict
            .sides
            .iter()
            .filter_map(|side| side.value.as_ref())
            .filter_map(Value::as_str)
            .collect();
        assert!(values.contains(&"Left name"));
        assert!(values.contains(&"Right name"));

        // Conflict provenance is canonical: both Devices report the same
        // conflict payload, including the preview merge.
        let reverse = expect_conflicts(merge_vaults(&right, &left));
        assert_eq!(
            serde_json::to_value(&report.conflicts).unwrap(),
            serde_json::to_value(&reverse.conflicts).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&report.merged).unwrap(),
            serde_json::to_value(&reverse.merged).unwrap()
        );
    }

    #[test]
    fn test_same_field_secret_conflict_never_carries_the_secret() {
        let (base, item_id, owner) = base_fixture();
        let other = device(2);

        let mut left = base.clone();
        write_password(&mut left, item_id, owner, "left-secret-value");

        let mut right = base.clone();
        write_password(&mut right, item_id, other, "right-secret-value");

        let report = expect_conflicts(merge_vaults(&left, &right));
        assert_eq!(report.conflicts.len(), 1);

        let conflict = &report.conflicts[0];
        assert_eq!(conflict.field_key, SECRET_FIELD_PASSWORD);
        assert!(conflict.is_secret);
        assert!(conflict.sides.iter().all(|side| side.value.is_none()));

        let payload = serde_json::to_string(&report.conflicts).unwrap();
        assert!(!payload.contains("left-secret-value"));
        assert!(!payload.contains("right-secret-value"));
    }

    #[test]
    fn test_move_record_beats_a_stale_live_copy_in_the_source_vault() {
        let (base, item_id, owner) = base_fixture();
        let destination = Uuid::from_u128(0xCCCC);

        let moved = moved_out(&base, item_id, owner, destination);

        for (left, right) in [(&moved, &base), (&base, &moved)] {
            let merged = expect_merged(merge_vaults(left, right));
            let item = stored_item(&merged, item_id);
            assert!(
                item.is_move_record(),
                "the move record must beat the stale live copy"
            );
            assert!(!item.is_deleted(), "moving out is not a deletion");
            assert_eq!(item.moved_to, Some(destination));
            assert!(item.kind.is_none(), "a move record carries no content");
            assert!(
                merged.secrets.iter().all(|entry| entry.item_id != item_id),
                "a move record carries no secrets"
            );
        }
    }

    #[test]
    fn test_moving_back_wins_over_the_earlier_move_record() {
        let (base, item_id, owner) = base_fixture();
        let destination = Uuid::from_u128(0xCCCC);

        let moved = moved_out(&base, item_id, owner, destination);

        // The item came back to this vault: same identity, a later clock and
        // its content again.
        let mut moved_back = base.clone();
        {
            let item = moved_back
                .items
                .iter_mut()
                .find(|item| item.id == item_id)
                .unwrap();
            item.clock.increment(owner);
            item.clock.increment(owner);
        }

        for (left, right) in [(&moved, &moved_back), (&moved_back, &moved)] {
            let merged = expect_merged(merge_vaults(left, right));
            let item = stored_item(&merged, item_id);
            assert!(!item.is_content_free(), "moving back must restore the item");
            assert_eq!(
                item.display_name.as_deref(),
                Some("Octocat"),
                "moving back must keep the item's content"
            );
            assert!(merged.secrets.iter().any(|entry| entry.item_id == item_id));
        }
    }

    #[test]
    fn test_deletion_after_a_move_beats_the_move_record_and_a_stale_live_copy() {
        let (base, item_id, owner) = base_fixture();
        let destination = Uuid::from_u128(0xCCCC);

        let moved = moved_out(&base, item_id, owner, destination);
        let deleted = {
            let mut deleted = base.clone();
            let clock = {
                let item = deleted
                    .items
                    .iter_mut()
                    .find(|item| item.id == item_id)
                    .unwrap();
                item.clock.increment(owner);
                item.clock.increment(owner);
                item.clock.clone()
            };
            deleted.items = vec![VaultItem::tombstone(item_id, deleted.id, clock)];
            deleted.secrets.clear();
            deleted
        };

        for (left, right) in [
            (&deleted, &moved),
            (&moved, &deleted),
            (&deleted, &base),
            (&base, &deleted),
        ] {
            let merged = expect_merged(merge_vaults(left, right));
            assert!(
                stored_item(&merged, item_id).is_deleted(),
                "deletion must win over a move and over a stale live copy"
            );
            assert!(merged.secrets.iter().all(|entry| entry.item_id != item_id));
        }
    }

    #[test]
    fn test_move_lifecycle_merge_is_associative() {
        let (base, item_id, owner) = base_fixture();
        let destination = Uuid::from_u128(0xCCCC);

        let stale = base.clone();
        let moved = moved_out(&base, item_id, owner, destination);
        let mut moved_back = base.clone();
        {
            let item = moved_back
                .items
                .iter_mut()
                .find(|item| item.id == item_id)
                .unwrap();
            item.clock.increment(owner);
            item.clock.increment(owner);
        }

        let left = expect_merged(merge_vaults(
            &expect_merged(merge_vaults(&stale, &moved)),
            &moved_back,
        ));
        let right = expect_merged(merge_vaults(
            &stale,
            &expect_merged(merge_vaults(&moved, &moved_back)),
        ));
        assert_eq!(
            serde_json::to_value(&left).unwrap(),
            serde_json::to_value(&right).unwrap()
        );
        assert!(!stored_item(&left, item_id).is_content_free());
    }

    #[test]
    fn test_move_record_moves_with_its_vault_metadata() {
        let (base, item_id, owner) = base_fixture();
        let destination = Uuid::from_u128(0xCCCC);

        let moved = moved_out(&base, item_id, owner, destination);
        let merged = expect_merged(merge_vaults(&moved, &moved));
        assert_eq!(merged.id, base.id);
        assert_eq!(stored_item(&merged, item_id).moved_to, Some(destination));
    }

    fn moved_out(base: &Vault, item_id: ItemId, writer: DeviceId, destination: VaultId) -> Vault {
        let mut moved = base.clone();
        let clock = {
            let item = moved
                .items
                .iter_mut()
                .find(|item| item.id == item_id)
                .expect("fixture item exists");
            item.clock.increment(writer);
            item.clock.clone()
        };
        moved.items = vec![VaultItem::move_record(
            item_id,
            moved.id,
            clock,
            destination,
        )];
        moved.secrets.clear();
        moved
    }

    #[test]
    fn test_merge_refuses_two_different_vault_ids() {
        let (base, _, _) = base_fixture();
        let mut other = base.clone();
        other.id = Uuid::from_u128(0xDDDD);

        let error = merge_vaults(&base, &other).unwrap_err();
        assert_eq!(
            error,
            MergeError::VaultIdMismatch {
                left: base.id,
                right: other.id,
            }
        );

        // Refusing is symmetric: neither argument position is treated as the
        // authoritative id.
        let reverse = merge_vaults(&other, &base).unwrap_err();
        assert_eq!(
            reverse,
            MergeError::VaultIdMismatch {
                left: other.id,
                right: base.id,
            }
        );
    }

    #[test]
    fn test_vault_rename_survives_a_sync_with_an_untouched_copy() {
        let (base, _, owner) = base_fixture();

        let mut renamed = base.clone();
        rename_vault(&mut renamed, owner, "Renamed");

        for (left, right) in [(&renamed, &base), (&base, &renamed)] {
            let merged = expect_merged(merge_vaults(left, right));
            assert_eq!(
                merged.name, "Renamed",
                "an untouched copy must not win a rename by being lexicographically smaller"
            );
            assert_eq!(merged.color, base.color);
        }
    }

    #[test]
    fn test_concurrent_vault_rename_and_recolour_merge_without_conflict() {
        let (base, _, owner) = base_fixture();
        let other = device(2);

        let mut renamed = base.clone();
        rename_vault(&mut renamed, owner, "Renamed");

        let mut recoloured = base.clone();
        recolour_vault(&mut recoloured, other, "#FF0000");

        for (left, right) in [(&renamed, &recoloured), (&recoloured, &renamed)] {
            let merged = expect_merged(merge_vaults(left, right));
            assert_eq!(merged.name, "Renamed", "the rename survives");
            assert_eq!(merged.color, "#FF0000", "the recolour survives");
        }
    }

    #[test]
    fn test_concurrent_vault_rename_is_a_conflict_with_each_writer_named() {
        let (base, _, owner) = base_fixture();
        let other = device(2);

        let mut left = base.clone();
        rename_vault(&mut left, owner, "Left name");

        let mut right = base.clone();
        rename_vault(&mut right, other, "Right name");

        let report = expect_conflicts(merge_vaults(&left, &right));
        assert_eq!(report.conflicts.len(), 1);

        let conflict = &report.conflicts[0];
        assert!(conflict.is_vault_metadata());
        assert_eq!(conflict.field_key, "name");
        assert!(!conflict.is_secret);
        let devices: BTreeSet<DeviceId> = conflict.sides.iter().map(|side| side.device).collect();
        assert_eq!(
            devices,
            BTreeSet::from([owner, other]),
            "each side must carry its own Device provenance"
        );
        let values: Vec<&str> = conflict
            .sides
            .iter()
            .filter_map(|side| side.value.as_ref())
            .filter_map(Value::as_str)
            .collect();
        assert!(values.contains(&"Left name"));
        assert!(values.contains(&"Right name"));

        // Canonical whichever way round the copies are passed.
        let reverse = expect_conflicts(merge_vaults(&right, &left));
        assert_eq!(
            serde_json::to_value(&report.conflicts).unwrap(),
            serde_json::to_value(&reverse.conflicts).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&report.merged).unwrap(),
            serde_json::to_value(&reverse.merged).unwrap()
        );
    }

    #[test]
    fn test_rename_after_a_sync_beats_the_old_name_on_both_sides() {
        let (base, _, owner) = base_fixture();
        let other = device(2);

        let mut first_rename = base.clone();
        rename_vault(&mut first_rename, owner, "First");

        // The other Device adopts the first rename, then renames again.
        let mut second_rename = first_rename.clone();
        rename_vault(&mut second_rename, other, "Second");

        let merged = expect_merged(merge_vaults(&first_rename, &second_rename));
        assert_eq!(
            merged.name, "Second",
            "a rename made after seeing the first must win"
        );
    }

    fn rename_vault(vault: &mut Vault, writer: DeviceId, name: &str) {
        let counter = vault.clock.increment(writer);
        vault.name = name.to_string();
        vault.name_clock = FieldVersion::new(writer, counter);
    }

    fn recolour_vault(vault: &mut Vault, writer: DeviceId, color: &str) {
        let counter = vault.clock.increment(writer);
        vault.color = color.to_string();
        vault.color_clock = FieldVersion::new(writer, counter);
    }

    #[test]
    fn test_kind_mismatch_conflict_names_the_device_that_wrote_each_kind() {
        let (base, item_id, owner) = base_fixture();
        let other = device(2);

        let left = base.clone();
        let mut right = base.clone();
        {
            let item = right
                .items
                .iter_mut()
                .find(|item| item.id == item_id)
                .unwrap();
            item.kind = Some(ItemKind::Card(CardData {
                cardholder_name: "Alex Doe".to_string(),
                expiry_month: 1,
                expiry_year: 2030,
                brand: None,
            }));
            item.clock = VersionVector::from_version(other, 1);
        }

        let report = expect_conflicts(merge_vaults(&left, &right));
        let conflict = &report.conflicts[0];
        assert_eq!(conflict.field_key, "kind");

        let devices: BTreeSet<DeviceId> = conflict.sides.iter().map(|side| side.device).collect();
        assert_eq!(
            devices,
            BTreeSet::from([owner, other]),
            "the prompt must be able to say which Device wrote which kind"
        );

        let values: Vec<&str> = conflict
            .sides
            .iter()
            .filter_map(|side| side.value.as_ref())
            .filter_map(Value::as_str)
            .collect();
        assert!(values.contains(&"account"));
        assert!(values.contains(&"card"));
    }

    #[test]
    fn test_merge_is_idempotent() {
        let (base, _, _) = base_fixture();

        let merged = expect_merged(merge_vaults(&base, &base));

        assert_eq!(
            serde_json::to_value(&base).unwrap(),
            serde_json::to_value(&merged).unwrap()
        );
    }

    #[test]
    fn test_merge_is_associative_for_clean_edits() {
        let (base, item_id, owner) = base_fixture();
        let second = device(2);
        let third = device(3);

        let mut first = base.clone();
        write_display_name(&mut first, item_id, owner, "Renamed");

        let mut second_copy = base.clone();
        write_password(&mut second_copy, item_id, second, "new-secret");

        let mut third_copy = base.clone();
        write_favourite(&mut third_copy, item_id, third, true);

        let left = {
            let ab = expect_merged(merge_vaults(&first, &second_copy));
            expect_merged(merge_vaults(&ab, &third_copy))
        };

        let right = {
            let bc = expect_merged(merge_vaults(&second_copy, &third_copy));
            expect_merged(merge_vaults(&first, &bc))
        };

        assert_eq!(
            serde_json::to_value(&left).unwrap(),
            serde_json::to_value(&right).unwrap()
        );
    }

    #[test]
    fn test_tombstone_beats_a_concurrent_live_copy() {
        let (base, item_id, owner) = base_fixture();
        let other = device(2);

        let mut deleted = base.clone();
        let clock = {
            let item = deleted
                .items
                .iter_mut()
                .find(|item| item.id == item_id)
                .unwrap();
            item.clock.increment(owner);
            item.clock.clone()
        };
        deleted.items = vec![VaultItem::tombstone(item_id, deleted.id, clock)];
        deleted.secrets.clear();

        let mut live = base.clone();
        write_password(&mut live, item_id, other, "zombie-secret");

        for (left, right) in [(&deleted, &live), (&live, &deleted)] {
            let merged = expect_merged(merge_vaults(left, right));
            let item = stored_item(&merged, item_id);
            assert!(item.is_deleted(), "deletion must win the merge");
            assert!(
                merged.secrets.iter().all(|entry| entry.item_id != item_id),
                "a tombstone carries no secrets"
            );
        }
    }

    #[test]
    fn test_merge_unions_duplicate_dismissals_from_both_devices() {
        let (base, _, _) = base_fixture();
        let alpha_first = Uuid::from_u128(0xDD01);
        let alpha_second = Uuid::from_u128(0xDD02);
        let beta_first = Uuid::from_u128(0xDD03);
        let beta_second = Uuid::from_u128(0xDD04);

        let mut left = base.clone();
        assert!(left.dismiss_duplicate(alpha_first, alpha_second));

        let mut right = base.clone();
        // The same decision recorded in reverse order is the same pair, so
        // recording it again in canonical order is a no-op.
        assert!(right.dismiss_duplicate(alpha_second, alpha_first));
        assert!(!right.dismiss_duplicate(alpha_first, alpha_second));
        assert!(right.dismiss_duplicate(beta_first, beta_second));

        for (a, b) in [(&left, &right), (&right, &left)] {
            let merged = expect_merged(merge_vaults(a, b));
            assert!(merged.is_duplicate_dismissed(alpha_first, alpha_second));
            assert!(merged.is_duplicate_dismissed(beta_first, beta_second));
            assert_eq!(
                merged.duplicate_dismissals.len(),
                2,
                "a dismissal recorded once must not multiply"
            );
        }
    }

    #[test]
    fn test_moved_item_keeps_identity_across_a_merge() {
        let (base, item_id, owner) = base_fixture();

        // A moved item lives in its target vault under the same UUID.
        let mut moved = base.clone();
        moved.id = Uuid::from_u128(0xCCCC);
        moved.items[0].vault_id = moved.id;

        let mut left = moved.clone();
        write_display_name(&mut left, item_id, owner, "Renamed");

        let mut right = moved.clone();
        write_password(&mut right, item_id, device(2), "new-secret");

        let merged = expect_merged(merge_vaults(&left, &right));
        assert_eq!(merged.id, moved.id);
        assert_eq!(stored_item(&merged, item_id).id, item_id);
    }

    fn setup() -> std::path::PathBuf {
        let temp_dir = std::env::temp_dir().join(format!("merge_test_{}", Uuid::new_v4()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        temp_dir
    }

    const MASTER_PW: &str = "SuperSecret123";
    const KEY: [u8; 32] = [0u8; 32];

    /// Two managers holding real encrypted copies of the same vault file.
    fn two_managers_with_the_same_vault() -> (VaultManager, VaultManager, VaultId, ItemId) {
        let first_dir = setup();
        let second_dir = setup();

        let mut first = VaultManager::new(first_dir.clone()).unwrap();
        let vault = first
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();
        let item = first
            .add_item(
                vault.id,
                Some("Octocat".to_string()),
                None,
                NewItemData::Account(AccountData {
                    username: "octocat".to_string(),
                    email: Some("octo@example.com".to_string()),
                }),
                Some(vec![SecretInput::new(
                    SECRET_FIELD_PASSWORD,
                    "hunter2".to_string(),
                )]),
            )
            .unwrap();
        let item_id = match item {
            crate::SafeItem::Account(account) => account.id,
            other => panic!("expected an account view, got {other:?}"),
        };

        first.lock_vault(Some(vault.id));

        let file =
            std::fs::read(first_dir.join("vaults").join(format!("{}.vault", vault.id))).unwrap();
        let second_vaults = second_dir.join("vaults");
        std::fs::create_dir_all(&second_vaults).unwrap();
        std::fs::write(second_vaults.join(format!("{}.vault", vault.id)), file).unwrap();

        let mut second = VaultManager::new(second_dir).unwrap();
        first.unlock_vault(vault.id, MASTER_PW, &KEY).unwrap();
        second.unlock_vault(vault.id, MASTER_PW, &KEY).unwrap();

        (first, second, vault.id, item_id)
    }

    #[test]
    fn test_two_managers_with_real_vault_files_merge_disjoint_edits() {
        let (mut first, mut second, vault_id, item_id) = two_managers_with_the_same_vault();

        first
            .update_item(
                vault_id,
                item_id,
                Some("Renamed".to_string()),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        second
            .update_item_secret(
                vault_id,
                item_id,
                SECRET_FIELD_PASSWORD,
                Some("new-secret".to_string()),
            )
            .unwrap();

        let first_copy = first.get_vault(vault_id).unwrap().clone();
        let second_copy = second.get_vault(vault_id).unwrap().clone();

        let merged = expect_merged(merge_vaults(&first_copy, &second_copy));

        assert_eq!(
            stored_item(&merged, item_id).display_name.as_deref(),
            Some("Renamed")
        );
        assert_eq!(
            merged
                .secrets
                .iter()
                .find(|entry| entry.item_id == item_id)
                .and_then(|entry| entry.get(SECRET_FIELD_PASSWORD)),
            Some("new-secret")
        );
    }
}
