//! The Device Registry: the shared, signed record of every Device admitted
//! to a User, and whether it has been revoked.
//!
//! There is no authority Device. Every Device may admit a new one or revoke
//! an existing one, and the registry reconciles as a set union keyed by
//! [`DeviceId`] with one asymmetric rule — **revocation wins, permanently**.
//! Admission is signed by the admitting Device; revocation is signed by the
//! Device that performed it. Both signatures are verified before an entry is
//! accepted, so no Device can forge an entry attributed to another.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use crate::device::{verify_signature, DeviceKeypair};
use crate::{DeviceId, VaultError, VaultId};

const ADMISSION_DOMAIN: &str = "lockr.device-admission.v1";
const REVOCATION_DOMAIN: &str = "lockr.device-revocation.v1";

/// One Device in the registry.
///
/// The admission fields are signed by `admitted_by`. Revocation is a later,
/// separate event signed by whichever Device performed it, so any honest
/// Device can revoke a lost one without being its admitter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceRecord {
    pub device_id: DeviceId,

    /// Base64 Ed25519 public key this Device signs with.
    pub public_key: String,

    /// The name announced at pairing. Display data, not identity.
    pub name: String,

    /// The platform announced at pairing.
    pub platform: String,

    pub admitted_by: DeviceId,
    pub admitted_at: DateTime<Utc>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<DateTime<Utc>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_by: Option<DeviceId>,

    /// Base64 signature over the admission fields, by `admitted_by`.
    #[serde(default)]
    pub signature: String,

    /// Base64 signature over the revocation fields, by `revoked_by`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revocation_signature: Option<String>,
}

impl DeviceRecord {
    /// Build and sign an admission. The caller passes the admitting Device's
    /// keypair; [`DeviceRegistry::admit`] verifies the result against the
    /// registry, so a wrong signer is rejected rather than trusted.
    pub fn admit(
        device_id: DeviceId,
        public_key: String,
        name: String,
        platform: String,
        admitted_by: DeviceId,
        admitted_at: DateTime<Utc>,
        signing_key: &DeviceKeypair,
    ) -> Self {
        let mut record = Self {
            device_id,
            public_key,
            name,
            platform,
            admitted_by,
            admitted_at,
            revoked_at: None,
            revoked_by: None,
            signature: String::new(),
            revocation_signature: None,
        };
        record.signature = signing_key.sign(&admission_payload(&record));
        record
    }

    /// Whether this Device has been barred from syncing.
    pub fn is_revoked(&self) -> bool {
        self.revoked_at.is_some()
    }

    fn admission_signature_is_present(&self) -> bool {
        !self.signature.is_empty()
    }
}

#[derive(Serialize)]
struct AdmissionPayload<'a> {
    domain: &'static str,
    device_id: &'a DeviceId,
    public_key: &'a str,
    name: &'a str,
    platform: &'a str,
    admitted_by: &'a DeviceId,
    admitted_at: String,
}

#[derive(Serialize)]
struct RevocationPayload<'a> {
    domain: &'static str,
    device_id: &'a DeviceId,
    revoked_at: String,
    revoked_by: &'a DeviceId,
}

/// A canonical byte form: fixed field order, fixed timestamp precision, and a
/// domain tag so an admission signature can never be replayed as a
/// revocation (or vice versa).
fn admission_payload(record: &DeviceRecord) -> Vec<u8> {
    serde_json::to_vec(&AdmissionPayload {
        domain: ADMISSION_DOMAIN,
        device_id: &record.device_id,
        public_key: &record.public_key,
        name: &record.name,
        platform: &record.platform,
        admitted_by: &record.admitted_by,
        admitted_at: record
            .admitted_at
            .to_rfc3339_opts(SecondsFormat::Nanos, true),
    })
    .expect("a fixed payload always serializes")
}

fn revocation_payload(
    device_id: &DeviceId,
    revoked_at: DateTime<Utc>,
    revoked_by: &DeviceId,
) -> Vec<u8> {
    serde_json::to_vec(&RevocationPayload {
        domain: REVOCATION_DOMAIN,
        device_id,
        revoked_at: revoked_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
        revoked_by,
    })
    .expect("a fixed payload always serializes")
}

/// The replicated Device Registry.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceRegistry {
    #[serde(default)]
    devices: BTreeMap<DeviceId, DeviceRecord>,
}

impl DeviceRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.devices.len()
    }

    pub fn is_empty(&self) -> bool {
        self.devices.is_empty()
    }

    pub fn get(&self, device_id: &DeviceId) -> Option<&DeviceRecord> {
        self.devices.get(device_id)
    }

    pub fn contains(&self, device_id: &DeviceId) -> bool {
        self.devices.contains_key(device_id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&DeviceId, &DeviceRecord)> {
        self.devices.iter()
    }

    /// Whether the registry currently bars this Device from syncing.
    pub fn is_revoked(&self, device_id: &DeviceId) -> bool {
        self.get(device_id).is_some_and(DeviceRecord::is_revoked)
    }

    /// Add a record after verifying its signatures. An unsigned or forged
    /// entry is rejected and leaves the registry untouched.
    ///
    /// Re-admitting the same entry (for example after a retry) is idempotent.
    /// A second, different admission for an existing Device id is refused:
    /// accepting it would let a peer substitute its own key for an honest
    /// Device's.
    pub fn admit(&mut self, record: DeviceRecord) -> Result<(), VaultError> {
        // The first Device in an otherwise empty registry is the root and is
        // the only one allowed to admit itself. Once a root exists, every
        // admission must be signed by an already-admitted Device.
        let root = if self.devices.is_empty() {
            (record.admitted_by == record.device_id).then_some(record.device_id)
        } else {
            self.root_device_id()?
        };
        self.verify_record_with_root(&record, root)?;

        if let Some(existing) = self.devices.get(&record.device_id) {
            let chosen = merge_records(existing, &record)?;
            self.devices.insert(record.device_id, chosen);
            return Ok(());
        }

        self.devices.insert(record.device_id, record);
        Ok(())
    }

    /// Verify every entry against the registry's own public keys.
    pub fn verify(&self) -> Result<(), VaultError> {
        if self.devices.is_empty() {
            return Ok(());
        }

        let root = self.root_device_id()?.ok_or_else(|| {
            VaultError::DeviceRegistry("registry has no self-admitted root Device".into())
        })?;

        for record in self.devices.values() {
            self.verify_record_with_root(record, Some(root))?;
        }

        // Every admission must be signed by a Device that is itself reachable
        // from the root. Presence alone is not enough: two Devices could admit
        // each other and form an island no admitted Device ever signed for.
        let mut admitted = BTreeSet::from([root]);
        loop {
            let before = admitted.len();
            for record in self.devices.values() {
                if admitted.contains(&record.device_id) {
                    continue;
                }
                if admitted.contains(&record.admitted_by) {
                    admitted.insert(record.device_id);
                }
            }
            if admitted.len() == before {
                break;
            }
        }

        if let Some(unreachable) = self
            .devices
            .keys()
            .find(|device_id| !admitted.contains(device_id))
        {
            return Err(VaultError::DeviceRegistry(format!(
                "Device {unreachable} was admitted outside the root Device's admission chain"
            )));
        }

        Ok(())
    }

    /// The one Device that admitted itself. Trust on first use covers this
    /// record and nothing else: a second self-admission would be a way for a
    /// peer to introduce a Device without any admitted Device's signature.
    fn root_device_id(&self) -> Result<Option<DeviceId>, VaultError> {
        let mut roots = self
            .devices
            .values()
            .filter(|record| record.admitted_by == record.device_id)
            .map(|record| record.device_id);

        match (roots.next(), roots.next()) {
            (None, _) => Ok(None),
            (Some(root), None) => Ok(Some(root)),
            (Some(_), Some(_)) => Err(VaultError::DeviceRegistry(
                "registry has more than one self-admitted Device".into(),
            )),
        }
    }

    /// Revoke a Device. Any admitted, unrevoked Device may do this, which is
    /// what makes losing the authority Device survivable.
    pub fn revoke(
        &mut self,
        device_id: &DeviceId,
        revoked_at: DateTime<Utc>,
        revoked_by: &DeviceId,
        signing_key: &DeviceKeypair,
    ) -> Result<(), VaultError> {
        if !self.contains(device_id) {
            return Err(VaultError::DeviceRegistry(format!(
                "cannot revoke unknown Device {device_id}"
            )));
        }
        if self.is_revoked(device_id) {
            // Revocation is permanent: a second one changes nothing.
            return Ok(());
        }

        let revoker = self.get(revoked_by).ok_or_else(|| {
            VaultError::DeviceRegistry(format!(
                "Device {revoked_by} is not in the registry and cannot revoke"
            ))
        })?;

        if revoker.is_revoked() {
            return Err(VaultError::DeviceRegistry(format!(
                "revoked Device {revoked_by} cannot revoke another Device"
            )));
        }

        if revoker.public_key != signing_key.public_key() {
            return Err(VaultError::DeviceRegistry(
                "revocation was not signed by the revoking Device".into(),
            ));
        }

        let mut updated = self
            .devices
            .get(device_id)
            .expect("presence checked above")
            .clone();
        updated.revoked_at = Some(revoked_at);
        updated.revoked_by = Some(*revoked_by);
        updated.revocation_signature =
            Some(signing_key.sign(&revocation_payload(device_id, revoked_at, revoked_by)));

        self.verify_record(&updated)?;
        self.devices.insert(*device_id, updated);
        Ok(())
    }

    /// Set union by Device id. Revocation wins unconditionally; an older
    /// copy that still shows a Device as admitted cannot undo it.
    ///
    /// The merged result is verified as a whole, so a peer cannot smuggle in
    /// a forged entry alongside an honest one.
    pub fn merge(&self, other: &Self) -> Result<Self, VaultError> {
        let mut merged = self.clone();

        for (device_id, incoming) in &other.devices {
            let chosen = match merged.devices.get(device_id) {
                None => incoming.clone(),
                Some(existing) => merge_records(existing, incoming)?,
            };
            merged.devices.insert(*device_id, chosen);
        }

        merged.verify()?;
        Ok(merged)
    }

    fn verify_record(&self, record: &DeviceRecord) -> Result<(), VaultError> {
        let root = self.root_device_id()?;
        self.verify_record_with_root(record, root)
    }

    fn verify_record_with_root(
        &self,
        record: &DeviceRecord,
        root: Option<DeviceId>,
    ) -> Result<(), VaultError> {
        if !record.admission_signature_is_present() {
            return Err(VaultError::DeviceRegistry(format!(
                "Device {} has no admission signature",
                record.device_id
            )));
        }

        let admitting_key = if record.admitted_by == record.device_id {
            // The first Device admits itself. This is trust on first use for
            // the root record only: any other self-admission would let a peer
            // introduce a Device no admitted Device ever signed for.
            if root != Some(record.device_id) {
                return Err(VaultError::DeviceRegistry(format!(
                    "Device {} may not admit itself; only the root Device may",
                    record.device_id
                )));
            }
            &record.public_key
        } else {
            let admitter = self.devices.get(&record.admitted_by).ok_or_else(|| {
                VaultError::DeviceRegistry(format!(
                    "Device {} was admitted by unknown Device {}",
                    record.device_id, record.admitted_by
                ))
            })?;
            &admitter.public_key
        };

        verify_signature(admitting_key, &admission_payload(record), &record.signature)?;

        match (
            record.revoked_at,
            record.revoked_by.as_ref(),
            record.revocation_signature.as_deref(),
        ) {
            (None, None, None) => Ok(()),
            (Some(revoked_at), Some(revoked_by), Some(signature)) => {
                let revoker = self.devices.get(revoked_by).ok_or_else(|| {
                    VaultError::DeviceRegistry(format!(
                        "Device {} was revoked by unknown Device {}",
                        record.device_id, revoked_by
                    ))
                })?;
                if revoker.is_revoked() {
                    // A Device that has itself been revoked may still hold
                    // its signing key and try to bar honest Devices. The
                    // check belongs here, at verification, because a peer
                    // can inject a revocation this Device produced earlier.
                    return Err(VaultError::DeviceRegistry(format!(
                        "Device {} was revoked by revoked Device {}",
                        record.device_id, revoked_by
                    )));
                }
                verify_signature(
                    &revoker.public_key,
                    &revocation_payload(&record.device_id, revoked_at, revoked_by),
                    signature,
                )
            }
            _ => Err(VaultError::DeviceRegistry(format!(
                "Device {} has an incomplete revocation",
                record.device_id
            ))),
        }
    }
}

fn same_admission(first: &DeviceRecord, second: &DeviceRecord) -> bool {
    first.device_id == second.device_id
        && first.public_key == second.public_key
        && first.name == second.name
        && first.platform == second.platform
        && first.admitted_by == second.admitted_by
        && first.admitted_at == second.admitted_at
        && first.signature == second.signature
}

/// Deterministic per-id resolution. If both copies are revoked, keep the
/// earlier revocation; otherwise revocation wins outright.
///
/// Two different admissions for one Device id are a collision, not something
/// to resolve: one of them is forged, and picking either would let a peer
/// substitute a key. Refusing the merge is the only safe answer.
fn merge_records(first: &DeviceRecord, second: &DeviceRecord) -> Result<DeviceRecord, VaultError> {
    if !same_admission(first, second) {
        return Err(VaultError::DeviceRegistry(format!(
            "Device {} has two different admission records",
            first.device_id
        )));
    }

    Ok(match (first.revoked_at, second.revoked_at) {
        (Some(first_at), Some(second_at)) => {
            let order = (
                first_at,
                first.revoked_by,
                first.revocation_signature.as_deref(),
            )
                .cmp(&(
                    second_at,
                    second.revoked_by,
                    second.revocation_signature.as_deref(),
                ));
            if order == std::cmp::Ordering::Greater {
                second.clone()
            } else {
                first.clone()
            }
        }
        (Some(_), None) => first.clone(),
        (None, Some(_)) => second.clone(),
        (None, None) => first.clone(),
    })
}

/// The shared record of which Vaults belong to a User. It carries identities
/// only — a Device that does not hold a Vault cannot even name it, because
/// names live inside the encrypted file.
///
/// This replaces the profile's stored `vault_ids`: the list is a view over
/// this registry, never a second source of truth.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultRegistry {
    #[serde(default)]
    vault_ids: BTreeSet<VaultId>,
}

impl VaultRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.vault_ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.vault_ids.is_empty()
    }

    pub fn contains(&self, vault_id: &VaultId) -> bool {
        self.vault_ids.contains(vault_id)
    }

    /// Record a Vault's identity. Returns whether it was newly added.
    pub fn insert(&mut self, vault_id: VaultId) -> bool {
        self.vault_ids.insert(vault_id)
    }

    /// Forget a Vault's identity. Returns whether it was present.
    pub fn remove(&mut self, vault_id: &VaultId) -> bool {
        self.vault_ids.remove(vault_id)
    }

    /// The derived `vault_ids` view, in stable order.
    pub fn ids(&self) -> Vec<VaultId> {
        self.vault_ids.iter().copied().collect()
    }

    pub fn iter(&self) -> impl Iterator<Item = &VaultId> {
        self.vault_ids.iter()
    }
}

/// The bytes that travel as [`crate::RegistryState`]: both registries, and
/// nothing else. In particular this carries no vault content, no secrets, and
/// no User metadata — every Device's registry is the same list of identities.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedRegistries {
    #[serde(default)]
    pub devices: DeviceRegistry,
    #[serde(default)]
    pub vaults: VaultRegistry,
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).expect("valid timestamp")
    }

    fn record(
        device_id: DeviceId,
        public_key: String,
        admitted_by: DeviceId,
        admitted_at: DateTime<Utc>,
        signer: &DeviceKeypair,
    ) -> DeviceRecord {
        DeviceRecord::admit(
            device_id,
            public_key,
            "Studio".to_string(),
            "windows".to_string(),
            admitted_by,
            admitted_at,
            signer,
        )
    }

    /// A registry holding one self-admitted root Device.
    fn root_registry(device_id: DeviceId, keypair: &DeviceKeypair) -> DeviceRegistry {
        let mut registry = DeviceRegistry::new();
        registry
            .admit(record(
                device_id,
                keypair.public_key(),
                device_id,
                at(1_000),
                keypair,
            ))
            .unwrap();
        registry
    }

    #[test]
    fn test_admission_is_signed_and_verifiable() {
        let root = Uuid::from_u128(1);
        let keypair = DeviceKeypair::generate();

        let registry = root_registry(root, &keypair);

        assert_eq!(registry.len(), 1);
        registry.verify().unwrap();
        let record = registry.get(&root).unwrap();
        assert_eq!(record.public_key, keypair.public_key());
        assert_eq!(record.admitted_by, root);
        assert!(!record.signature.is_empty());
    }

    #[test]
    fn test_an_unsigned_entry_is_rejected() {
        let root = Uuid::from_u128(1);
        let keypair = DeviceKeypair::generate();
        let mut registry = root_registry(root, &keypair);

        let mut unsigned = record(root, keypair.public_key(), root, at(2_000), &keypair);
        unsigned.signature.clear();

        let result = registry.admit(unsigned);

        assert!(matches!(result, Err(VaultError::DeviceRegistry(_))));
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn test_a_tampered_entry_is_rejected() {
        let root = Uuid::from_u128(1);
        let keypair = DeviceKeypair::generate();
        let mut registry = root_registry(root, &keypair);
        let mut forged = registry.get(&root).unwrap().clone();
        forged.name = "Attacker's Laptop".to_string();

        let result = registry.admit(forged);

        assert!(matches!(result, Err(VaultError::DeviceRegistry(_))));
        assert_eq!(
            registry.get(&root).unwrap().name,
            "Studio",
            "the rejected entry must not overwrite the honest one"
        );
    }

    #[test]
    fn test_an_entry_signed_by_the_wrong_device_is_rejected() {
        let root = Uuid::from_u128(1);
        let joiner = Uuid::from_u128(2);
        let root_key = DeviceKeypair::generate();
        let joiner_key = DeviceKeypair::generate();
        let mut registry = root_registry(root, &root_key);

        // Claims to have been admitted by the root, but signed by the joiner.
        let forged = record(
            joiner,
            joiner_key.public_key(),
            root,
            at(2_000),
            &joiner_key,
        );

        let result = registry.admit(forged);

        assert!(matches!(result, Err(VaultError::DeviceRegistry(_))));
        assert!(!registry.contains(&joiner));
    }

    #[test]
    fn test_merge_is_set_union_by_device_id() {
        let root = Uuid::from_u128(1);
        let joiner = Uuid::from_u128(2);
        let root_key = DeviceKeypair::generate();
        let joiner_key = DeviceKeypair::generate();

        let mut first = root_registry(root, &root_key);
        let second = root_registry(root, &root_key);
        first
            .admit(record(
                joiner,
                joiner_key.public_key(),
                root,
                at(2_000),
                &root_key,
            ))
            .unwrap();

        let merged = first.merge(&second).unwrap();
        assert_eq!(merged.len(), 2);
        assert!(merged.contains(&joiner));

        let reversed = second.merge(&first).unwrap();
        assert_eq!(merged, reversed, "merge must be commutative");
        merged.verify().unwrap();
    }

    #[test]
    fn test_revocation_wins_and_cannot_be_undone_by_an_older_copy() {
        let root = Uuid::from_u128(1);
        let joiner = Uuid::from_u128(2);
        let root_key = DeviceKeypair::generate();
        let joiner_key = DeviceKeypair::generate();

        let mut admitted = root_registry(root, &root_key);
        admitted
            .admit(record(
                joiner,
                joiner_key.public_key(),
                root,
                at(2_000),
                &root_key,
            ))
            .unwrap();

        let mut revoked = admitted.clone();
        revoked
            .revoke(&joiner, at(3_000), &root, &root_key)
            .unwrap();

        let merged = admitted.merge(&revoked).unwrap();
        assert!(merged.is_revoked(&joiner));

        let older_copy = revoked.merge(&admitted).unwrap();
        assert!(
            older_copy.is_revoked(&joiner),
            "an older registry that still shows the Device as admitted must not undo revocation"
        );

        // Revocation is permanent even when the revoked Device's own copy
        // insists it is still admitted.
        let mut from_the_revoked_device = admitted.clone();
        from_the_revoked_device
            .revoke(&joiner, at(3_000), &root, &root_key)
            .unwrap();
        assert!(from_the_revoked_device.is_revoked(&joiner));
    }

    #[test]
    fn test_any_admitted_device_can_revoke_a_lost_one() {
        let root = Uuid::from_u128(1);
        let laptop = Uuid::from_u128(2);
        let phone = Uuid::from_u128(3);
        let root_key = DeviceKeypair::generate();
        let laptop_key = DeviceKeypair::generate();
        let phone_key = DeviceKeypair::generate();

        let mut registry = root_registry(root, &root_key);
        for (id, key) in [(laptop, &laptop_key), (phone, &phone_key)] {
            registry
                .admit(record(id, key.public_key(), root, at(2_000), &root_key))
                .unwrap();
        }

        // The phone revokes the lost laptop, not the laptop's admitter.
        registry
            .revoke(&laptop, at(3_000), &phone, &phone_key)
            .unwrap();

        assert!(registry.is_revoked(&laptop));
        assert!(!registry.is_revoked(&phone));
        assert_eq!(registry.get(&laptop).unwrap().revoked_by, Some(phone));
        registry.verify().unwrap();
    }

    #[test]
    fn test_a_revoked_device_cannot_revoke_an_honest_one() {
        let root = Uuid::from_u128(1);
        let lost = Uuid::from_u128(2);
        let root_key = DeviceKeypair::generate();
        let lost_key = DeviceKeypair::generate();

        let mut registry = root_registry(root, &root_key);
        registry
            .admit(record(
                lost,
                lost_key.public_key(),
                root,
                at(2_000),
                &root_key,
            ))
            .unwrap();
        registry.revoke(&lost, at(3_000), &root, &root_key).unwrap();

        // The revoked Device still holds its signing key. It signs a
        // revocation of the honest root and a peer forwards it.
        let mut forged = registry.get(&root).unwrap().clone();
        forged.revoked_at = Some(at(4_000));
        forged.revoked_by = Some(lost);
        forged.revocation_signature =
            Some(lost_key.sign(&revocation_payload(&root, at(4_000), &lost)));

        let mut forged_registry = registry.clone();
        forged_registry.devices.insert(root, forged.clone());

        assert!(
            matches!(forged_registry.verify(), Err(VaultError::DeviceRegistry(_))),
            "verification must reject a revocation signed by a revoked Device"
        );
        assert!(matches!(
            registry.admit(forged),
            Err(VaultError::DeviceRegistry(_))
        ));
        assert!(
            !registry.is_revoked(&root),
            "the rejected revocation must leave the honest Device unrevoked"
        );
    }

    #[test]
    fn test_only_the_root_device_may_admit_itself() {
        let root = Uuid::from_u128(1);
        let intruder = Uuid::from_u128(2);
        let root_key = DeviceKeypair::generate();
        let intruder_key = DeviceKeypair::generate();

        let registry = root_registry(root, &root_key);
        let self_admitted = record(
            intruder,
            intruder_key.public_key(),
            intruder,
            at(2_000),
            &intruder_key,
        );

        // A peer cannot introduce a brand-new Device with only its own
        // signature; pairing by an admitted Device is the only way in.
        let mut target = registry.clone();
        assert!(matches!(
            target.admit(self_admitted.clone()),
            Err(VaultError::DeviceRegistry(_))
        ));
        assert!(!target.contains(&intruder));

        // The same rule holds when the forged registry arrives whole.
        let mut forged = registry;
        forged.devices.insert(intruder, self_admitted);
        assert!(
            matches!(forged.verify(), Err(VaultError::DeviceRegistry(_))),
            "a registry with a second self-admitted Device must not verify"
        );
    }

    #[test]
    fn test_an_admission_must_chain_to_the_root_device() {
        let root = Uuid::from_u128(1);
        let first = Uuid::from_u128(2);
        let second = Uuid::from_u128(3);
        let root_key = DeviceKeypair::generate();
        let first_key = DeviceKeypair::generate();
        let second_key = DeviceKeypair::generate();

        let mut registry = root_registry(root, &root_key);

        // Neither Device is reachable from the root: they vouch for each
        // other, which would otherwise smuggle both in without the root's
        // (or any admitted Device's) signature.
        registry.devices.insert(
            first,
            record(
                first,
                first_key.public_key(),
                second,
                at(2_000),
                &second_key,
            ),
        );
        registry.devices.insert(
            second,
            record(
                second,
                second_key.public_key(),
                first,
                at(3_000),
                &first_key,
            ),
        );

        assert!(
            matches!(registry.verify(), Err(VaultError::DeviceRegistry(_))),
            "an admission island must not verify"
        );
    }

    #[test]
    fn test_an_admitted_device_can_still_admit_a_new_one() {
        let root = Uuid::from_u128(1);
        let laptop = Uuid::from_u128(2);
        let phone = Uuid::from_u128(3);
        let root_key = DeviceKeypair::generate();
        let laptop_key = DeviceKeypair::generate();
        let phone_key = DeviceKeypair::generate();

        let mut registry = root_registry(root, &root_key);
        registry
            .admit(record(
                laptop,
                laptop_key.public_key(),
                root,
                at(2_000),
                &root_key,
            ))
            .unwrap();

        // The laptop, not the root, admits the phone.
        registry
            .admit(record(
                phone,
                phone_key.public_key(),
                laptop,
                at(3_000),
                &laptop_key,
            ))
            .unwrap();

        registry.verify().unwrap();
    }

    #[test]
    fn test_revocation_must_be_signed_by_the_revoking_device() {
        let root = Uuid::from_u128(1);
        let laptop = Uuid::from_u128(2);
        let root_key = DeviceKeypair::generate();
        let laptop_key = DeviceKeypair::generate();
        let attacker_key = DeviceKeypair::generate();

        let mut registry = root_registry(root, &root_key);
        registry
            .admit(record(
                laptop,
                laptop_key.public_key(),
                root,
                at(2_000),
                &root_key,
            ))
            .unwrap();

        let result = registry.revoke(&laptop, at(3_000), &root, &attacker_key);

        assert!(matches!(result, Err(VaultError::DeviceRegistry(_))));
        assert!(!registry.is_revoked(&laptop));
    }

    #[test]
    fn test_a_revoked_device_stays_in_the_registry_and_a_reinstall_is_a_new_device() {
        let root = Uuid::from_u128(1);
        let old_install = Uuid::from_u128(2);
        let new_install = Uuid::from_u128(3);
        let root_key = DeviceKeypair::generate();
        let old_key = DeviceKeypair::generate();
        let new_key = DeviceKeypair::generate();

        let mut registry = root_registry(root, &root_key);
        registry
            .admit(record(
                old_install,
                old_key.public_key(),
                root,
                at(2_000),
                &root_key,
            ))
            .unwrap();
        registry
            .revoke(&old_install, at(3_000), &root, &root_key)
            .unwrap();

        registry
            .admit(record(
                new_install,
                new_key.public_key(),
                root,
                at(4_000),
                &root_key,
            ))
            .unwrap();

        assert!(registry.is_revoked(&old_install));
        assert!(registry.contains(&old_install));
        assert!(!registry.is_revoked(&new_install));
        assert_ne!(old_install, new_install);
    }

    #[test]
    fn test_readmitting_the_same_entry_is_idempotent_but_a_second_admission_is_refused() {
        let root = Uuid::from_u128(1);
        let root_key = DeviceKeypair::generate();
        let attacker_key = DeviceKeypair::generate();

        let mut registry = root_registry(root, &root_key);
        let existing = registry.get(&root).unwrap().clone();
        registry.admit(existing.clone()).unwrap();
        assert_eq!(registry.len(), 1);

        let attacker_admission = record(
            root,
            attacker_key.public_key(),
            root,
            at(9_000),
            &attacker_key,
        );
        let result = registry.admit(attacker_admission);

        assert!(matches!(result, Err(VaultError::DeviceRegistry(_))));
        assert_eq!(
            registry.get(&root).unwrap().public_key,
            root_key.public_key()
        );
    }

    #[test]
    fn test_merge_rejects_a_second_admission_that_substitutes_a_devices_key() {
        let root = Uuid::from_u128(1);
        let laptop = Uuid::from_u128(2);
        let root_key = DeviceKeypair::generate();
        let honest_key = DeviceKeypair::generate();
        let attacker_key = DeviceKeypair::generate();

        let mut honest = root_registry(root, &root_key);
        honest
            .admit(record(
                laptop,
                honest_key.public_key(),
                root,
                at(2_000),
                &root_key,
            ))
            .unwrap();

        let mut forged = root_registry(root, &root_key);
        forged.devices.insert(
            laptop,
            record(
                laptop,
                attacker_key.public_key(),
                laptop,
                at(9_000),
                &attacker_key,
            ),
        );

        // A self-admitted Device is only valid as the root. Once an honest
        // root exists, this copy is already rejected by verification; the
        // merge additionally refuses the identity collision.
        assert!(matches!(
            forged.verify(),
            Err(VaultError::DeviceRegistry(_))
        ));

        let result = honest.merge(&forged);

        assert!(matches!(result, Err(VaultError::DeviceRegistry(_))));
    }

    #[test]
    fn test_merge_rejects_a_forged_entry_hiding_behind_an_honest_one() {
        let root = Uuid::from_u128(1);
        let joiner = Uuid::from_u128(2);
        let root_key = DeviceKeypair::generate();
        let joiner_key = DeviceKeypair::generate();
        let attacker_key = DeviceKeypair::generate();

        let honest = root_registry(root, &root_key);
        let mut forged = root_registry(root, &root_key);
        forged.devices.insert(
            joiner,
            record(
                joiner,
                joiner_key.public_key(),
                root,
                at(2_000),
                &attacker_key,
            ),
        );

        let result = honest.merge(&forged);

        assert!(matches!(result, Err(VaultError::DeviceRegistry(_))));
    }

    #[test]
    fn test_vault_registry_is_an_identity_only_membership_set() {
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let mut registry = VaultRegistry::new();

        assert!(registry.is_empty());
        assert!(registry.insert(first));
        assert!(!registry.insert(first), "inserting twice changes nothing");
        assert!(registry.insert(second));
        assert_eq!(registry.len(), 2);
        assert_eq!(registry.ids(), {
            let mut expected = vec![first, second];
            expected.sort();
            expected
        });

        let json = serde_json::to_value(&registry).unwrap();
        assert_eq!(
            json.as_object().unwrap().keys().collect::<Vec<_>>(),
            vec!["vault_ids"],
            "the Vault Registry carries identities only"
        );

        assert!(registry.remove(&first));
        assert!(!registry.remove(&first));
        assert!(!registry.contains(&first));
    }

    #[test]
    fn test_the_registry_roundtrips_as_shared_state() {
        let root = Uuid::from_u128(1);
        let joiner = Uuid::from_u128(2);
        let root_key = DeviceKeypair::generate();
        let joiner_key = DeviceKeypair::generate();

        let mut registry = root_registry(root, &root_key);
        registry
            .admit(record(
                joiner,
                joiner_key.public_key(),
                root,
                at(2_000),
                &root_key,
            ))
            .unwrap();
        registry
            .revoke(&joiner, at(3_000), &root, &root_key)
            .unwrap();

        let bytes = serde_json::to_vec(&registry).unwrap();
        let parsed: DeviceRegistry = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(parsed, registry);
        parsed.verify().unwrap();
    }
}
