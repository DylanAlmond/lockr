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
//! Conflict payloads follow the Safe View Pattern: they name the Item, the
//! field key, and both writers, but a secret field's value is always withheld.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::Value;

use crate::{
    allowed_secret_keys, item_field_keys, ClockOrder, DeviceId, FieldVersion, ItemId, ItemKind,
    ItemSecrets, SecretField, Vault, VaultId, VaultItem, VersionVector,
};

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

    /// One entry per contested field, ordered by Item and field key.
    pub conflicts: Vec<Conflict>,
}

/// A field that changed concurrently on two Devices.
///
/// Safe to send to the UI: secret values are never present in `value`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Conflict {
    pub item_id: ItemId,

    /// Display name of the Item, for the prompt. It is a safe view field.
    pub item_display_name: Option<String>,

    /// The wire field key, e.g. `password` or `display_name`.
    pub field_key: String,

    /// True when the contested field is a secret. In that case both sides'
    /// `value` is `None`, even though the backend holds both values.
    pub is_secret: bool,

    /// Both writers, ordered canonically so either Device reports the same
    /// conflict.
    pub sides: [ConflictSide; 2],
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

/// A field value as it exists in one vault copy.
#[derive(Debug, Clone, PartialEq)]
enum FieldValue {
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
    fn conflict_value(&self) -> Option<Value> {
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

/// One Item reconciled with its optional secret entry.
struct MergedItem {
    item: VaultItem,
    secrets: Option<ItemSecrets>,
    conflicts: Vec<Conflict>,
}

/// Merge two copies of the same vault.
///
/// The function is commutative and associative for clean merges, and reports
/// the same conflict set whichever way round its arguments are passed.
pub fn merge_vaults(left: &Vault, right: &Vault) -> MergeOutcome {
    // The common idempotent case: hand back the input untouched, preserving
    // its item order exactly.
    if vaults_are_identical(left, right) {
        return MergeOutcome::Merged(left.clone());
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

    // Vault metadata has no clock in schema v3, so choose a whole label pair
    // deterministically rather than mixing the two sides.
    let (name, color) = if (&left.name, &left.color) <= (&right.name, &right.color) {
        (left.name.clone(), left.color.clone())
    } else {
        (right.name.clone(), right.color.clone())
    };

    let vault_id = if left.id == right.id {
        left.id
    } else {
        left.id.min(right.id)
    };

    let merged = Vault {
        schema_version: left.schema_version.max(right.schema_version),
        id: vault_id,
        name,
        color,
        items,
        secrets,
    };

    if conflicts.is_empty() {
        MergeOutcome::Merged(merged)
    } else {
        MergeOutcome::Conflicts(ConflictReport { merged, conflicts })
    }
}

fn vaults_are_identical(left: &Vault, right: &Vault) -> bool {
    match (serde_json::to_vec(left), serde_json::to_vec(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
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

    // Deletion is permanent for an identity. A Tombstone wins even against a
    // concurrent live edit, whose clock is absorbed so the next merge does not
    // re-litigate the deletion.
    if left.is_tombstone() || right.is_tombstone() {
        let mut clock = left.clock.clone();
        clock.merge(&right.clock);
        let vault_id = if left.vault_id == right.vault_id {
            left.vault_id
        } else {
            left.vault_id.min(right.vault_id)
        };
        return MergedItem {
            item: VaultItem::tombstone(left.id, vault_id, clock),
            secrets: None,
            conflicts: Vec::new(),
        };
    }

    match left.clock.compare(&right.clock) {
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

        let version = latest_clock_version(&clock);
        let mut sides = [
            ConflictSide {
                device: version.device,
                counter: version.counter,
                value: Some(Value::String(kind_tag(left_kind).to_string())),
            },
            ConflictSide {
                device: version.device,
                counter: version.counter,
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
                item_id: left.id,
                item_display_name: display_name,
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
                item_id: left.id,
                item_display_name: display_name.clone(),
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

fn kind_tag(kind: &ItemKind) -> &'static str {
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

fn read_item_field(item: &VaultItem, key: &str) -> Option<FieldValue> {
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

fn write_item_field(item: &mut VaultItem, key: &str, value: &FieldValue) -> bool {
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
        AccountData, NewItemData, SecretInput, CURRENT_SCHEMA_VERSION, SECRET_FIELD_PASSWORD,
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
            items: vec![item],
            secrets: vec![ItemSecrets::new(
                item_id,
                SECRET_FIELD_PASSWORD,
                "hunter2".to_string(),
                FieldVersion::new(owner, 1),
            )],
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

    fn expect_merged(outcome: MergeOutcome) -> Vault {
        match outcome {
            MergeOutcome::Merged(vault) => vault,
            MergeOutcome::Conflicts(report) => {
                panic!("expected a clean merge, got {:?}", report.conflicts)
            }
        }
    }

    fn expect_conflicts(outcome: MergeOutcome) -> ConflictReport {
        match outcome {
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
        assert_eq!(conflict.item_id, item_id);
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
            assert!(item.is_tombstone(), "deletion must win the merge");
            assert!(
                merged.secrets.iter().all(|entry| entry.item_id != item_id),
                "a tombstone carries no secrets"
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
