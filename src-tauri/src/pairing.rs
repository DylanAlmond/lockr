//! Pairing: admitting a new Device to a User.
//!
//! Pairing is the only grant of access (ADR-0002 §3). The source Device mints
//! a single-use 128-bit [`PairingSecret`], shows it as a QR payload with an
//! 8-character typed fallback, and keeps it alive for
//! [`PAIRING_TTL_SECS`] with a small attempt budget. The secret authenticates
//! the channel; it is never part of a message body.
//!
//! The human on the source Device approves the joining Device by the name and
//! platform it announced **before** any key material moves. Only after that
//! does the source send a [`PairingTransfer`] holding the User id, the shared
//! profile metadata, and the 32-byte secret key. The master password is never
//! part of any pairing type: the joining Device re-wraps the secret key under
//! the password typed locally.

use std::fmt;

use argon2::password_hash::rand_core::{OsRng, RngCore};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroize;

use crate::models::SharedProfile;
use crate::{DeviceId, UserId, VaultError, VaultId};

/// How long a pairing offer is usable. Short by design: a code glimpsed over
/// a shoulder is worthless shortly after it is shown.
pub const PAIRING_TTL_SECS: i64 = 60;

/// Characters in the typed fallback, excluding the display hyphen.
pub const PAIRING_CODE_LEN: usize = 8;

/// How many join attempts one offer tolerates before it is burned.
pub const PAIRING_MAX_ATTEMPTS: u32 = 5;

/// Minimum spacing between join attempts, as a crude rate limit on top of the
/// attempt budget.
pub const PAIRING_RETRY_INTERVAL_SECS: i64 = 1;

const QR_PREFIX: &str = "lockr-pair";
const QR_VERSION: &str = "v1";
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// A single-use 128-bit pairing secret.
///
/// Only the QR payload carries all 128 bits. The typed fallback carries the
/// first 40 bits, which is why it is rate-limited and short-lived; the ADR
/// fixes this trade-off.
#[derive(Clone)]
pub struct PairingSecret([u8; 16]);

impl PairingSecret {
    /// Draw a fresh secret from the operating system's randomness.
    pub fn generate() -> Self {
        let mut bytes = [0u8; 16];
        OsRng.fill_bytes(&mut bytes);
        Self(bytes)
    }

    fn from_slice(bytes: &[u8]) -> Result<Self, VaultError> {
        let bytes: [u8; 16] = bytes
            .try_into()
            .map_err(|_| VaultError::Pairing("pairing secret has the wrong length".into()))?;
        Ok(Self(bytes))
    }

    /// The 8-character Crockford Base32 fallback, derived from the first five
    /// bytes of the secret.
    pub fn typed_code(&self) -> String {
        let mut code = String::with_capacity(PAIRING_CODE_LEN + 1);
        let mut buffer: u64 = 0;
        let mut bits = 0;
        for byte in &self.0[..5] {
            buffer = (buffer << 8) | u64::from(*byte);
            bits += 8;
        }
        // Align the 40 bits to the left so the 8 five-bit groups are the code.
        let buffer = buffer << (40 - bits);
        for index in 0..PAIRING_CODE_LEN {
            let shift = 35 - index * 5;
            let value = ((buffer >> shift) & 0b11111) as usize;
            if index == 4 {
                code.push('-');
            }
            code.push(CROCKFORD[value] as char);
        }
        code
    }

    /// The URL-safe, unpadded Base64 form the QR payload carries.
    fn qr_token(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.0)
    }

    fn constant_time_eq(&self, other: &Self) -> bool {
        self.0
            .iter()
            .zip(other.0.iter())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
    }
}

impl Drop for PairingSecret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl fmt::Debug for PairingSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PairingSecret(<redacted>)")
    }
}

/// Decode the typed fallback, accepting the Crockford aliases (`I`/`L` for 1,
/// `O` for 0) and ignoring the display hyphen.
fn typed_prefix(code: &str) -> Result<[u8; 5], VaultError> {
    let normalised: String = code
        .chars()
        .filter(|character| *character != '-')
        .map(|character| match character.to_ascii_uppercase() {
            'I' | 'L' => '1',
            'O' => '0',
            other => other,
        })
        .collect();

    if normalised.len() != PAIRING_CODE_LEN {
        return Err(VaultError::Pairing(format!(
            "a typed pairing code is {PAIRING_CODE_LEN} characters"
        )));
    }

    let mut bits: u64 = 0;
    for character in normalised.bytes() {
        let value = CROCKFORD
            .iter()
            .position(|candidate| *candidate == character)
            .ok_or_else(|| {
                VaultError::Pairing("pairing code contains an invalid character".into())
            })?;
        bits = (bits << 5) | value as u64;
    }

    // 8 characters of 5 bits = 40 bits = the first 5 bytes of the secret.
    let mut prefix = [0u8; 5];
    for (index, byte) in prefix.iter_mut().enumerate() {
        *byte = ((bits >> (32 - index * 8)) & 0xff) as u8;
    }
    Ok(prefix)
}

/// What the joining Device presents to authenticate the pairing channel. It is
/// either the full secret scanned from the QR payload or the 40-bit typed
/// fallback.
#[derive(Clone)]
pub struct PairingClaim {
    kind: ClaimKind,
}

#[derive(Clone)]
enum ClaimKind {
    Full(PairingSecret),
    Typed([u8; 5]),
}

impl PairingClaim {
    fn full(secret: PairingSecret) -> Self {
        Self {
            kind: ClaimKind::Full(secret),
        }
    }

    fn typed(code: &str) -> Result<Self, VaultError> {
        Ok(Self {
            kind: ClaimKind::Typed(typed_prefix(code)?),
        })
    }

    /// Whether this claim authenticates `secret`. Constant-time for the full
    /// form; the typed form can only match the 40-bit prefix it carries.
    pub fn matches(&self, secret: &PairingSecret) -> bool {
        match &self.kind {
            ClaimKind::Full(candidate) => candidate.constant_time_eq(secret),
            ClaimKind::Typed(prefix) => constant_time_eq(prefix, &secret.0[..5]),
        }
    }

    /// The bytes a transport proof may be derived from, and whether they are
    /// the full 128-bit secret.
    ///
    /// The typed fallback carries only its 40-bit prefix, so a channel where
    /// either side presents the typed form can never be stronger than that
    /// prefix. This is the trade-off ADR-0002 §3 accepts for pairing without a
    /// camera; a transport must truncate to these bytes on both sides rather
    /// than pretend the full secret was proven.
    pub(crate) fn transport_material(&self) -> (&[u8], bool) {
        match &self.kind {
            ClaimKind::Full(secret) => (&secret.0, true),
            ClaimKind::Typed(prefix) => (prefix, false),
        }
    }

    /// Whether two presentations name the same secret. Used by the transport
    /// seam to decide if an inbound channel is authenticated by the offer it
    /// registered.
    pub(crate) fn matches_claim(&self, other: &Self) -> bool {
        match (&self.kind, &other.kind) {
            (ClaimKind::Full(first), ClaimKind::Full(second)) => first.constant_time_eq(second),
            (ClaimKind::Full(first), ClaimKind::Typed(prefix))
            | (ClaimKind::Typed(prefix), ClaimKind::Full(first)) => {
                constant_time_eq(prefix, &first.0[..5])
            }
            (ClaimKind::Typed(first), ClaimKind::Typed(second)) => constant_time_eq(first, second),
        }
    }
}

fn constant_time_eq(first: &[u8], second: &[u8]) -> bool {
    first.len() == second.len()
        && first
            .iter()
            .zip(second.iter())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

impl fmt::Debug for PairingClaim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PairingClaim(<redacted>)")
    }
}

/// What the User enters on the joining Device: a scanned QR payload, or the
/// short code typed by hand plus the Device it belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PairingCode {
    Qr {
        payload: String,
    },
    Typed {
        source_device_id: DeviceId,
        code: String,
    },
}

impl PairingCode {
    pub fn qr(payload: impl Into<String>) -> Self {
        Self::Qr {
            payload: payload.into(),
        }
    }

    pub fn typed(source_device_id: DeviceId, code: impl Into<String>) -> Self {
        Self::Typed {
            source_device_id,
            code: code.into(),
        }
    }

    pub fn source_device_id(&self) -> Result<DeviceId, VaultError> {
        Ok(self.parse()?.source_device_id)
    }

    /// The claim used to authenticate the channel. It is deliberately not
    /// serializable so it cannot be handed to the UI as display data.
    pub fn claim(&self) -> Result<PairingClaim, VaultError> {
        Ok(self.parse()?.claim)
    }

    pub(crate) fn expires_at(&self) -> Result<Option<DateTime<Utc>>, VaultError> {
        Ok(self.parse()?.expires_at)
    }

    fn parse(&self) -> Result<ParsedCode, VaultError> {
        match self {
            Self::Qr { payload } => parse_qr_payload(payload),
            Self::Typed {
                source_device_id,
                code,
            } => Ok(ParsedCode {
                source_device_id: *source_device_id,
                claim: PairingClaim::typed(code)?,
                expires_at: None,
            }),
        }
    }
}

struct ParsedCode {
    source_device_id: DeviceId,
    claim: PairingClaim,
    expires_at: Option<DateTime<Utc>>,
}

fn parse_qr_payload(payload: &str) -> Result<ParsedCode, VaultError> {
    let fields: Vec<&str> = payload.split(':').collect();
    if fields.len() != 6 || fields[0] != QR_PREFIX || fields[1] != QR_VERSION {
        return Err(VaultError::Pairing(
            "this is not a Lockr pairing code".into(),
        ));
    }

    let source_device_id = Uuid::parse_str(fields[2])
        .map_err(|_| VaultError::Pairing("pairing code names an invalid Device".into()))?;
    // The session id is structural: parse it so a truncated code is refused
    // rather than silently accepted.
    Uuid::parse_str(fields[3])
        .map_err(|_| VaultError::Pairing("pairing code names an invalid session".into()))?;
    let expires_at = DateTime::from_timestamp(
        fields[4]
            .parse::<i64>()
            .map_err(|_| VaultError::Pairing("pairing code has an invalid expiry".into()))?,
        0,
    )
    .ok_or_else(|| VaultError::Pairing("pairing code has an invalid expiry".into()))?;

    let secret = URL_SAFE_NO_PAD
        .decode(fields[5])
        .map_err(|_| VaultError::Pairing("pairing code carries an invalid secret".into()))?;
    let secret = PairingSecret::from_slice(&secret)?;

    Ok(ParsedCode {
        source_device_id,
        claim: PairingClaim::full(secret),
        expires_at: Some(expires_at),
    })
}

/// What the source Device renders for the User: a QR payload, a typed
/// fallback, and the moment the offer stops working.
#[derive(Debug, Clone, Serialize)]
pub struct PairingOffer {
    pub session_id: Uuid,
    pub source_device_id: DeviceId,
    pub qr_payload: String,
    pub typed_code: String,
    pub expires_at: DateTime<Utc>,
}

impl PairingOffer {
    /// The claim the source binds its ephemeral transport to. Deliberately
    /// detached from the serializable offer so `PairingOffer` can be shown in
    /// the UI without exposing the secret as a field.
    pub fn claim(&self) -> PairingClaim {
        parse_qr_payload(&self.qr_payload)
            .expect("an offer this type constructed always parses")
            .claim
    }

    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        now >= self.expires_at
    }
}

/// The joining Device's announcement. Safe to show in the approval prompt and
/// safe to travel in the clear: it carries identity metadata only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairingRequest {
    pub device_id: DeviceId,
    pub name: String,
    pub platform: String,
    pub public_key: String,
    pub schema_version: u32,
}

/// What the human on the source Device is asked to approve.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PairingApprovalPrompt {
    pub device_id: DeviceId,
    pub name: String,
    pub platform: String,
}

/// The User's 32-byte secret key while it is being transferred. Zeroized when
/// dropped and redacted from `Debug`; it is only ever serialized as part of a
/// [`PairingTransfer`] frame on an already-authenticated channel.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PairingSecretKey([u8; 32]);

impl PairingSecretKey {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Drop for PairingSecretKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl fmt::Debug for PairingSecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PairingSecretKey(<redacted>)")
    }
}

/// Everything the source sends once - and only once - a human has approved.
///
/// It holds the User id and shared profile metadata inside
/// [`SharedProfile`], plus the secret key. It never holds the master password.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairingTransfer {
    pub schema_version: u32,
    pub shared_profile: SharedProfile,
    pub secret_key: PairingSecretKey,

    /// The source Device's pre-checked Sync Set for the joining Device, in
    /// Vault ids. `None` means the default: every Vault in the shared Vault
    /// Registry. It is the joining Device's initial device-local state, not
    /// shared registry state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync_set: Option<Vec<VaultId>>,
}

/// The messages that cross a pairing channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PairingMessage {
    Join(PairingRequest),
    Transfer(PairingTransfer),
}

impl PairingMessage {
    pub fn as_request(&self) -> Option<&PairingRequest> {
        match self {
            Self::Join(request) => Some(request),
            Self::Transfer(_) => None,
        }
    }

    pub fn as_transfer(&self) -> Option<&PairingTransfer> {
        match self {
            Self::Transfer(transfer) => Some(transfer),
            Self::Join(_) => None,
        }
    }
}

/// The source-side state machine for one offer.
pub(crate) struct PairingHost {
    user_id: UserId,
    source_device_id: DeviceId,
    schema_version: u32,
    expires_at: DateTime<Utc>,
    secret_key: Option<PairingSecretKey>,
    pending: Option<PairingRequest>,
    attempts: u32,
    last_attempt_at: Option<DateTime<Utc>>,
    consumed: bool,
}

impl PairingHost {
    pub(crate) fn start(
        user_id: UserId,
        source_device_id: DeviceId,
        schema_version: u32,
        secret_key: [u8; 32],
        now: DateTime<Utc>,
    ) -> (Self, PairingOffer) {
        let secret = PairingSecret::generate();
        let session_id = Uuid::new_v4();
        let expires_at = now + TimeDelta::seconds(PAIRING_TTL_SECS);
        let qr_payload = format!(
            "{QR_PREFIX}:{QR_VERSION}:{source_device_id}:{session_id}:{}:{}",
            expires_at.timestamp(),
            secret.qr_token()
        );

        let host = Self {
            user_id,
            source_device_id,
            schema_version,
            expires_at,
            secret_key: Some(PairingSecretKey::new(secret_key)),
            pending: None,
            attempts: 0,
            last_attempt_at: None,
            consumed: false,
        };

        let offer = PairingOffer {
            session_id,
            source_device_id,
            qr_payload,
            typed_code: secret.typed_code(),
            expires_at,
        };

        (host, offer)
    }

    pub(crate) fn user_id(&self) -> UserId {
        self.user_id
    }

    pub(crate) fn schema_version(&self) -> u32 {
        self.schema_version
    }

    #[cfg(test)]
    pub(crate) fn pending(&self) -> Option<&PairingRequest> {
        self.pending.as_ref()
    }

    /// Accept a join announcement, rate-limited and never before the Schema
    /// Version is known to agree. No key material is released here.
    pub(crate) fn receive(
        &mut self,
        message: PairingMessage,
        now: DateTime<Utc>,
    ) -> Result<PairingRequest, VaultError> {
        self.check_live(now)?;
        self.check_rate_limit(now)?;

        self.attempts += 1;
        self.last_attempt_at = Some(now);

        let request = match message {
            PairingMessage::Join(request) => request,
            PairingMessage::Transfer(_) => {
                return Err(VaultError::Pairing(
                    "the source Device received a transfer instead of a join request".into(),
                ));
            }
        };

        if request.schema_version != self.schema_version {
            // A schema mismatch aborts the offer outright: proceeding could
            // only hand over vaults the joining Device cannot read.
            self.consume();
            return Err(VaultError::PairingSchemaMismatch {
                local: self.schema_version,
                remote: request.schema_version,
            });
        }

        if request.device_id == self.source_device_id {
            return Err(VaultError::Pairing(
                "a Device cannot pair with itself".into(),
            ));
        }

        self.pending = Some(request.clone());
        Ok(request)
    }

    /// Consume the offer and release the secret key. This is the moment the
    /// human approved; it cannot be undone and the offer cannot be reused.
    pub(crate) fn approve(
        &mut self,
        device_id: DeviceId,
        now: DateTime<Utc>,
    ) -> Result<(PairingRequest, PairingSecretKey), VaultError> {
        self.check_live(now)?;

        let request = self
            .pending
            .as_ref()
            .ok_or_else(|| VaultError::Pairing("no joining Device is waiting".into()))?
            .clone();
        if request.device_id != device_id {
            return Err(VaultError::Pairing(
                "the approved Device is not the Device that asked to join".into(),
            ));
        }

        let secret_key = self
            .secret_key
            .take()
            .ok_or(VaultError::PairingSecretStale)?;
        self.pending = None;
        self.consumed = true;
        Ok((request, secret_key))
    }

    pub(crate) fn cancel(&mut self) {
        self.consume();
    }

    fn check_live(&mut self, now: DateTime<Utc>) -> Result<(), VaultError> {
        if self.consumed || self.secret_key.is_none() {
            return Err(VaultError::PairingSecretStale);
        }
        if now >= self.expires_at {
            self.consume();
            return Err(VaultError::PairingSecretStale);
        }
        Ok(())
    }

    fn check_rate_limit(&self, now: DateTime<Utc>) -> Result<(), VaultError> {
        if self.attempts >= PAIRING_MAX_ATTEMPTS {
            return Err(VaultError::PairingRateLimited);
        }
        if let Some(last) = self.last_attempt_at {
            if now.signed_duration_since(last) < TimeDelta::seconds(PAIRING_RETRY_INTERVAL_SECS) {
                return Err(VaultError::PairingRateLimited);
            }
        }
        Ok(())
    }

    fn consume(&mut self) {
        self.consumed = true;
        self.pending = None;
        self.secret_key = None;
    }
}

/// The joining side's state between announcing itself and receiving the
/// transfer. It keeps the keypair whose public half the source will admit, so
/// the same identity is written to the local profile.
pub(crate) struct PendingJoin {
    pub(crate) source_device_id: DeviceId,
    pub(crate) schema_version: u32,
    pub(crate) expires_at: DateTime<Utc>,
    pub(crate) keypair: crate::device::DeviceKeypair,
    pub(crate) request: PairingRequest,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_the_qr_payload_and_typed_fallback_both_authenticate_the_secret() {
        let secret = PairingSecret::generate();
        let source = Uuid::new_v4();

        let payload = format!(
            "{QR_PREFIX}:{QR_VERSION}:{source}:{}:{}:{}",
            Uuid::new_v4(),
            (Utc::now() + TimeDelta::seconds(PAIRING_TTL_SECS)).timestamp(),
            secret.qr_token()
        );

        let scanned = PairingCode::qr(payload);
        assert!(scanned.claim().unwrap().matches(&secret));

        let typed = PairingCode::typed(source, secret.typed_code());
        assert!(typed.claim().unwrap().matches(&secret));
        assert_eq!(typed.source_device_id().unwrap(), source);

        // A different secret never matches either form.
        let other = PairingSecret::generate();
        assert!(!scanned.claim().unwrap().matches(&other));
        assert!(!PairingCode::typed(source, other.typed_code())
            .claim()
            .unwrap()
            .matches(&secret));
    }

    #[test]
    fn test_the_typed_fallback_is_eight_characters_with_a_display_hyphen() {
        let code = PairingSecret::generate().typed_code();

        assert_eq!(code.len(), PAIRING_CODE_LEN + 1);
        assert_eq!(code.chars().nth(4), Some('-'));

        // Crockford aliases are accepted when typing.
        let normalised: String = code.chars().filter(|c| *c != '-').collect();
        let alias = normalised
            .chars()
            .map(|c| match c {
                '1' => 'I',
                '0' => 'O',
                other => other,
            })
            .collect::<String>();
        assert_eq!(
            typed_prefix(&normalised).unwrap(),
            typed_prefix(&alias).unwrap()
        );
    }

    #[test]
    fn test_a_malformed_pairing_code_is_refused() {
        assert!(PairingCode::qr("not a pairing code").claim().is_err());
        assert!(PairingCode::qr(format!("{QR_PREFIX}:v2:not:enough"))
            .claim()
            .is_err());
        assert!(PairingCode::typed(Uuid::new_v4(), "short").claim().is_err());
        assert!(PairingCode::typed(Uuid::new_v4(), "!!!!-!!!!")
            .claim()
            .is_err());
    }

    #[test]
    fn test_the_offer_is_short_lived() {
        let now = Utc::now();
        let (_, offer) = PairingHost::start(Uuid::new_v4(), Uuid::new_v4(), 3, [7u8; 32], now);

        assert!(!offer.is_expired(now));
        assert!(
            offer.is_expired(now + TimeDelta::seconds(PAIRING_TTL_SECS)),
            "the offer must not outlive its TTL"
        );
    }

    fn join(device_id: DeviceId, schema_version: u32) -> PairingMessage {
        PairingMessage::Join(PairingRequest {
            device_id,
            name: "Laptop".to_string(),
            platform: "windows".to_string(),
            public_key: "public-key".to_string(),
            schema_version,
        })
    }

    #[test]
    fn test_a_stale_or_reused_offer_is_refused() {
        let now = Utc::now();
        let source = Uuid::new_v4();
        let joiner = Uuid::new_v4();
        let (mut host, _) = PairingHost::start(Uuid::new_v4(), source, 3, [9u8; 32], now);

        // Expired before any attempt.
        assert!(matches!(
            host.receive(join(joiner, 3), now + TimeDelta::seconds(PAIRING_TTL_SECS)),
            Err(VaultError::PairingSecretStale)
        ));

        // A fresh offer can be approved exactly once.
        let (mut host, _) = PairingHost::start(Uuid::new_v4(), source, 3, [9u8; 32], now);
        host.receive(join(joiner, 3), now).unwrap();
        host.approve(joiner, now).unwrap();
        assert!(matches!(
            host.receive(join(joiner, 3), now),
            Err(VaultError::PairingSecretStale)
        ));
    }

    #[test]
    fn test_pairing_attempts_are_rate_limited() {
        let now = Utc::now();
        let source = Uuid::new_v4();
        let joiner = Uuid::new_v4();
        let (mut host, _) = PairingHost::start(Uuid::new_v4(), source, 3, [9u8; 32], now);

        for attempt in 0..PAIRING_MAX_ATTEMPTS {
            let at = now + TimeDelta::seconds(i64::from(attempt) * 2);
            host.receive(join(joiner, 3), at).unwrap();
        }

        assert!(matches!(
            host.receive(
                join(joiner, 3),
                now + TimeDelta::seconds(i64::from(PAIRING_MAX_ATTEMPTS) * 2)
            ),
            Err(VaultError::PairingRateLimited)
        ));
    }

    #[test]
    fn test_repeated_attempts_inside_the_retry_window_are_rate_limited() {
        let now = Utc::now();
        let source = Uuid::new_v4();
        let (mut host, _) = PairingHost::start(Uuid::new_v4(), source, 3, [9u8; 32], now);

        host.receive(join(Uuid::new_v4(), 3), now).unwrap();

        assert!(matches!(
            host.receive(join(Uuid::new_v4(), 3), now),
            Err(VaultError::PairingRateLimited)
        ));
    }

    #[test]
    fn test_a_schema_mismatch_burns_the_offer_before_anything_is_released() {
        let now = Utc::now();
        let source = Uuid::new_v4();
        let joiner = Uuid::new_v4();
        let (mut host, _) = PairingHost::start(Uuid::new_v4(), source, 3, [9u8; 32], now);

        assert!(matches!(
            host.receive(join(joiner, 2), now),
            Err(VaultError::PairingSchemaMismatch {
                local: 3,
                remote: 2
            })
        ));

        // The offer cannot be approved afterwards, and the secret key never
        // became reachable.
        assert!(matches!(
            host.approve(joiner, now),
            Err(VaultError::PairingSecretStale)
        ));
        assert!(host.pending().is_none());
    }
}

#[cfg(test)]
mod integration_tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use super::*;
    use crate::network::{
        encode_frame, InMemoryDeviceConfig, InMemoryHub, InMemoryNetwork, PeerChannel, PeerNetwork,
        SyncMessage,
    };
    use crate::user_manager::UserManager;

    const MASTER_PASSWORD: &str = "Correct-Horse-Battery-Staple-9";
    const SCHEMA_VERSION: u32 = 3;

    fn setup_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lockr_pairing_{tag}_{}", Uuid::new_v4()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    struct Fixture {
        hub: Arc<InMemoryHub>,
        source_net: InMemoryNetwork,
        joiner_net: InMemoryNetwork,
        source: UserManager,
        joiner: UserManager,
        user_id: UserId,
        secret_key: Vec<u8>,
        source_dir: PathBuf,
        joiner_dir: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let source_dir = setup_dir("source");
            let joiner_dir = setup_dir("joiner");

            let mut source = UserManager::new(source_dir.clone()).unwrap();
            let user = source
                .register("Alice".to_string(), MASTER_PASSWORD)
                .unwrap();
            let secret_key = source.login(&user.id, MASTER_PASSWORD).unwrap();

            let joiner = UserManager::new(joiner_dir.clone()).unwrap();

            let hub = InMemoryHub::new();
            let source_net = hub.join(InMemoryDeviceConfig::new(
                source.device_id(),
                SCHEMA_VERSION,
            ));
            let joiner_net = hub.join(InMemoryDeviceConfig::new(
                joiner.device_id(),
                SCHEMA_VERSION,
            ));
            source_net.start_advertising().unwrap();

            Self {
                hub,
                source_net,
                joiner_net,
                source,
                joiner,
                user_id: user.id,
                secret_key,
                source_dir,
                joiner_dir,
            }
        }

        /// The joining side dials the source over the pairing secret, and the
        /// source takes the responder side. The channel exists only while the
        /// offer is live.
        fn open_channel(
            &self,
            claim: &PairingClaim,
        ) -> (Box<dyn PeerChannel>, Box<dyn PeerChannel>) {
            let peer = self.joiner_net.discover().unwrap().remove(0);
            assert_eq!(peer.device_id(), self.source.device_id());

            let initiator = self.joiner_net.connect_for_pairing(&peer, claim).unwrap();
            let responder = self
                .source_net
                .accept_for_pairing(claim)
                .expect("the source accepts the authenticated pairing channel")
                .expect("the dialled channel is waiting");

            (initiator, responder)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = self.hub;
            std::fs::remove_dir_all(&self.source_dir).ok();
            std::fs::remove_dir_all(&self.joiner_dir).ok();
        }
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle)
    }

    fn pairing_message(message: SyncMessage) -> PairingMessage {
        match message {
            SyncMessage::Pairing(message) => message,
            other => panic!("expected a pairing frame, got {other:?}"),
        }
    }

    #[test]
    fn test_pairing_requires_approval_before_the_secret_key_is_transferred() {
        let mut fixture = Fixture::new();
        let now = Utc::now();

        let offer = fixture
            .source
            .start_pairing(&fixture.secret_key, SCHEMA_VERSION, now)
            .unwrap();
        let code = PairingCode::qr(offer.qr_payload.clone());
        let claim = code.claim().unwrap();
        fixture.source_net.begin_pairing(&claim).unwrap();

        let join = fixture
            .joiner
            .join_pairing(&code, SCHEMA_VERSION, now)
            .unwrap();
        let (mut initiator, mut responder) = fixture.open_channel(&claim);
        initiator.send(SyncMessage::Pairing(join.clone())).unwrap();

        // The announcement is what the human approves, and it carries no key
        // material. Nothing has been written on the joining side yet.
        let prompt = fixture
            .source
            .receive_pairing_request(pairing_message(responder.receive().unwrap()), now)
            .unwrap();
        assert_eq!(prompt.device_id, fixture.joiner.device_id());
        assert_eq!(prompt.name, crate::device::device_name());
        assert_eq!(prompt.platform, crate::device::device_platform());
        assert!(
            fixture.joiner.active_user_id.is_none(),
            "nothing is written before approval"
        );
        assert!(
            !fixture
                .source
                .get_shared_profile(&fixture.user_id)
                .unwrap()
                .devices
                .contains(&fixture.joiner.device_id()),
            "the joining Device must not be admitted before approval"
        );

        // Only then does the source build the one-time transfer.
        let transfer = fixture
            .source
            .approve_pairing(prompt.device_id, now)
            .unwrap();
        let PairingMessage::Transfer(ref payload) = transfer else {
            panic!("approval must produce a transfer");
        };
        assert_eq!(
            payload.secret_key.as_bytes().as_slice(),
            fixture.secret_key.as_slice(),
            "the approved transfer carries the User's secret key"
        );

        responder.send(SyncMessage::Pairing(transfer)).unwrap();
        let joined = fixture
            .joiner
            .complete_pairing(
                pairing_message(initiator.receive().unwrap()),
                MASTER_PASSWORD,
                now,
            )
            .unwrap();

        assert_eq!(joined.id, fixture.user_id);
        assert_eq!(joined.name, "Alice");
        assert_eq!(
            fixture.joiner.login(&joined.id, MASTER_PASSWORD).unwrap(),
            fixture.secret_key,
            "the joining Device re-wraps the same secret key under the local password"
        );

        let joiner_shared = fixture.joiner.get_shared_profile(&joined.id).unwrap();
        joiner_shared.devices.verify().unwrap();
        assert!(joiner_shared.devices.contains(&fixture.joiner.device_id()));
        assert!(joiner_shared.devices.contains(&fixture.source.device_id()));

        let source_shared = fixture.source.get_shared_profile(&fixture.user_id).unwrap();
        assert!(source_shared.devices.contains(&fixture.joiner.device_id()));

        // The offer is single use: closing it refuses the secret at the seam.
        fixture.source_net.end_pairing().unwrap();
        assert!(
            fixture.source_net.accept_for_pairing(&claim).is_err(),
            "a reused pairing secret must not open another channel"
        );
    }

    #[test]
    fn test_pairing_carries_the_prechecked_sync_set_for_the_joining_device() {
        let mut fixture = Fixture::new();
        let now = Utc::now();
        let vault_id = Uuid::new_v4();
        let unknown = Uuid::new_v4();
        fixture
            .source
            .add_vault_to_user(vault_id)
            .expect("the source's Vault Registry accepts the Vault");

        let offer = fixture
            .source
            .start_pairing(&fixture.secret_key, SCHEMA_VERSION, now)
            .unwrap();
        let code = PairingCode::qr(offer.qr_payload.clone());
        let claim = code.claim().unwrap();
        fixture.source_net.begin_pairing(&claim).unwrap();

        let join = fixture
            .joiner
            .join_pairing(&code, SCHEMA_VERSION, now)
            .unwrap();
        let (mut initiator, mut responder) = fixture.open_channel(&claim);
        initiator.send(SyncMessage::Pairing(join)).unwrap();
        let prompt = fixture
            .source
            .receive_pairing_request(pairing_message(responder.receive().unwrap()), now)
            .unwrap();

        // Ids outside the shared Vault Registry are dropped: the registry is
        // authoritative on which Vaults exist.
        let transfer = fixture
            .source
            .approve_pairing_with_sync_set(prompt.device_id, Some(vec![vault_id, unknown]), now)
            .unwrap();
        let PairingMessage::Transfer(payload) = transfer else {
            panic!("approval must produce a transfer");
        };
        assert_eq!(payload.sync_set, Some(vec![vault_id]));
    }

    #[test]
    fn test_the_typed_fallback_can_pair_without_a_camera() {
        let mut fixture = Fixture::new();
        let now = Utc::now();

        let offer = fixture
            .source
            .start_pairing(&fixture.secret_key, SCHEMA_VERSION, now)
            .unwrap();
        let code = PairingCode::typed(fixture.source.device_id(), offer.typed_code.clone());
        let claim = code.claim().unwrap();
        fixture.source_net.begin_pairing(&claim).unwrap();

        let join = fixture
            .joiner
            .join_pairing(&code, SCHEMA_VERSION, now)
            .unwrap();
        let (mut initiator, mut responder) = fixture.open_channel(&claim);
        initiator.send(SyncMessage::Pairing(join)).unwrap();

        let prompt = fixture
            .source
            .receive_pairing_request(pairing_message(responder.receive().unwrap()), now)
            .unwrap();
        assert_eq!(prompt.device_id, fixture.joiner.device_id());
    }

    #[test]
    fn test_a_stale_or_reused_pairing_secret_is_refused() {
        let mut fixture = Fixture::new();
        let now = Utc::now();

        // Expired after the short TTL.
        let offer = fixture
            .source
            .start_pairing(&fixture.secret_key, SCHEMA_VERSION, now)
            .unwrap();
        let code = PairingCode::qr(offer.qr_payload.clone());
        let join = fixture
            .joiner
            .join_pairing(&code, SCHEMA_VERSION, now)
            .unwrap();
        assert!(matches!(
            fixture
                .source
                .receive_pairing_request(join, now + TimeDelta::seconds(PAIRING_TTL_SECS + 1)),
            Err(VaultError::PairingSecretStale)
        ));

        // Reused after a successful approval.
        let offer = fixture
            .source
            .start_pairing(&fixture.secret_key, SCHEMA_VERSION, now)
            .unwrap();
        let code = PairingCode::qr(offer.qr_payload.clone());
        let join = fixture
            .joiner
            .join_pairing(&code, SCHEMA_VERSION, now)
            .unwrap();
        let prompt = fixture
            .source
            .receive_pairing_request(join.clone(), now)
            .unwrap();
        fixture
            .source
            .approve_pairing(prompt.device_id, now)
            .unwrap();

        assert!(matches!(
            fixture
                .source
                .receive_pairing_request(join, now + TimeDelta::seconds(2)),
            Err(VaultError::PairingSecretStale)
        ));
    }

    #[test]
    fn test_a_schema_mismatch_aborts_before_the_transfer() {
        let mut fixture = Fixture::new();
        let now = Utc::now();

        let offer = fixture
            .source
            .start_pairing(&fixture.secret_key, SCHEMA_VERSION, now)
            .unwrap();
        let code = PairingCode::qr(offer.qr_payload.clone());
        // The joining Device still speaks the previous schema.
        let join = fixture
            .joiner
            .join_pairing(&code, SCHEMA_VERSION - 1, now)
            .unwrap();

        let error = fixture
            .source
            .receive_pairing_request(join, now)
            .expect_err("a schema mismatch must refuse the pairing");
        assert!(matches!(
            error,
            VaultError::PairingSchemaMismatch {
                local: SCHEMA_VERSION,
                remote: 2,
            }
        ));

        assert!(
            fixture
                .source
                .approve_pairing(fixture.joiner.device_id(), now)
                .is_err(),
            "a mismatched offer cannot be approved into a transfer"
        );
        assert!(fixture.joiner.active_user_id.is_none());
        assert!(!fixture
            .source
            .get_shared_profile(&fixture.user_id)
            .unwrap()
            .devices
            .contains(&fixture.joiner.device_id()));
    }

    #[test]
    fn test_no_exchanged_payload_contains_the_master_password() {
        let mut fixture = Fixture::new();
        let now = Utc::now();

        let offer = fixture
            .source
            .start_pairing(&fixture.secret_key, SCHEMA_VERSION, now)
            .unwrap();
        let code = PairingCode::qr(offer.qr_payload.clone());
        let claim = code.claim().unwrap();
        fixture.source_net.begin_pairing(&claim).unwrap();

        let join = fixture
            .joiner
            .join_pairing(&code, SCHEMA_VERSION, now)
            .unwrap();
        let (mut initiator, mut responder) = fixture.open_channel(&claim);

        let join_frame = encode_frame(&SyncMessage::Pairing(join.clone())).unwrap();
        let join_wire = serde_json::to_value(&join).unwrap();
        assert_eq!(join_wire["kind"], "join");
        assert!(
            join_wire.get("secret_key").is_none(),
            "the announcement carries no key material"
        );
        assert!(
            !contains(&join_frame, MASTER_PASSWORD.as_bytes()),
            "the master password must never cross the wire"
        );
        assert!(
            !contains(&join_frame, &fixture.secret_key),
            "no key material may move before approval"
        );

        initiator.send(SyncMessage::Pairing(join)).unwrap();
        let prompt = fixture
            .source
            .receive_pairing_request(pairing_message(responder.receive().unwrap()), now)
            .unwrap();
        let transfer = fixture
            .source
            .approve_pairing(prompt.device_id, now)
            .unwrap();

        let transfer_frame = encode_frame(&SyncMessage::Pairing(transfer.clone())).unwrap();
        // The transfer is not vacuous: it really carries the secret key.
        let wire = serde_json::to_value(&transfer).unwrap();
        let secret_on_wire: Vec<u8> = wire["secret_key"]
            .as_array()
            .expect("the transfer carries the key as a byte array")
            .iter()
            .map(|byte| byte.as_u64().expect("a byte") as u8)
            .collect();
        assert_eq!(secret_on_wire, fixture.secret_key);
        assert!(
            !contains(&transfer_frame, MASTER_PASSWORD.as_bytes()),
            "the transfer must carry the secret key, never the master password"
        );
        assert!(
            !format!("{transfer:?}").contains(MASTER_PASSWORD),
            "debug output must not leak the password"
        );

        responder.send(SyncMessage::Pairing(transfer)).unwrap();
        fixture
            .joiner
            .complete_pairing(
                pairing_message(initiator.receive().unwrap()),
                MASTER_PASSWORD,
                now,
            )
            .unwrap();
    }
}
