//! The single network seam for Device sync.
//!
//! Everything the sync engine needs from the network lives here: discovering
//! peers on the local network, opening an authenticated channel to one, and
//! advertising this Device for the duration of a session.
//!
//! The seam deliberately never exposes transport details. Discovery yields an
//! opaque [`DeviceId`] and an opaque [`PeerEndpoint`]; a channel moves only
//! already-encrypted [`EncryptedVaultBlob`]s and opaque [`RegistryState`].
//! mDNS, TLS, and raw addresses live behind a [`PeerNetwork`] implementation,
//! never in front of one.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::pairing::{PairingClaim, PairingMessage};
use crate::{DeviceId, VaultId};

/// The mDNS service type this app advertises. Paired with a [`DeviceId`] it
/// is the entire advertised payload.
pub const SYNC_SERVICE_TYPE: &str = "_lockr-sync._tcp.local.";

/// Version of the exchange protocol spoken over a [`PeerChannel`].
pub const SYNC_PROTOCOL_VERSION: u16 = 1;

/// Errors the seam reports to its callers. Each failure mode named by the
/// sync specification is distinct, so callers never have to guess.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum NetworkError {
    #[error("Peer is unreachable: {0}")]
    PeerUnreachable(String),

    #[error("Peer authentication failed")]
    AuthenticationFailed,

    #[error("Incompatible sync protocol: local {local}, remote {remote}")]
    ProtocolMismatch { local: u16, remote: u16 },

    #[error("Incompatible vault schema: local {local}, remote {remote}")]
    SchemaMismatch { local: u32, remote: u32 },

    #[error("Malformed sync frame: {0}")]
    MalformedFrame(String),

    #[error("Peer channel is closed")]
    ConnectionClosed,

    #[error("Peer did not respond in time")]
    Timeout,

    #[error("Discovery failed: {0}")]
    DiscoveryFailed(String),

    #[error("Peer transport is unavailable: {0}")]
    TransportUnavailable(String),
}

/// What a Device advertises on the local network: a service type and an
/// opaque device id. Nothing else may be added here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PeerAdvertisement {
    service_type: String,
    device_id: DeviceId,
}

impl PeerAdvertisement {
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            service_type: SYNC_SERVICE_TYPE.to_string(),
            device_id,
        }
    }

    pub fn service_type(&self) -> &str {
        &self.service_type
    }

    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }
}

/// Where a discovered peer can be reached. Opaque by design: only a
/// [`PeerNetwork`] implementation interprets it, and its `Debug` output
/// deliberately hides the address so it cannot leak into logs or the engine.
#[derive(Clone, PartialEq, Eq)]
pub struct PeerEndpoint(String);

impl PeerEndpoint {
    /// Only a [`PeerNetwork`] implementation may construct an endpoint.
    pub(crate) fn new(address: impl Into<String>) -> Self {
        Self(address.into())
    }

    /// Only a [`PeerNetwork`] implementation may read the address back.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for PeerEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PeerEndpoint(<opaque>)")
    }
}

/// A peer found during discovery. The device id and advertisement are all
/// the sync engine may look at; the endpoint is carried back to
/// [`PeerNetwork::connect`] untouched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredPeer {
    advertisement: PeerAdvertisement,
    endpoint: PeerEndpoint,
}

impl DiscoveredPeer {
    pub(crate) fn new(advertisement: PeerAdvertisement, endpoint: PeerEndpoint) -> Self {
        Self {
            advertisement,
            endpoint,
        }
    }

    pub fn device_id(&self) -> DeviceId {
        self.advertisement.device_id()
    }

    pub fn advertisement(&self) -> &PeerAdvertisement {
        &self.advertisement
    }

    /// The opaque address, for the one place that may display it: the
    /// discovered-peer list. The sync engine never reads it.
    pub(crate) fn address(&self) -> &str {
        self.endpoint.as_str()
    }
}

/// An already-encrypted vault file, opaque to the engine and to discovery.
/// Construction is explicit so plaintext vault content cannot accidentally
/// be handed to the seam.
#[derive(Clone, PartialEq, Eq)]
pub struct EncryptedVaultBlob(Vec<u8>);

impl EncryptedVaultBlob {
    pub fn from_encrypted_bytes(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for EncryptedVaultBlob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EncryptedVaultBlob")
            .field("len", &self.0.len())
            .finish()
    }
}

/// Shared registry state (Device Registry and Vault Registry) travelling as
/// opaque bytes. It carries no vault content and no secrets.
#[derive(Clone, PartialEq, Eq)]
pub struct RegistryState(Vec<u8>);

impl RegistryState {
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// The registry state a sync exchanges: both registries, encoded as
    /// shared identity data. No vault content and no secrets.
    pub fn from_registries(registries: &crate::SharedRegistries) -> Result<Self, NetworkError> {
        let bytes = serde_json::to_vec(registries).map_err(|err| {
            NetworkError::MalformedFrame(format!("registry state cannot be encoded: {err}"))
        })?;
        Ok(Self(bytes))
    }

    pub fn to_registries(&self) -> Result<crate::SharedRegistries, NetworkError> {
        serde_json::from_slice(&self.0).map_err(|err| {
            NetworkError::MalformedFrame(format!("registry state cannot be decoded: {err}"))
        })
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for RegistryState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegistryState")
            .field("len", &self.0.len())
            .finish()
    }
}

/// The scope a Device brings to one sync: the Vault ids it participates in,
/// plus the shared registries that say which Vaults and Devices exist.
///
/// It carries identities only. Vault names, item content, and secrets live
/// inside the encrypted vault files and never appear here. Exchanging scope
/// before any vault blob is what lets both sides agree on the intersection
/// without one side offering a Vault the other does not want.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeState {
    pub vault_ids: Vec<VaultId>,
    pub registries: crate::SharedRegistries,
}

impl ScopeState {
    pub fn new(vault_ids: Vec<VaultId>, registries: crate::SharedRegistries) -> Self {
        Self {
            vault_ids,
            registries,
        }
    }
}

/// Everything that may cross an authenticated peer channel. Vault content is
/// always already encrypted; there is no message for plaintext vault content.
#[derive(Clone, PartialEq, Eq)]
pub enum SyncMessage {
    Vault {
        vault_id: VaultId,
        blob: EncryptedVaultBlob,
    },
    Registry(RegistryState),

    /// The sender's Sync Set and shared registries, sent before any blob.
    Scope(ScopeState),

    /// A pairing exchange. Valid only on a channel a pairing secret
    /// authenticated; it is the one place a secret key legitimately travels.
    Pairing(PairingMessage),

    Complete,
}

impl fmt::Debug for SyncMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Vault { vault_id, blob } => f
                .debug_struct("Vault")
                .field("vault_id", vault_id)
                .field("blob", blob)
                .finish(),
            Self::Registry(state) => f.debug_tuple("Registry").field(state).finish(),
            Self::Scope(scope) => f
                .debug_struct("Scope")
                .field("vault_ids", &scope.vault_ids)
                .finish_non_exhaustive(),
            Self::Pairing(message) => f.debug_tuple("Pairing").field(message).finish(),
            Self::Complete => f.write_str("Complete"),
        }
    }
}

/// Wire framing for the exchange protocol.
///
/// ```text
/// +--------+------------------+------+----------------+---------+
/// | magic  | protocol version | kind | payload length | payload |
/// | 4 "LKRS"| u16 big-endian  | u8   | u32 big-endian | bytes   |
/// +--------+------------------+------+----------------+---------+
/// ```
///
/// The protocol version is checked on every frame, so a peer speaking a
/// different protocol is refused rather than half-read.
const MAGIC: [u8; 4] = *b"LKRS";
pub(crate) const HEADER_LEN: usize = 11;
const KIND_VAULT: u8 = 1;
const KIND_REGISTRY: u8 = 2;
const KIND_COMPLETE: u8 = 3;
const KIND_PAIRING: u8 = 4;
const KIND_SCOPE: u8 = 5;

/// Ceiling on a single frame, so a damaged length field cannot make the app
/// allocate without bound. Whole encrypted vault files are far smaller.
const MAX_FRAME_PAYLOAD: usize = 64 * 1024 * 1024;

pub(crate) fn encode_frame(message: &SyncMessage) -> Result<Vec<u8>, NetworkError> {
    let (kind, payload) = match message {
        SyncMessage::Vault { vault_id, blob } => {
            let mut payload = Vec::with_capacity(16 + blob.len());
            payload.extend_from_slice(vault_id.as_bytes());
            payload.extend_from_slice(blob.as_bytes());
            (KIND_VAULT, payload)
        }
        SyncMessage::Registry(state) => (KIND_REGISTRY, state.as_bytes().to_vec()),
        SyncMessage::Scope(scope) => (
            KIND_SCOPE,
            serde_json::to_vec(scope).map_err(|err| {
                NetworkError::MalformedFrame(format!("scope frame cannot be encoded: {err}"))
            })?,
        ),
        SyncMessage::Pairing(message) => (
            KIND_PAIRING,
            serde_json::to_vec(message).map_err(|err| {
                NetworkError::MalformedFrame(format!("pairing frame cannot be encoded: {err}"))
            })?,
        ),
        SyncMessage::Complete => (KIND_COMPLETE, Vec::new()),
    };

    if payload.len() > MAX_FRAME_PAYLOAD {
        return Err(NetworkError::MalformedFrame(
            "frame payload is too large".into(),
        ));
    }
    let payload_len = u32::try_from(payload.len())
        .map_err(|_| NetworkError::MalformedFrame("frame payload is too large".into()))?;

    let mut frame = Vec::with_capacity(HEADER_LEN + payload.len());
    frame.extend_from_slice(&MAGIC);
    frame.extend_from_slice(&SYNC_PROTOCOL_VERSION.to_be_bytes());
    frame.push(kind);
    frame.extend_from_slice(&payload_len.to_be_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

/// Validate a frame header and return the payload length it announces.
///
/// Streaming transports read the header first so they can size the payload
/// read without ever trusting an unbounded length; the magic, protocol
/// version, and ceiling are checked before a single payload byte is read.
pub(crate) fn frame_payload_len(header: &[u8; HEADER_LEN]) -> Result<usize, NetworkError> {
    if header[..4] != MAGIC {
        return Err(NetworkError::MalformedFrame(
            "missing Lockr frame header".into(),
        ));
    }

    let version = u16::from_be_bytes([header[4], header[5]]);
    if version != SYNC_PROTOCOL_VERSION {
        return Err(NetworkError::ProtocolMismatch {
            local: SYNC_PROTOCOL_VERSION,
            remote: version,
        });
    }

    let payload_len = u32::from_be_bytes([header[7], header[8], header[9], header[10]]) as usize;
    if payload_len > MAX_FRAME_PAYLOAD {
        return Err(NetworkError::MalformedFrame(
            "frame payload is too large".into(),
        ));
    }

    Ok(payload_len)
}

pub(crate) fn decode_frame(frame: &[u8]) -> Result<SyncMessage, NetworkError> {
    if frame.len() < HEADER_LEN {
        return Err(NetworkError::MalformedFrame(
            "missing Lockr frame header".into(),
        ));
    }

    let kind = frame[6];
    let payload_len = frame_payload_len(frame[..HEADER_LEN].try_into().expect("fixed length"))?;
    if frame.len() != HEADER_LEN + payload_len {
        return Err(NetworkError::MalformedFrame(
            "frame length does not match its payload".into(),
        ));
    }

    let payload = &frame[HEADER_LEN..];
    match kind {
        KIND_VAULT => {
            if payload.len() < 16 {
                return Err(NetworkError::MalformedFrame(
                    "vault frame is missing its vault id".into(),
                ));
            }
            let vault_id = Uuid::from_slice(&payload[..16])
                .map_err(|_| NetworkError::MalformedFrame("invalid vault id".into()))?;
            Ok(SyncMessage::Vault {
                vault_id,
                blob: EncryptedVaultBlob::from_encrypted_bytes(payload[16..].to_vec()),
            })
        }
        KIND_REGISTRY => Ok(SyncMessage::Registry(RegistryState::from_bytes(
            payload.to_vec(),
        ))),
        KIND_SCOPE => {
            let scope = serde_json::from_slice(payload).map_err(|err| {
                NetworkError::MalformedFrame(format!("scope frame cannot be decoded: {err}"))
            })?;
            Ok(SyncMessage::Scope(scope))
        }
        KIND_PAIRING => {
            let message = serde_json::from_slice(payload).map_err(|err| {
                NetworkError::MalformedFrame(format!("pairing frame cannot be decoded: {err}"))
            })?;
            Ok(SyncMessage::Pairing(message))
        }
        KIND_COMPLETE if payload.is_empty() => Ok(SyncMessage::Complete),
        KIND_COMPLETE => Err(NetworkError::MalformedFrame(
            "complete frame carries a payload".into(),
        )),
        _ => Err(NetworkError::MalformedFrame(format!(
            "unknown frame kind {kind}"
        ))),
    }
}

/// An authenticated channel to one peer. It exposes no address, no socket,
/// and no transport type.
pub trait PeerChannel: Send {
    fn peer_device_id(&self) -> DeviceId;

    fn send(&mut self, message: SyncMessage) -> Result<(), NetworkError>;

    fn receive(&mut self) -> Result<SyncMessage, NetworkError>;

    /// Receive with a deadline. Transports that can block must override this;
    /// the default refuses rather than waits, so forgetting the override makes
    /// the sync fail closed instead of silently blocking forever.
    fn receive_timeout(&mut self, _timeout: Duration) -> Result<SyncMessage, NetworkError> {
        Err(NetworkError::Timeout)
    }

    fn close(&mut self);
}

/// The single seam between the sync engine and the network. Implementations
/// may use mDNS and TLS in production, or an in-memory pipe in tests; the
/// engine cannot tell the difference.
pub trait PeerNetwork: Send + Sync {
    /// Begin advertising this Device for the current session.
    fn start_advertising(&self) -> Result<(), NetworkError>;

    /// Stop advertising. An idle app must be invisible on the network.
    fn stop_advertising(&self) -> Result<(), NetworkError>;

    /// Peers currently advertising on the local network.
    fn discover(&self) -> Result<Vec<DiscoveredPeer>, NetworkError>;

    /// Open an authenticated channel to a discovered peer. Fails if the peer
    /// cannot be reached, is not paired, or speaks another protocol/schema.
    fn connect(&self, peer: &DiscoveredPeer) -> Result<Box<dyn PeerChannel>, NetworkError>;

    /// Take the responder side of the next inbound connection, if any.
    fn accept(&self) -> Result<Option<Box<dyn PeerChannel>>, NetworkError>;

    /// Accept inbound pairing connections authenticated by `claim` until the
    /// offer ends. The production transport binds its ephemeral certificate to
    /// this one-time secret; the in-memory seam compares it directly.
    fn begin_pairing(&self, _claim: &PairingClaim) -> Result<(), NetworkError> {
        Err(NetworkError::TransportUnavailable(
            "the secure pairing transport is not wired up".into(),
        ))
    }

    /// Stop accepting pairing connections and forget the one-time secret.
    fn end_pairing(&self) -> Result<(), NetworkError> {
        Ok(())
    }

    /// Open a pairing channel to an unpaired peer, authenticated solely by the
    /// single-use `claim`. Pairing is the only grant of access, so this is the
    /// one connection that does not require a prior mutual admission.
    fn connect_for_pairing(
        &self,
        _peer: &DiscoveredPeer,
        _claim: &PairingClaim,
    ) -> Result<Box<dyn PeerChannel>, NetworkError> {
        Err(NetworkError::TransportUnavailable(
            "the secure pairing transport is not wired up".into(),
        ))
    }

    /// Take the responder side of the next inbound pairing connection, if it
    /// authenticated with `claim`.
    fn accept_for_pairing(
        &self,
        _claim: &PairingClaim,
    ) -> Result<Option<Box<dyn PeerChannel>>, NetworkError> {
        Err(NetworkError::TransportUnavailable(
            "the secure pairing transport is not wired up".into(),
        ))
    }

    /// Hand a transport the identity this Device proves on a paired channel
    /// and the Device Registry it verifies peers against, for one signed-in
    /// session.
    ///
    /// This is deliberately part of the seam rather than an argument to
    /// `connect`: the sync and pairing engines must not carry key material,
    /// and the same session serves both halves of a connection. A transport
    /// that does not authenticate peers (the in-memory seam) ignores it.
    fn set_session(&self, _session: &TransportSession) -> Result<(), NetworkError> {
        Ok(())
    }

    /// Forget the session identity and its registry snapshot. A logout must
    /// not leave a signing key reachable from the network layer.
    fn clear_session(&self) {}
}

/// What a transport needs from the Device Registry to authenticate a peer.
///
/// Implementations answer for the *current* registry, so a revoked Device is
/// refused on the next connection. A Device that is absent and a Device that
/// is revoked are both `None`: neither may authenticate, and the transport
/// must not help a caller distinguish them before the proof is checked.
pub trait DeviceDirectory: Send + Sync {
    /// The Base64 Ed25519 public key of an admitted, unrevoked Device.
    fn admitted_public_key(&self, device_id: &DeviceId) -> Option<String>;
}

/// The identity and registry a transport uses for one session.
///
/// The keypair is held behind an `Arc` so the transport and the `UserManager`
/// share one unwrapped signing key for the session, and dropping it at logout
/// drops it everywhere.
pub struct TransportSession {
    pub device_id: DeviceId,
    pub keypair: Arc<crate::device::DeviceKeypair>,
    pub devices: Arc<dyn DeviceDirectory>,
}

impl fmt::Debug for TransportSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never print the signing key or a peer's public key set.
        f.debug_struct("TransportSession")
            .field("device_id", &self.device_id)
            .finish_non_exhaustive()
    }
}

/// Configuration for one in-memory Device.
#[derive(Debug, Clone)]
pub struct InMemoryDeviceConfig {
    device_id: DeviceId,
    schema_version: u32,
    protocol_version: u16,
    reachable: bool,
    paired_devices: HashSet<DeviceId>,
}

impl InMemoryDeviceConfig {
    pub fn new(device_id: DeviceId, schema_version: u32) -> Self {
        Self {
            device_id,
            schema_version,
            protocol_version: SYNC_PROTOCOL_VERSION,
            reachable: true,
            paired_devices: HashSet::new(),
        }
    }

    /// Announce a different exchange protocol, to exercise the distinct
    /// protocol-mismatch error.
    pub fn with_protocol_version(mut self, protocol_version: u16) -> Self {
        self.protocol_version = protocol_version;
        self
    }

    /// Record Devices this one has paired with. Pairing must be mutual for a
    /// channel to authenticate.
    pub fn paired_with(mut self, devices: impl IntoIterator<Item = DeviceId>) -> Self {
        self.paired_devices = devices.into_iter().collect();
        self
    }

    /// Make this Device discoverable but unreachable, to exercise the
    /// distinct unreachable-peer error.
    pub fn unreachable(mut self) -> Self {
        self.reachable = false;
        self
    }
}

struct InMemoryDevice {
    schema_version: u32,
    protocol_version: u16,
    reachable: bool,
    advertising: bool,
    paired_devices: HashSet<DeviceId>,
    /// The one-time claim this Device is currently accepting for pairing.
    pairing_claim: Option<PairingClaim>,
    inbound: VecDeque<InMemoryPipe>,
    pairing_inbound: VecDeque<InMemoryPipe>,
}

struct InMemoryPipe {
    initiator: DeviceId,
    to_responder: Arc<Mutex<VecDeque<Vec<u8>>>>,
    to_initiator: Arc<Mutex<VecDeque<Vec<u8>>>>,
    closed: Arc<AtomicBool>,
}

struct InMemoryChannel {
    peer_device_id: DeviceId,
    outbound: Arc<Mutex<VecDeque<Vec<u8>>>>,
    inbound: Arc<Mutex<VecDeque<Vec<u8>>>>,
    closed: Arc<AtomicBool>,
}

impl PeerChannel for InMemoryChannel {
    fn peer_device_id(&self) -> DeviceId {
        self.peer_device_id
    }

    fn send(&mut self, message: SyncMessage) -> Result<(), NetworkError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(NetworkError::ConnectionClosed);
        }
        let frame = encode_frame(&message)?;
        self.outbound.lock().unwrap().push_back(frame);
        Ok(())
    }

    fn receive(&mut self) -> Result<SyncMessage, NetworkError> {
        let frame = self
            .inbound
            .lock()
            .unwrap()
            .pop_front()
            .ok_or(NetworkError::ConnectionClosed)?;
        decode_frame(&frame)
    }

    fn receive_timeout(&mut self, _timeout: Duration) -> Result<SyncMessage, NetworkError> {
        // The in-memory pipe never blocks: a missing frame is a closed
        // connection, not a deadline.
        self.receive()
    }

    fn close(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
    }
}

/// A process-local network used by tests. Devices join a shared hub, which
/// serves the static peer list for discovery.
pub struct InMemoryHub {
    devices: Mutex<HashMap<DeviceId, InMemoryDevice>>,
}

impl InMemoryHub {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            devices: Mutex::new(HashMap::new()),
        })
    }

    pub fn join(self: &Arc<Self>, config: InMemoryDeviceConfig) -> InMemoryNetwork {
        self.devices.lock().unwrap().insert(
            config.device_id,
            InMemoryDevice {
                schema_version: config.schema_version,
                protocol_version: config.protocol_version,
                reachable: config.reachable,
                advertising: false,
                paired_devices: config.paired_devices,
                pairing_claim: None,
                inbound: VecDeque::new(),
                pairing_inbound: VecDeque::new(),
            },
        );

        InMemoryNetwork {
            device_id: config.device_id,
            hub: Arc::clone(self),
        }
    }
}

/// The in-memory [`PeerNetwork`]. A static peer list (the hub) plus an
/// in-process pipe stands in for mDNS and TLS.
pub struct InMemoryNetwork {
    device_id: DeviceId,
    hub: Arc<InMemoryHub>,
}

impl PeerNetwork for InMemoryNetwork {
    fn start_advertising(&self) -> Result<(), NetworkError> {
        let mut devices = self.hub.devices.lock().unwrap();
        let device = devices
            .get_mut(&self.device_id)
            .ok_or_else(|| NetworkError::DiscoveryFailed("device is not registered".into()))?;
        device.advertising = true;
        Ok(())
    }

    fn stop_advertising(&self) -> Result<(), NetworkError> {
        let mut devices = self.hub.devices.lock().unwrap();
        let device = devices
            .get_mut(&self.device_id)
            .ok_or_else(|| NetworkError::DiscoveryFailed("device is not registered".into()))?;
        device.advertising = false;
        Ok(())
    }

    fn discover(&self) -> Result<Vec<DiscoveredPeer>, NetworkError> {
        let devices = self.hub.devices.lock().unwrap();
        let mut peers: Vec<DiscoveredPeer> = devices
            .iter()
            .filter(|(id, device)| **id != self.device_id && device.advertising)
            .map(|(id, _)| {
                DiscoveredPeer::new(
                    PeerAdvertisement::new(*id),
                    PeerEndpoint::new(format!("in-memory://{}", id)),
                )
            })
            .collect();

        // A static peer list still deserves deterministic ordering.
        peers.sort_by_key(|peer| peer.device_id());
        Ok(peers)
    }

    fn connect(&self, peer: &DiscoveredPeer) -> Result<Box<dyn PeerChannel>, NetworkError> {
        let peer_id = peer.device_id();
        let mut devices = self.hub.devices.lock().unwrap();

        // The endpoint must be one this hub handed out: the sync engine can
        // carry it, but it cannot invent or alter an address.
        if peer.endpoint.as_str() != format!("in-memory://{}", peer_id) {
            return Err(NetworkError::PeerUnreachable(peer_id.to_string()));
        }

        let local = devices
            .get(&self.device_id)
            .ok_or_else(|| NetworkError::PeerUnreachable(format!("unknown peer {peer_id}")))?;
        let remote = devices
            .get(&peer_id)
            .ok_or_else(|| NetworkError::PeerUnreachable(peer_id.to_string()))?;

        if !remote.reachable {
            return Err(NetworkError::PeerUnreachable(peer_id.to_string()));
        }

        // Pairing is the only grant of access. Both sides must have admitted
        // the other, just as mutual TLS would require.
        if !local.paired_devices.contains(&peer_id)
            || !remote.paired_devices.contains(&self.device_id)
        {
            return Err(NetworkError::AuthenticationFailed);
        }

        if local.protocol_version != remote.protocol_version {
            return Err(NetworkError::ProtocolMismatch {
                local: local.protocol_version,
                remote: remote.protocol_version,
            });
        }

        if local.schema_version != remote.schema_version {
            return Err(NetworkError::SchemaMismatch {
                local: local.schema_version,
                remote: remote.schema_version,
            });
        }

        let pipe = InMemoryPipe {
            initiator: self.device_id,
            to_responder: Arc::new(Mutex::new(VecDeque::new())),
            to_initiator: Arc::new(Mutex::new(VecDeque::new())),
            closed: Arc::new(AtomicBool::new(false)),
        };

        let channel = InMemoryChannel {
            peer_device_id: peer_id,
            outbound: Arc::clone(&pipe.to_responder),
            inbound: Arc::clone(&pipe.to_initiator),
            closed: Arc::clone(&pipe.closed),
        };

        devices
            .get_mut(&peer_id)
            .expect("peer checked above")
            .inbound
            .push_back(pipe);

        Ok(Box::new(channel))
    }

    fn accept(&self) -> Result<Option<Box<dyn PeerChannel>>, NetworkError> {
        let mut devices = self.hub.devices.lock().unwrap();
        let local = devices
            .get_mut(&self.device_id)
            .ok_or_else(|| NetworkError::DiscoveryFailed("device is not registered".into()))?;

        Ok(local.inbound.pop_front().map(|pipe| {
            Box::new(InMemoryChannel {
                peer_device_id: pipe.initiator,
                outbound: Arc::clone(&pipe.to_initiator),
                inbound: Arc::clone(&pipe.to_responder),
                closed: pipe.closed,
            }) as Box<dyn PeerChannel>
        }))
    }

    fn begin_pairing(&self, claim: &PairingClaim) -> Result<(), NetworkError> {
        let mut devices = self.hub.devices.lock().unwrap();
        let device = devices
            .get_mut(&self.device_id)
            .ok_or_else(|| NetworkError::DiscoveryFailed("device is not registered".into()))?;
        device.pairing_claim = Some(claim.clone());
        Ok(())
    }

    fn end_pairing(&self) -> Result<(), NetworkError> {
        let mut devices = self.hub.devices.lock().unwrap();
        let device = devices
            .get_mut(&self.device_id)
            .ok_or_else(|| NetworkError::DiscoveryFailed("device is not registered".into()))?;
        device.pairing_claim = None;
        device.pairing_inbound.clear();
        Ok(())
    }

    fn connect_for_pairing(
        &self,
        peer: &DiscoveredPeer,
        claim: &PairingClaim,
    ) -> Result<Box<dyn PeerChannel>, NetworkError> {
        let peer_id = peer.device_id();
        let mut devices = self.hub.devices.lock().unwrap();

        if peer.endpoint.as_str() != format!("in-memory://{}", peer_id) {
            return Err(NetworkError::PeerUnreachable(peer_id.to_string()));
        }

        let local = devices
            .get(&self.device_id)
            .ok_or_else(|| NetworkError::PeerUnreachable(format!("unknown peer {peer_id}")))?;
        let remote = devices
            .get(&peer_id)
            .ok_or_else(|| NetworkError::PeerUnreachable(peer_id.to_string()))?;

        if !remote.reachable {
            return Err(NetworkError::PeerUnreachable(peer_id.to_string()));
        }

        // The one-time pairing secret replaces mutual admission here. It is
        // the only thing that authenticates an otherwise unpaired peer.
        let authenticated = remote
            .pairing_claim
            .as_ref()
            .is_some_and(|expected| expected.matches_claim(claim));
        if !authenticated {
            return Err(NetworkError::AuthenticationFailed);
        }

        if local.protocol_version != remote.protocol_version {
            return Err(NetworkError::ProtocolMismatch {
                local: local.protocol_version,
                remote: remote.protocol_version,
            });
        }

        // The vault schema is deliberately not checked at the transport: the
        // pairing protocol exchanges it and must report the mismatch as its
        // own distinct failure before any secret key moves.

        let pipe = InMemoryPipe {
            initiator: self.device_id,
            to_responder: Arc::new(Mutex::new(VecDeque::new())),
            to_initiator: Arc::new(Mutex::new(VecDeque::new())),
            closed: Arc::new(AtomicBool::new(false)),
        };

        let channel = InMemoryChannel {
            peer_device_id: peer_id,
            outbound: Arc::clone(&pipe.to_responder),
            inbound: Arc::clone(&pipe.to_initiator),
            closed: Arc::clone(&pipe.closed),
        };

        devices
            .get_mut(&peer_id)
            .expect("peer checked above")
            .pairing_inbound
            .push_back(pipe);

        Ok(Box::new(channel))
    }

    fn accept_for_pairing(
        &self,
        claim: &PairingClaim,
    ) -> Result<Option<Box<dyn PeerChannel>>, NetworkError> {
        let mut devices = self.hub.devices.lock().unwrap();
        let local = devices
            .get_mut(&self.device_id)
            .ok_or_else(|| NetworkError::DiscoveryFailed("device is not registered".into()))?;

        let authenticated = local
            .pairing_claim
            .as_ref()
            .is_some_and(|expected| expected.matches_claim(claim));
        if !authenticated {
            return Err(NetworkError::AuthenticationFailed);
        }

        Ok(local.pairing_inbound.pop_front().map(|pipe| {
            Box::new(InMemoryChannel {
                peer_device_id: pipe.initiator,
                outbound: Arc::clone(&pipe.to_initiator),
                inbound: Arc::clone(&pipe.to_responder),
                closed: pipe.closed,
            }) as Box<dyn PeerChannel>
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discovery_yields_only_an_opaque_device_id_and_address() {
        let hub = InMemoryHub::new();
        let a_id = uuid::Uuid::new_v4();
        let b_id = uuid::Uuid::new_v4();

        let a = hub.join(InMemoryDeviceConfig::new(a_id, 3));
        let b = hub.join(InMemoryDeviceConfig::new(b_id, 3));
        b.start_advertising().unwrap();

        let peers = a.discover().unwrap();
        assert_eq!(peers.len(), 1);

        let peer = &peers[0];
        assert_eq!(peer.device_id(), b_id);

        let advertisement = peer.advertisement();
        assert_eq!(advertisement.service_type(), SYNC_SERVICE_TYPE);
        assert_eq!(advertisement.device_id(), b_id);

        // The advertised record carries a service type and an opaque device
        // id, and nothing derived from the User or their Vaults.
        let record = serde_json::to_value(advertisement).unwrap();
        let keys: Vec<&String> = record.as_object().unwrap().keys().collect();
        assert_eq!(keys, vec!["device_id", "service_type"]);

        // The address is carried for connect() but never exposed to the
        // sync engine, not even through Debug.
        assert!(!format!("{:?}", peer).contains("in-memory://"));
    }

    #[test]
    fn test_unpaired_peer_is_discoverable_but_cannot_be_synced_with() {
        let hub = InMemoryHub::new();
        let a_id = uuid::Uuid::new_v4();
        let c_id = uuid::Uuid::new_v4();

        let a = hub.join(InMemoryDeviceConfig::new(a_id, 3));
        let c = hub.join(InMemoryDeviceConfig::new(c_id, 3));
        c.start_advertising().unwrap();

        // Discovery is a convenience, never a grant of access.
        let discovered = a.discover().unwrap();
        assert_eq!(discovered.len(), 1);
        assert_eq!(discovered[0].device_id(), c_id);

        let result = a.connect(&discovered[0]);

        assert_eq!(result.err(), Some(NetworkError::AuthenticationFailed));
        assert!(c.accept().unwrap().is_none());
    }

    #[test]
    fn test_unreachable_paired_peer_is_reported_distinctly() {
        let hub = InMemoryHub::new();
        let a_id = uuid::Uuid::new_v4();
        let b_id = uuid::Uuid::new_v4();

        let a = hub.join(InMemoryDeviceConfig::new(a_id, 3).paired_with([b_id]));
        let b = hub.join(
            InMemoryDeviceConfig::new(b_id, 3)
                .paired_with([a_id])
                .unreachable(),
        );
        b.start_advertising().unwrap();

        let peer = a.discover().unwrap().remove(0);
        assert_eq!(
            a.connect(&peer).err(),
            Some(NetworkError::PeerUnreachable(b_id.to_string()))
        );
    }

    #[test]
    fn test_connect_refuses_a_peer_on_another_protocol_version() {
        let hub = InMemoryHub::new();
        let a_id = uuid::Uuid::new_v4();
        let b_id = uuid::Uuid::new_v4();

        let a = hub.join(InMemoryDeviceConfig::new(a_id, 3).paired_with([b_id]));
        let b = hub.join(
            InMemoryDeviceConfig::new(b_id, 3)
                .paired_with([a_id])
                .with_protocol_version(SYNC_PROTOCOL_VERSION + 1),
        );
        b.start_advertising().unwrap();

        let peer = a.discover().unwrap().remove(0);
        assert_eq!(
            a.connect(&peer).err(),
            Some(NetworkError::ProtocolMismatch {
                local: SYNC_PROTOCOL_VERSION,
                remote: SYNC_PROTOCOL_VERSION + 1,
            })
        );
    }

    #[test]
    fn test_connect_refuses_a_peer_on_another_vault_schema() {
        let hub = InMemoryHub::new();
        let a_id = uuid::Uuid::new_v4();
        let b_id = uuid::Uuid::new_v4();

        let a = hub.join(InMemoryDeviceConfig::new(a_id, 3).paired_with([b_id]));
        let b = hub.join(InMemoryDeviceConfig::new(b_id, 4).paired_with([a_id]));
        b.start_advertising().unwrap();

        let peer = a.discover().unwrap().remove(0);
        assert_eq!(
            a.connect(&peer).err(),
            Some(NetworkError::SchemaMismatch {
                local: 3,
                remote: 4
            })
        );
    }

    #[test]
    fn test_two_managers_exchange_an_encrypted_vault_blob_and_registry_state() {
        use crate::vault_manager::VaultManager;
        use crate::{NewItemData, NoteData};

        const MASTER_PW: &str = "SuperSecret123";
        const KEY: [u8; 32] = [0u8; 32];

        let hub = InMemoryHub::new();
        let a_id = uuid::Uuid::new_v4();
        let b_id = uuid::Uuid::new_v4();
        let a_net = hub.join(InMemoryDeviceConfig::new(a_id, 3).paired_with([b_id]));
        let b_net = hub.join(InMemoryDeviceConfig::new(b_id, 3).paired_with([a_id]));
        b_net.start_advertising().unwrap();

        let a_dir = std::env::temp_dir().join(format!("lockr_net_a_{}", uuid::Uuid::new_v4()));
        let b_dir = std::env::temp_dir().join(format!("lockr_net_b_{}", uuid::Uuid::new_v4()));
        let mut a = VaultManager::new(a_dir.clone()).unwrap();
        let mut b = VaultManager::new(b_dir.clone()).unwrap();

        let vault = a
            .create_vault(
                "Personal".to_string(),
                "#6240BF".to_string(),
                MASTER_PW,
                &KEY,
            )
            .unwrap();
        a.add_item(
            vault.id,
            Some("Notebook".to_string()),
            None,
            NewItemData::Note(NoteData {
                title: Some("Recovery codes".to_string()),
                body: "one two three".to_string(),
            }),
            None,
        )
        .unwrap();

        let encrypted =
            std::fs::read(a_dir.join("vaults").join(format!("{}.vault", vault.id))).unwrap();

        let peer = a_net.discover().unwrap().remove(0);
        let mut initiator = a_net.connect(&peer).unwrap();
        let mut responder = b_net.accept().unwrap().unwrap();
        assert_eq!(initiator.peer_device_id(), b_id);
        assert_eq!(responder.peer_device_id(), a_id);

        initiator
            .send(SyncMessage::Vault {
                vault_id: vault.id,
                blob: EncryptedVaultBlob::from_encrypted_bytes(encrypted.clone()),
            })
            .unwrap();

        let received = match responder.receive().unwrap() {
            SyncMessage::Vault { vault_id, blob } => {
                assert_eq!(vault_id, vault.id);
                blob.as_bytes().to_vec()
            }
            other => panic!("expected a vault blob, got {other:?}"),
        };

        // The blob arriving over the seam is the encrypted file, and it is
        // enough for the receiving manager to unlock the same vault.
        std::fs::write(
            b_dir.join("vaults").join(format!("{}.vault", vault.id)),
            &received,
        )
        .unwrap();
        let unlocked = b.unlock_vault(vault.id, MASTER_PW, &KEY).unwrap();
        assert_eq!(unlocked.items.len(), 1);
        assert_eq!(unlocked.items[0].display_name.as_deref(), Some("Notebook"));

        // Registry state travels the other way over the same channel.
        let registry = RegistryState::from_bytes(br#"{"devices":[]}"#.to_vec());
        responder
            .send(SyncMessage::Registry(registry.clone()))
            .unwrap();
        assert_eq!(
            initiator.receive().unwrap(),
            SyncMessage::Registry(registry)
        );

        responder.send(SyncMessage::Complete).unwrap();
        assert_eq!(initiator.receive().unwrap(), SyncMessage::Complete);

        std::fs::remove_dir_all(a_dir).ok();
        std::fs::remove_dir_all(b_dir).ok();
    }

    #[test]
    fn test_registry_state_carries_both_registries_and_nothing_else() {
        use crate::registry::{SharedRegistries, VaultRegistry};
        use crate::DeviceRegistry;

        let vault_id = uuid::Uuid::new_v4();
        let mut vaults = VaultRegistry::new();
        vaults.insert(vault_id);

        let registries = SharedRegistries {
            devices: DeviceRegistry::new(),
            vaults,
        };

        let state = RegistryState::from_registries(&registries).unwrap();
        assert_eq!(state.to_registries().unwrap(), registries);

        let wire = String::from_utf8(state.as_bytes().to_vec()).unwrap();
        assert!(wire.contains("devices"));
        assert!(wire.contains("vaults"));
        assert!(
            !wire.contains("password") && !wire.contains("secret"),
            "registry state must never carry secret material"
        );

        // The opaque blob is rejected if a peer sends something else.
        assert!(RegistryState::from_bytes(b"not registry state".to_vec())
            .to_registries()
            .is_err());
    }

    #[test]
    fn test_frame_roundtrips_a_pairing_message() {
        use crate::pairing::{PairingMessage, PairingRequest};

        let message = SyncMessage::Pairing(PairingMessage::Join(PairingRequest {
            device_id: uuid::Uuid::new_v4(),
            name: "New Laptop".to_string(),
            platform: "windows".to_string(),
            public_key: "public-key".to_string(),
            schema_version: 3,
        }));

        let frame = encode_frame(&message).unwrap();
        assert!(frame.starts_with(b"LKRS"));
        assert_eq!(decode_frame(&frame).unwrap(), message);
    }

    #[test]
    fn test_a_pairing_channel_requires_the_one_time_secret() {
        let hub = InMemoryHub::new();
        let a_id = uuid::Uuid::new_v4();
        let b_id = uuid::Uuid::new_v4();
        let a = hub.join(InMemoryDeviceConfig::new(a_id, 3));
        let b = hub.join(InMemoryDeviceConfig::new(b_id, 3));
        b.start_advertising().unwrap();

        let offer =
            crate::pairing::PairingHost::start(a_id, a_id, 3, [1u8; 32], chrono::Utc::now()).1;
        let correct = offer.claim();
        let wrong = crate::pairing::PairingCode::typed(
            a_id,
            crate::pairing::PairingSecret::generate().typed_code(),
        )
        .claim()
        .unwrap();

        let peer = a.discover().unwrap().remove(0);

        // Nothing is registered to accept pairing yet.
        assert_eq!(
            a.connect_for_pairing(&peer, &correct).err(),
            Some(NetworkError::AuthenticationFailed)
        );

        b.begin_pairing(&correct).unwrap();
        assert_eq!(
            a.connect_for_pairing(&peer, &wrong).err(),
            Some(NetworkError::AuthenticationFailed),
            "a different one-time secret must not authenticate"
        );
        assert!(a.connect_for_pairing(&peer, &correct).is_ok());
    }

    #[test]
    fn test_frame_roundtrips_vault_registry_and_complete_messages() {
        let vault_id = uuid::Uuid::new_v4();
        let messages = [
            SyncMessage::Vault {
                vault_id,
                blob: EncryptedVaultBlob::from_encrypted_bytes(vec![0, 1, 2, 255]),
            },
            SyncMessage::Registry(RegistryState::from_bytes(br#"{"devices":[]}"#.to_vec())),
            SyncMessage::Complete,
        ];

        for message in messages {
            let frame = encode_frame(&message).unwrap();
            assert!(frame.starts_with(b"LKRS"));
            assert_eq!(
                decode_frame(&frame).unwrap(),
                message,
                "roundtrip failed for {message:?}"
            );
        }
    }

    #[test]
    fn test_a_foreign_protocol_version_is_rejected_distinctly() {
        let mut frame = encode_frame(&SyncMessage::Complete).unwrap();
        frame[4..6].copy_from_slice(&(SYNC_PROTOCOL_VERSION + 1).to_be_bytes());

        assert_eq!(
            decode_frame(&frame),
            Err(NetworkError::ProtocolMismatch {
                local: SYNC_PROTOCOL_VERSION,
                remote: SYNC_PROTOCOL_VERSION + 1,
            })
        );
    }

    #[test]
    fn test_a_malformed_frame_is_rejected() {
        assert!(matches!(
            decode_frame(b"not a Lockr frame"),
            Err(NetworkError::MalformedFrame(_))
        ));

        let frame = encode_frame(&SyncMessage::Complete).unwrap();
        let truncated = &frame[..frame.len() - 1];
        assert!(matches!(
            decode_frame(truncated),
            Err(NetworkError::MalformedFrame(_))
        ));
    }

    /// A transport that forgot to implement a deadline. Its unbounded
    /// `receive` must never be reached through `receive_timeout`.
    struct ForgetsDeadlineChannel;

    impl PeerChannel for ForgetsDeadlineChannel {
        fn peer_device_id(&self) -> DeviceId {
            uuid::Uuid::new_v4()
        }

        fn send(&mut self, _message: SyncMessage) -> Result<(), NetworkError> {
            Ok(())
        }

        fn receive(&mut self) -> Result<SyncMessage, NetworkError> {
            panic!("receive_timeout must not fall back to an unbounded receive")
        }

        fn close(&mut self) {}
    }

    #[test]
    fn test_receive_timeout_fails_closed_when_a_transport_forgets_its_deadline() {
        let mut channel = ForgetsDeadlineChannel;

        assert_eq!(
            channel.receive_timeout(Duration::from_millis(10)),
            Err(NetworkError::Timeout),
            "a transport without a deadline must fail, not block forever"
        );
    }

    #[test]
    fn test_advertising_stops_when_the_session_ends() {
        let hub = InMemoryHub::new();
        let a_id = uuid::Uuid::new_v4();
        let b_id = uuid::Uuid::new_v4();

        let a = hub.join(InMemoryDeviceConfig::new(a_id, 3));
        let b = hub.join(InMemoryDeviceConfig::new(b_id, 3));

        b.start_advertising().unwrap();
        assert_eq!(a.discover().unwrap().len(), 1);

        b.stop_advertising().unwrap();
        assert!(a.discover().unwrap().is_empty());
    }
}
