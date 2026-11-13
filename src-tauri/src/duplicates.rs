//! Duplicate detection and Combine.
//!
//! A **Duplicate** is two Items the User considers the same thing despite
//! having been created separately, and so having different identities.
//! Detection only ever *suggests*: nothing here runs unless the User asks to
//! resolve a suggestion. **Combine** is the deliberate declaration that two
//! identities are one thing; it is never inferred from a name match.
//!
//! A Combine keeps one identity, unions tags, ORs favourite, and demands an
//! explicit pick for every field whose values differ - secrets included.
//! Fields that do not collide are carried over from the surviving identity.
//!
//! Dismissals (the User keeps both) are stored on the [`Vault`] so they travel
//! with vault content, and [`merge_vaults`](crate::merge_vaults) unions them.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::merge::{FieldValue, kind_tag, read_item_field, write_item_field};
use crate::{
    DeviceId, FieldVersion, ItemId, ItemKind, ItemSecrets, Vault, VaultError, VaultItem,
    allowed_secret_keys, item_field_keys,
};

/// One side of a suggested pair, as the UI needs to render it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DuplicateCandidate {
    pub item_id: ItemId,
    pub display_name: Option<String>,
}

/// A pair of Items that look like the same thing, ordered canonically by id.
///
/// Safe to send to the UI: secret values never appear in `colliding_fields`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DuplicateSuggestion {
    pub first: DuplicateCandidate,
    pub second: DuplicateCandidate,

    /// Present for account pairs, whose usernames also matched.
    pub username: Option<String>,

    /// Fields whose values differ and therefore need an explicit choice when
    /// Combining. Secret values are always withheld (`None`). `tags` and
    /// `favourite` are absent because they combine automatically.
    pub colliding_fields: Vec<DuplicateField>,
}

/// One field that Combine must ask the User about.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DuplicateField {
    pub field_key: String,
    pub is_secret: bool,

    /// The non-secret value from each side, as JSON. Always `None` for a
    /// secret field, even though the backend holds both values.
    pub first_value: Option<Value>,
    pub second_value: Option<Value>,
}

/// What the User decided about a suggested pair.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "resolution", rename_all = "snake_case")]
pub enum DuplicateResolution {
    /// Keep both Items and stop suggesting the pair.
    KeepBoth,

    /// Keep this identity and tombstone the other, without combining fields.
    Keep { item_id: ItemId },

    /// Keep one identity, carrying across the chosen values of both.
    ///
    /// `field_choices` must name the winning Item for every colliding field.
    /// The decision is explicit: there is no default for a secret.
    Combine {
        survivor: ItemId,
        field_choices: BTreeMap<String, ItemId>,
    },
}

/// What actually happened, for the caller and the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DuplicateOutcome {
    KeptBoth,
    Kept { item_id: ItemId },
    Combined { survivor: ItemId },
}

/// A display name reduced to the form used for comparison: trimmed,
/// whitespace-collapsed and case-insensitive.
pub fn normalise_name(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Every Duplicate suggestion in one vault. Suggestions are per-vault: two
/// Items with the same name in different vaults are never a pair.
pub(crate) fn suggest_duplicates(vault: &Vault) -> Vec<DuplicateSuggestion> {
    let mut suggestions = Vec::new();

    for (index, first) in vault.items.iter().enumerate() {
        if first.is_content_free() {
            continue;
        }

        for second in vault.items.iter().skip(index + 1) {
            if second.is_content_free() || vault.is_duplicate_dismissed(first.id, second.id) {
                continue;
            }

            if let Some(suggestion) = suggestion_for(vault, first, second) {
                suggestions.push(suggestion);
            }
        }
    }

    // Item order differs between Devices; the suggestion list must not.
    suggestions.sort_by_key(|suggestion| {
        (suggestion.first.item_id, suggestion.second.item_id)
    });
    suggestions
}

/// The pair rule: same kind, same normalised display name, and for accounts
/// the same username. A pair is only a suggestion - nothing is applied.
fn suggestion_for(
    vault: &Vault,
    first: &VaultItem,
    second: &VaultItem,
) -> Option<DuplicateSuggestion> {
    let (first, second) = if first.id <= second.id {
        (first, second)
    } else {
        (second, first)
    };

    let first_kind = first.kind.as_ref()?;
    let second_kind = second.kind.as_ref()?;
    if kind_tag(first_kind) != kind_tag(second_kind) {
        return None;
    }

    // An item with no name would otherwise pair with every other unnamed item
    // of the same kind, which is noise rather than a suggestion.
    let first_name = normalise_name(first.display_name.as_deref().unwrap_or_default());
    let second_name = normalise_name(second.display_name.as_deref().unwrap_or_default());
    if first_name.is_empty() || first_name != second_name {
        return None;
    }

    let username = match (first_kind, second_kind) {
        (ItemKind::Account(first_data), ItemKind::Account(second_data)) => {
            if first_data.username != second_data.username {
                return None;
            }
            Some(first_data.username.clone())
        }
        _ => None,
    };

    let secret_keys = allowed_secret_keys(first_kind);
    let colliding_fields = colliding_keys(vault, first, second, first_kind)
        .into_iter()
        .map(|key| {
            let is_secret = secret_keys.contains(&key);
            DuplicateField {
                field_key: key.to_string(),
                is_secret,
                first_value: if is_secret {
                    None
                } else {
                    read_item_field(first, key).and_then(|value| value.conflict_value())
                },
                second_value: if is_secret {
                    None
                } else {
                    read_item_field(second, key).and_then(|value| value.conflict_value())
                },
            }
        })
        .collect();

    Some(DuplicateSuggestion {
        first: candidate(first),
        second: candidate(second),
        username,
        colliding_fields,
    })
}

fn candidate(item: &VaultItem) -> DuplicateCandidate {
    DuplicateCandidate {
        item_id: item.id,
        display_name: item.display_name.clone(),
    }
}

/// Fields whose values differ between two same-kind Items. `tags` and
/// `favourite` are excluded because Combine unions and ORs them automatically.
fn colliding_keys(
    vault: &Vault,
    first: &VaultItem,
    second: &VaultItem,
    kind: &ItemKind,
) -> Vec<&'static str> {
    let secret_keys = allowed_secret_keys(kind);

    item_field_keys(kind)
        .into_iter()
        .chain(secret_keys.iter().copied())
        .filter(|key| *key != "tags" && *key != "favourite")
        .filter(|key| {
            let is_secret = secret_keys.contains(key);
            field_comparison(vault, first, key, is_secret)
                != field_comparison(vault, second, key, is_secret)
        })
        .collect()
}

/// Apply the User's explicit decision to a suggested pair.
///
/// Validation happens before any mutation, so a rejected resolution leaves
/// both Items exactly as they were.
pub(crate) fn resolve_duplicate(
    vault: &mut Vault,
    first: ItemId,
    second: ItemId,
    resolution: DuplicateResolution,
    device: DeviceId,
) -> Result<DuplicateOutcome, VaultError> {
    if first == second {
        return Err(VaultError::Validation(
            "A Duplicate pair needs two different Items".to_string(),
        ));
    }

    // Both identities must be live *before* anything changes.
    live_index(vault, first)?;
    live_index(vault, second)?;

    match resolution {
        DuplicateResolution::KeepBoth => {
            vault.dismiss_duplicate(first, second);
            Ok(DuplicateOutcome::KeptBoth)
        }
        DuplicateResolution::Keep { item_id } => {
            if item_id != first && item_id != second {
                return Err(VaultError::Validation(
                    "The surviving Item must be one of the suggested pair".to_string(),
                ));
            }

            let discarded = if item_id == first { second } else { first };
            tombstone_item(vault, discarded, device)?;
            Ok(DuplicateOutcome::Kept { item_id })
        }
        DuplicateResolution::Combine {
            survivor,
            field_choices,
        } => combine(
            vault,
            first,
            second,
            survivor,
            &field_choices,
            device,
        ),
    }
}

fn live_index(vault: &Vault, item_id: ItemId) -> Result<usize, VaultError> {
    vault
        .items
        .iter()
        .position(|item| item.id == item_id && !item.is_content_free())
        .ok_or_else(|| VaultError::ItemNotFound(item_id.to_string()))
}

/// Retire one identity in favour of the other, carrying no values across.
fn tombstone_item(
    vault: &mut Vault,
    item_id: ItemId,
    device: DeviceId,
) -> Result<(), VaultError> {
    let index = live_index(vault, item_id)?;
    let mut item = vault.items.remove(index);
    item.clock.increment(device);

    vault
        .items
        .push(VaultItem::tombstone(item.id, item.vault_id, item.clock));
    vault.secrets.retain(|entry| entry.item_id != item_id);
    Ok(())
}

/// Declare two same-kind identities to be one thing.
///
/// The survivor keeps its id. Tags are unioned, favourite is ORed, and every
/// field whose values differ must name a winning Item. A missing choice is a
/// rejection, not a default - especially for secrets.
fn combine(
    vault: &mut Vault,
    first: ItemId,
    second: ItemId,
    survivor: ItemId,
    field_choices: &BTreeMap<String, ItemId>,
    device: DeviceId,
) -> Result<DuplicateOutcome, VaultError> {
    if survivor != first && survivor != second {
        return Err(VaultError::Validation(
            "The surviving Item must be one of the suggested pair".to_string(),
        ));
    }

    let other = if survivor == first { second } else { first };
    let survivor_index = live_index(vault, survivor)?;
    let other_index = live_index(vault, other)?;

    // Everything below validates against these copies; the vault is only
    // touched once the whole Combine is known to be valid.
    let survivor_item = vault.items[survivor_index].clone();
    let other_item = vault.items[other_index].clone();
    let survivor_kind = survivor_item
        .kind
        .as_ref()
        .expect("a live item always carries a kind");
    let other_kind = other_item
        .kind
        .as_ref()
        .expect("a live item always carries a kind");

    if kind_tag(survivor_kind) != kind_tag(other_kind) {
        return Err(VaultError::Validation(
            "Only Items of the same kind can be combined".to_string(),
        ));
    }

    let field_keys = item_field_keys(survivor_kind);
    let secret_keys = allowed_secret_keys(survivor_kind);
    let choiceable: BTreeSet<&str> = field_keys
        .iter()
        .chain(secret_keys.iter())
        .copied()
        .collect();

    for (key, chosen) in field_choices {
        if key == "tags" || key == "favourite" {
            return Err(VaultError::Validation(format!(
                "'{key}' is combined automatically and must not be chosen"
            )));
        }
        if !choiceable.contains(key.as_str()) {
            return Err(VaultError::Validation(format!(
                "Unknown field '{key}' for this kind of Item"
            )));
        }
        if *chosen != survivor && *chosen != other {
            return Err(VaultError::Validation(format!(
                "The choice for '{key}' must be one of the two Items"
            )));
        }
    }

    // Every value that differs needs its owner named, secrets included.
    for key in colliding_keys(vault, &survivor_item, &other_item, survivor_kind) {
        if !field_choices.contains_key(key) {
            return Err(VaultError::Validation(format!(
                "An explicit choice is required for field '{key}'"
            )));
        }
    }

    let mut combined = survivor_item.clone();
    combined.tags = union_tags(&survivor_item.tags, &other_item.tags);
    combined.favourite = survivor_item.favourite || other_item.favourite;
    combined.updated_at = chrono::Utc::now();

    for (key, chosen) in field_choices {
        if secret_keys.contains(&key.as_str()) {
            continue;
        }

        let source = if *chosen == survivor {
            &survivor_item
        } else {
            &other_item
        };
        let value = read_item_field(source, key).ok_or_else(|| {
            VaultError::Validation(format!(
                "Field '{key}' is not part of this kind of Item"
            ))
        })?;
        if !write_item_field(&mut combined, key, &value) {
            return Err(VaultError::Validation(format!(
                "Field '{key}' cannot be written for this kind of Item"
            )));
        }
    }

    // Combine is a write by the resolving Device: one fresh tick for the
    // surviving Item, dominating both inputs.
    let mut clock = survivor_item.clock.clone();
    clock.merge(&other_item.clock);
    let tick = clock.increment(device);
    let version = FieldVersion::new(device, tick);
    combined.clock = clock;
    combined.stamp_all_fields(device, tick);

    let mut secret_fields = Vec::new();
    for key in secret_keys {
        let chosen = field_choices.get(*key).copied().unwrap_or(survivor);
        let value = if chosen == other {
            secret_value(vault, other, key)
        } else {
            secret_value(vault, survivor, key)
        };

        // Record the fresh tick even for an absent field, so a stale copy of
        // a removed secret cannot come back on a later merge.
        combined.record_field(key, version);
        if let Some(value) = value {
            secret_fields.push(crate::SecretField {
                key: (*key).to_string(),
                value,
                clock: version,
            });
        }
    }

    // The retired identity becomes a Tombstone, clocked past both inputs.
    let mut tombstone_clock = combined.clock.clone();
    tombstone_clock.merge(&other_item.clock);
    tombstone_clock.increment(device);
    let tombstone =
        VaultItem::tombstone(other_item.id, other_item.vault_id, tombstone_clock);

    // Apply atomically, from the caller's point of view.
    vault
        .secrets
        .retain(|entry| entry.item_id != survivor && entry.item_id != other);
    if !secret_fields.is_empty() {
        vault.secrets.push(ItemSecrets {
            item_id: survivor,
            fields: secret_fields,
        });
    }

    vault.items[survivor_index] = combined;
    let other_index = vault
        .items
        .iter()
        .position(|item| item.id == other)
        .expect("the loser was just validated");
    vault.items.remove(other_index);
    vault.items.push(tombstone);

    Ok(DuplicateOutcome::Combined { survivor })
}

fn secret_value(vault: &Vault, item_id: ItemId, key: &str) -> Option<String> {
    vault
        .secrets
        .iter()
        .find(|entry| entry.item_id == item_id)
        .and_then(|entry| entry.get(key))
        .map(str::to_string)
}

fn union_tags(survivor: &[String], other: &[String]) -> Vec<String> {
    let mut tags = survivor.to_vec();
    for tag in other {
        if !tags.contains(tag) {
            tags.push(tag.clone());
        }
    }
    tags
}

fn field_comparison(
    vault: &Vault,
    item: &VaultItem,
    key: &str,
    is_secret: bool,
) -> Option<FieldComparison> {
    if is_secret {
        Some(FieldComparison::Secret(secret_value(vault, item.id, key)))
    } else {
        read_item_field(item, key).map(FieldComparison::Value)
    }
}

#[derive(PartialEq)]
enum FieldComparison {
    Value(FieldValue),
    Secret(Option<String>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalise_name_collapses_whitespace_and_case() {
        assert_eq!(normalise_name("  Git   Hub "), "git hub");
        assert_eq!(normalise_name("GIT HUB"), "git hub");
    }
}
