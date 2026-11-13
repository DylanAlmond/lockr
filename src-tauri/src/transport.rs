//! The production peer transport: TLS 1.3 over a local TCP connection.
//!
//! This is the other half of the [`PeerNetwork`] seam. Discovery is mDNS
//! ([`crate::mdns`]); the connection it hands back is opened, encrypted, and
//! authenticated here. The sync and pairing engines are unchanged: they still
//! see only an opaque [`DiscoveredPeer`] and a [`PeerChannel`].
//!
//! # Why the certificate is not the identity
//!
//! Both sides generate an ephemeral self-signed certificate for the session
//! ([`rcgen`]) and present it. Rustls is configured to *not* use certificate
//! verification as an identity decision: the client's [`ServerCertVerifier`]
//! accepts any chain and host name, and the server's [`ClientCertVerifier`]
//! accepts any client certificate.
//!
//! That is deliberate and safe **only because identity is proven separately**,
//! after the handshake, over the encrypted session:
//!
//! * a paired channel proves possession of the Device's Ed25519 signing key,
//!   checked against the public key in the Device Registry (which is the
//!   source of truth for admission and revocation); and
//! * a pairing channel proves possession of the one-time pairing secret.
//!
//! Both proofs are bound to keying material exported from *this* TLS session
//! (`export_keying_material`) and to fresh nonces from both sides. A
//! machine-in-the-middle that terminates TLS has different exported material
//! on each leg, so it cannot relay a proof from one leg to the other, and a
//! proof captured from an earlier session cannot be replayed into a later one.
//!
//! The protocol version is pinned to TLS 1.3 on both sides
//! (`with_protocol_versions(&[&TLS13])`); the build has no TLS 1.2 support
//! compiled in, and the handshake is additionally refused unless the
//! negotiated version is exactly TLS 1.3. There is no plaintext path and no
//! "if the handshake fails, carry on": every failure aborts the connection.
//!
//! # Deadlines
//!
//! Every connection has a handshake deadline and per-read/per-write
//! deadlines, so a peer that accepts a connection and then goes silent cannot
//! hold a worker indefinitely. [`PeerChannel::receive_timeout`] tightens the
//! read deadline for one call.

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::Duration;

use argon2::password_hash::rand_core::{OsRng, RngCore};
use ring::hmac;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName};
use rustls::{
    ClientConfig, ClientConnection, ProtocolVersion, ServerConfig, ServerConnection, StreamOwned,
};
use uuid::Uuid;

use crate::device::{verify_signature, DeviceKeypair};
use crate::mdns::MdnsDiscovery;
use crate::network::{
    decode_frame, frame_payload_len, DeviceDirectory, DiscoveredPeer, NetworkError, PeerChannel,
    PeerNetwork, SyncMessage, TransportSession, HEADER_LEN,
};
use crate::pairing::PairingClaim;
use crate::DeviceId;

/// ALPN identifiers separate the two things that can arrive on one port. A
/// connection that offers neither is refused before any handshake proof is
/// read.
const ALPN_SYNC: &[u8] = b"lockr-sync-v1";
const ALPN_PAIR: &[u8] = b"lockr-pair-v1";

/// RFC 5705 / RFC 8446 exporter label and context. Both sides must use the
/// same values; the derived bytes are the session binding every proof covers.
const EXPORTER_LABEL: &[u8] = b"EXPORTER-lockr-peer-v1";
const EXPORTER_CONTEXT: &[u8] = b"lockr peer authentication v1";

/// Domain tags. Distinct tags for the two directions of each exchange mean a
/// client proof can never be turned into a server proof.
const CLIENT_SIGN_DOMAIN: &[u8] = b"lockr.peer-auth.client.v1";
const SERVER_SIGN_DOMAIN: &[u8] = b"lockr.peer-auth.server.v1";
const PAIR_KEY_DOMAIN: &[u8] = b"lockr.pair.key.v1";
const PAIR_CLIENT_DOMAIN: &[u8] = b"lockr.pair.client.v1";
const PAIR_SERVER_DOMAIN: &[u8] = b"lookr.pair.server.v1";

/// Fixed-size auth messages. Every field has a known length, so the signed
/// payload is unambiguous without length prefixes.
const MSG_CLIENT_HELLO: u8 = 0x01;
const MSG_SERVER_HELLO: u8 = 0x02;
const MSG_CLIENT_AUTH: u8 = 0x03;
const MSG_SERVER_AUTH: u8 = 0x04;
const MSG_REJECT: u8 = 0x05;
const MSG_PAIR_CLIENT_HELLO: u8 = 0x11;
const MSG_PAIR_SERVER_HELLO: u8 = 0x12;
const MSG_PAIR_CLIENT_AUTH: u8 = 0x13;
const MSG_PAIR_SERVER_AUTH: u8 = 0x14;

const NONCE_LEN: usize = 32;
const TAG_LEN: usize = 32;
/// Base64 of a 64-byte Ed25519 signature, padded, as
/// [`DeviceKeypair::sign`] returns it.
const SIGNATURE_LEN: usize = 88;
const DEVICE_ID_LEN: usize = 16;

const SYNC_HELLO_LEN: usize = 1 + NONCE_LEN;
const PAIR_HELLO_LEN: usize = 1 + 1 + NONCE_LEN;
const SYNC_AUTH_LEN: usize = 1 + DEVICE_ID_LEN + SIGNATURE_LEN;
const PAIR_AUTH_LEN: usize = 1 + TAG_LEN;

/// Payload sizes, excluding the one-byte message tag.
const SYNC_HELLO_PAYLOAD: usize = NONCE_LEN;
const PAIR_HELLO_PAYLOAD: usize = 1 + NONCE_LEN;
const SYNC_AUTH_PAYLOAD: usize = DEVICE_ID_LEN + SIGNATURE_LEN;
const PAIR_AUTH_PAYLOAD: usize = TAG_LEN;

/// How often the accept loop wakes to check whether the session ended. Only
/// while a session is open; the thread exits with the session.
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(20);

/// A ceiling on connections being authenticated at once, so a peer cannot
/// spawn unbounded worker threads. Channels that finish authenticating are
/// handed to a bounded queue, so the ceiling covers both.
const MAX_PENDING_CONNECTIONS: usize = 16;

/// Timeouts for one transport. The defaults are deliberately generous enough
/// for a slow machine and short enough that a dead peer is noticed.
#[derive(Debug, Clone)]
pub struct TlsTransportConfig {
    /// From TCP connect through the authentication exchange.
    pub handshake_timeout: Duration,
    /// A normal frame read/write with no explicit deadline.
    pub frame_timeout: Duration,
    /// How long `accept` waits for an inbound channel before returning `None`.
    pub accept_wait: Duration,
}

impl Default for TlsTransportConfig {
    fn default() -> Self {
        Self {
            handshake_timeout: Duration::from_secs(10),
            frame_timeout: Duration::from_secs(30),
            accept_wait: Duration::from_millis(250),
        }
    }
}

/// One session's certificate and the rustls configurations built from it.
struct TlsConfigs {
    server: Arc<ServerConfig>,
    client: Arc<ClientConfig>,
}

/// Generate a fresh ephemeral certificate for a session and the TLS 1.3-only
/// configurations that present it.
fn generate_tls_configs() -> Result<TlsConfigs, NetworkError> {
    let certified = rcgen::generate_simple_self_signed(vec!["lockr.invalid".to_string()])
        .map_err(|err| transport_error(format!("ephemeral certificate failed: {err}")))?;
    let certificate: CertificateDer<'static> = certified.cert.der().clone();
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(certified.signing_key.serialize_der()));

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let versions: &[&'static rustls::SupportedProtocolVersion] = &[&rustls::version::TLS13];

    let mut server = ServerConfig::builder_with_provider(Arc::clone(&provider))
        .with_protocol_versions(versions)
        .map_err(tls_error)?
        // Certificate verification is NOT the identity mechanism. See the
        // module documentation: identity is proven after the handshake, bound
        // to this session's exported keying material.
        .with_client_cert_verifier(Arc::new(AcceptAnyCertificate))
        .with_single_cert(vec![certificate.clone()], key.clone_key())
        .map_err(tls_error)?;
    server.alpn_protocols = vec![ALPN_SYNC.to_vec(), ALPN_PAIR.to_vec()];

    let mut client = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(versions)
        .map_err(tls_error)?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAnyCertificate))
        .with_client_auth_cert(vec![certificate], key)
        .map_err(tls_error)?;
    client.alpn_protocols = vec![ALPN_SYNC.to_vec(), ALPN_PAIR.to_vec()];

    Ok(TlsConfigs {
        server: Arc::new(server),
        client: Arc::new(client),
    })
}

fn transport_error(message: String) -> NetworkError {
    NetworkError::TransportUnavailable(message)
}

fn tls_error(err: rustls::Error) -> NetworkError {
    transport_error(format!("TLS could not be configured: {err}"))
}

/// A deliberately permissive certificate verifier.
///
/// The certificate is a container for a throwaway key, not a credential. The
/// peer's identity is the Ed25519 key or pairing secret proven over the
/// encrypted session, so accepting any certificate here cannot grant access.
/// The *handshake signature* is still verified, so the peer has to possess the
/// key in the certificate it presented, and only TLS 1.3 signature schemes are
/// accepted.
#[derive(Debug)]
struct AcceptAnyCertificate;

impl rustls::client::danger::ServerCertVerifier for AcceptAnyCertificate {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General(
            "TLS 1.2 is not supported by the Lockr peer transport".into(),
        ))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

impl rustls::server::danger::ClientCertVerifier for AcceptAnyCertificate {
    fn offer_client_auth(&self) -> bool {
        true
    }

    fn client_auth_mandatory(&self) -> bool {
        true
    }

    fn root_hint_subjects(&self) -> &[rustls::DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::server::danger::ClientCertVerified, rustls::Error> {
        Ok(rustls::server::danger::ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General(
            "TLS 1.2 is not supported by the Lockr peer transport".into(),
        ))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// The Device Registry, seen through the one question the transport may ask.
impl DeviceDirectory for crate::registry::DeviceRegistry {
    fn admitted_public_key(&self, device_id: &DeviceId) -> Option<String> {
        self.get(device_id)
            .filter(|record| !record.is_revoked())
            .map(|record| record.public_key.clone())
    }
}

/// Everything a connection worker needs at the moment it accepts a socket.
#[derive(Clone, Default)]
struct SessionSnapshot {
    keypair: Option<Arc<DeviceKeypair>>,
    devices: Option<Arc<dyn DeviceDirectory>>,
    pairing_claim: Option<PairingClaim>,
}

struct SessionState {
    keypair: Option<Arc<DeviceKeypair>>,
    devices: Option<Arc<dyn DeviceDirectory>>,
    pairing_claim: Option<PairingClaim>,
    /// Failed pairing authentications against the registered claim. The
    /// application-level pairing host has the authoritative attempt budget;
    /// the transport needs its own because otherwise an attacker could brute
    /// force the 40-bit typed fallback without ever reaching that code.
    pairing_failures: u32,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            keypair: None,
            devices: None,
            pairing_claim: None,
            pairing_failures: 0,
        }
    }
}

impl SessionState {
    fn snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            keypair: self.keypair.clone(),
            devices: self.devices.clone(),
            pairing_claim: self.pairing_claim.clone(),
        }
    }

    fn record_pairing_failure(&mut self) {
        self.pairing_failures = self.pairing_failures.saturating_add(1);
        if self.pairing_failures >= crate::pairing::PAIRING_MAX_ATTEMPTS {
            // Burn the transport's copy of the secret. The application host
            // will separately refuse further joins on its own budget.
            self.pairing_claim = None;
        }
    }
}

/// Both halves of a TLS stream, so one channel type can own either.
enum TlsStream {
    Client(StreamOwned<ClientConnection, TcpStream>),
    Server(StreamOwned<ServerConnection, TcpStream>),
}

impl TlsStream {
    fn socket(&self) -> &TcpStream {
        match self {
            Self::Client(stream) => &stream.sock,
            Self::Server(stream) => &stream.sock,
        }
    }

    fn protocol_version(&self) -> Option<ProtocolVersion> {
        match self {
            Self::Client(stream) => stream.conn.protocol_version(),
            Self::Server(stream) => stream.conn.protocol_version(),
        }
    }

    fn alpn_protocol(&self) -> Option<&[u8]> {
        match self {
            Self::Client(stream) => stream.conn.alpn_protocol(),
            Self::Server(stream) => stream.conn.alpn_protocol(),
        }
    }

    /// Exported keying material from this exact TLS session.
    fn exporter(&self) -> Result<[u8; 32], NetworkError> {
        let mut material = [0u8; 32];
        let result = match self {
            Self::Client(stream) => stream.conn.export_keying_material(
                &mut material,
                EXPORTER_LABEL,
                Some(EXPORTER_CONTEXT),
            ),
            Self::Server(stream) => stream.conn.export_keying_material(
                &mut material,
                EXPORTER_LABEL,
                Some(EXPORTER_CONTEXT),
            ),
        };
        result.map_err(|_| NetworkError::AuthenticationFailed)?;
        Ok(material)
    }

    fn shutdown(&mut self) {
        match self {
            Self::Client(stream) => {
                stream.conn.send_close_notify();
                let _ = stream.flush();
                let _ = stream.sock.shutdown(Shutdown::Both);
            }
            Self::Server(stream) => {
                stream.conn.send_close_notify();
                let _ = stream.flush();
                let _ = stream.sock.shutdown(Shutdown::Both);
            }
        }
    }
}

impl Read for TlsStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Client(stream) => stream.read(buf),
            Self::Server(stream) => stream.read(buf),
        }
    }
}

impl Write for TlsStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Client(stream) => stream.write(buf),
            Self::Server(stream) => stream.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Client(stream) => stream.flush(),
            Self::Server(stream) => stream.flush(),
        }
    }
}

/// An authenticated channel. Everything above it sees frames; everything
/// below is TLS.
struct TlsChannel {
    stream: Option<TlsStream>,
    peer: DeviceId,
    frame_timeout: Duration,
}

impl TlsChannel {
    fn new(stream: TlsStream, peer: DeviceId, frame_timeout: Duration) -> Self {
        // From here on the whole connection has per-frame deadlines. The
        // handshake deadline has already served its purpose.
        let _ = stream.socket().set_read_timeout(Some(frame_timeout));
        let _ = stream.socket().set_write_timeout(Some(frame_timeout));
        Self {
            stream: Some(stream),
            peer,
            frame_timeout,
        }
    }

    fn stream(&mut self) -> Result<&mut TlsStream, NetworkError> {
        self.stream.as_mut().ok_or(NetworkError::ConnectionClosed)
    }
}

impl PeerChannel for TlsChannel {
    fn peer_device_id(&self) -> DeviceId {
        self.peer
    }

    fn send(&mut self, message: SyncMessage) -> Result<(), NetworkError> {
        let frame = crate::network::encode_frame(&message)?;
        write_all(self.stream()?, &frame)
    }

    fn receive(&mut self) -> Result<SyncMessage, NetworkError> {
        read_frame(self.stream()?)
    }

    fn receive_timeout(&mut self, timeout: Duration) -> Result<SyncMessage, NetworkError> {
        let frame_timeout = self.frame_timeout;
        let stream = self.stream()?;
        stream
            .socket()
            .set_read_timeout(Some(timeout))
            .map_err(map_io_error)?;
        let result = read_frame(stream);
        // Restore the connection's normal deadline even when the call failed.
        let _ = stream.socket().set_read_timeout(Some(frame_timeout));
        result
    }

    fn close(&mut self) {
        if let Some(mut stream) = self.stream.take() {
            stream.shutdown();
        }
    }
}

// ============ Framing ============

fn write_all<W: Write>(writer: &mut W, bytes: &[u8]) -> Result<(), NetworkError> {
    writer.write_all(bytes).map_err(map_io_error)
}

fn read_exact<R: Read>(reader: &mut R, buffer: &mut [u8]) -> Result<(), NetworkError> {
    reader.read_exact(buffer).map_err(map_io_error)
}

fn read_frame<R: Read>(reader: &mut R) -> Result<SyncMessage, NetworkError> {
    let mut header = [0u8; HEADER_LEN];
    read_exact(reader, &mut header)?;
    // The length is validated against magic, protocol version, and ceiling
    // before a payload buffer is allocated.
    let payload_len = frame_payload_len(&header)?;
    let mut frame = Vec::with_capacity(HEADER_LEN + payload_len);
    frame.extend_from_slice(&header);
    frame.resize(HEADER_LEN + payload_len, 0);
    read_exact(reader, &mut frame[HEADER_LEN..])?;
    decode_frame(&frame)
}

fn map_io_error(error: io::Error) -> NetworkError {
    match error.kind() {
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => NetworkError::Timeout,
        _ => NetworkError::ConnectionClosed,
    }
}

fn map_handshake_error(error: io::Error) -> NetworkError {
    match error.kind() {
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => NetworkError::Timeout,
        _ => NetworkError::AuthenticationFailed,
    }
}

// ============ Authentication ============

fn random_nonce() -> [u8; NONCE_LEN] {
    let mut nonce = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);
    nonce
}

/// `domain || exporter || nonce_c || nonce_s`.
///
/// Every component has a fixed length and a distinct domain tag, so the
/// encoding is unambiguous and a proof for one role or one protocol can never
/// be presented as another.
fn signed_payload(
    domain: &[u8],
    exporter: &[u8; 32],
    nonce_c: &[u8; NONCE_LEN],
    nonce_s: &[u8; NONCE_LEN],
) -> Vec<u8> {
    let mut payload = Vec::with_capacity(domain.len() + 64);
    payload.extend_from_slice(domain);
    payload.extend_from_slice(exporter);
    payload.extend_from_slice(nonce_c);
    payload.extend_from_slice(nonce_s);
    payload
}

fn pairing_tag_payload(domain: &[u8], nonce_c: &[u8; NONCE_LEN], nonce_s: &[u8; NONCE_LEN]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(domain.len() + 64);
    payload.extend_from_slice(domain);
    payload.extend_from_slice(nonce_c);
    payload.extend_from_slice(nonce_s);
    payload
}

/// Derive the HMAC key used for pairing confirmations.
///
/// A channel is only as strong as the material both sides can actually prove.
/// The typed fallback carries 40 bits, so whenever either side presents it
/// both sides truncate to that prefix. The pairing host's short TTL and
/// attempt budget, plus the transport's own failure budget, are what make the
/// short form acceptable; this function must never silently claim the full
/// secret was proven.
fn pairing_key(material: &[u8], full_secret: bool, exporter: &[u8; 32]) -> hmac::Key {
    let key_bytes = if full_secret {
        material
    } else {
        &material[..5.min(material.len())]
    };
    let base = hmac::Key::new(hmac::HMAC_SHA256, key_bytes);
    let mut data = Vec::with_capacity(PAIR_KEY_DOMAIN.len() + 32);
    data.extend_from_slice(PAIR_KEY_DOMAIN);
    data.extend_from_slice(exporter);
    let derived = hmac::sign(&base, &data);
    hmac::Key::new(hmac::HMAC_SHA256, derived.as_ref())
}

fn pairing_confirmation(
    key: &hmac::Key,
    domain: &[u8],
    nonce_c: &[u8; NONCE_LEN],
    nonce_s: &[u8; NONCE_LEN],
) -> hmac::Tag {
    hmac::sign(key, &pairing_tag_payload(domain, nonce_c, nonce_s))
}

/// Refuse a connection that reached TLS but failed authentication.
///
/// Anything the peer already sent is drained first: closing a socket that
/// still has unread input can make the operating system send a reset, which
/// would discard the refusal and leave the peer reporting a bare connection
/// close instead of an authentication failure. The drain is bounded, so a
/// peer cannot keep a worker busy by streaming data.
fn refuse_connection(stream: &mut TlsStream) {
    let _ = stream
        .socket()
        .set_read_timeout(Some(Duration::from_millis(100)));
    let mut buffer = [0u8; 512];
    let mut drained = 0usize;
    while drained < 8192 {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => drained += read,
        }
    }

    let _ = stream.write_all(&[MSG_REJECT]);
    let _ = stream.flush();
    stream.shutdown();
}

/// Read one fixed-size auth message and check its tag. `MSG_REJECT` and any
/// unexpected tag are both an authentication failure, never a decode error:
/// an unauthenticated peer is not allowed to say anything else.
fn expect_tag<R: Read>(
    reader: &mut R,
    expected: u8,
    payload_len: usize,
) -> Result<Vec<u8>, NetworkError> {
    let mut tag = [0u8; 1];
    read_exact(reader, &mut tag)?;
    if tag[0] == MSG_REJECT {
        return Err(NetworkError::AuthenticationFailed);
    }
    if tag[0] != expected {
        return Err(NetworkError::AuthenticationFailed);
    }
    let mut payload = vec![0u8; payload_len];
    read_exact(reader, &mut payload)?;
    Ok(payload)
}

fn nonce_from(payload: &[u8]) -> [u8; NONCE_LEN] {
    let mut nonce = [0u8; NONCE_LEN];
    nonce.copy_from_slice(&payload[..NONCE_LEN]);
    nonce
}

fn read_device_id(bytes: &[u8]) -> Result<DeviceId, NetworkError> {
    Uuid::from_slice(bytes).map_err(|_| NetworkError::AuthenticationFailed)
}

/// Client side of the paired-channel handshake.
///
/// The client sends its proof before it has verified the server, while the
/// server answers with its own proof only after verifying the client. That
/// ordering reveals nothing secret: the proof is a signature over fresh
/// session material, not the key itself, and the client verifies the server's
/// identity and registry admission before any channel is returned.
fn authenticate_client_sync(
    stream: &mut TlsStream,
    session: &SessionSnapshot,
    local_device: DeviceId,
    expected_peer: DeviceId,
    client_auth_override: Option<&[u8]>,
) -> Result<(DeviceId, Vec<u8>), NetworkError> {
    let keypair = session
        .keypair
        .as_ref()
        .ok_or(NetworkError::AuthenticationFailed)?;
    let devices = session
        .devices
        .as_ref()
        .ok_or(NetworkError::AuthenticationFailed)?;
    let exporter = stream.exporter()?;
    let nonce_c = random_nonce();

    let mut hello = Vec::with_capacity(SYNC_HELLO_LEN);
    hello.push(MSG_CLIENT_HELLO);
    hello.extend_from_slice(&nonce_c);
    write_all(stream, &hello)?;

    let nonce_s = nonce_from(&expect_tag(stream, MSG_SERVER_HELLO, SYNC_HELLO_PAYLOAD)?);

    let sent_auth = match client_auth_override {
        Some(captured) => captured.to_vec(),
        None => {
            let payload = signed_payload(CLIENT_SIGN_DOMAIN, &exporter, &nonce_c, &nonce_s);
            let signature = keypair.sign(&payload);
            debug_assert_eq!(signature.len(), SIGNATURE_LEN);
            let mut auth = Vec::with_capacity(SYNC_AUTH_LEN);
            auth.push(MSG_CLIENT_AUTH);
            auth.extend_from_slice(local_device.as_bytes());
            auth.extend_from_slice(signature.as_bytes());
            auth
        }
    };
    write_all(stream, &sent_auth)?;

    let rest = expect_tag(stream, MSG_SERVER_AUTH, SYNC_AUTH_PAYLOAD)?;
    let server_device = read_device_id(&rest[..DEVICE_ID_LEN])?;
    if server_device != expected_peer {
        return Err(NetworkError::AuthenticationFailed);
    }

    let public_key = devices
        .admitted_public_key(&server_device)
        .ok_or(NetworkError::AuthenticationFailed)?;
    let signature = std::str::from_utf8(&rest[DEVICE_ID_LEN..])
        .map_err(|_| NetworkError::AuthenticationFailed)?;
    let payload = signed_payload(SERVER_SIGN_DOMAIN, &exporter, &nonce_c, &nonce_s);
    verify_signature(&public_key, &payload, signature)
        .map_err(|_| NetworkError::AuthenticationFailed)?;

    Ok((server_device, sent_auth))
}

/// Server side of the paired-channel handshake. It refuses an absent,
/// unknown, or revoked Device before it sends its own proof, and before the
/// caller ever sees a channel.
fn authenticate_server_sync(
    stream: &mut TlsStream,
    session: &SessionSnapshot,
    local_device: DeviceId,
) -> Result<DeviceId, NetworkError> {
    let keypair = session
        .keypair
        .as_ref()
        .ok_or(NetworkError::AuthenticationFailed)?;
    let devices = session
        .devices
        .as_ref()
        .ok_or(NetworkError::AuthenticationFailed)?;
    let exporter = stream.exporter()?;

    let nonce_c = nonce_from(&expect_tag(stream, MSG_CLIENT_HELLO, SYNC_HELLO_PAYLOAD)?);
    let nonce_s = random_nonce();
    let mut hello = Vec::with_capacity(SYNC_HELLO_LEN);
    hello.push(MSG_SERVER_HELLO);
    hello.extend_from_slice(&nonce_s);
    write_all(stream, &hello)?;

    let rest = expect_tag(stream, MSG_CLIENT_AUTH, SYNC_AUTH_PAYLOAD)?;
    let client_device = read_device_id(&rest[..DEVICE_ID_LEN])?;
    let public_key = devices
        .admitted_public_key(&client_device)
        .ok_or(NetworkError::AuthenticationFailed)?;
    let signature = std::str::from_utf8(&rest[DEVICE_ID_LEN..])
        .map_err(|_| NetworkError::AuthenticationFailed)?;
    let payload = signed_payload(CLIENT_SIGN_DOMAIN, &exporter, &nonce_c, &nonce_s);
    verify_signature(&public_key, &payload, signature)
        .map_err(|_| NetworkError::AuthenticationFailed)?;

    let server_payload = signed_payload(SERVER_SIGN_DOMAIN, &exporter, &nonce_c, &nonce_s);
    let signature = keypair.sign(&server_payload);
    let mut auth = Vec::with_capacity(SYNC_AUTH_LEN);
    auth.push(MSG_SERVER_AUTH);
    auth.extend_from_slice(local_device.as_bytes());
    auth.extend_from_slice(signature.as_bytes());
    write_all(stream, &auth)?;

    Ok(client_device)
}

/// Client side of the pairing handshake. Authentication is the one-time
/// secret, proven through the TLS exporter; the secret itself never crosses
/// the wire.
fn authenticate_client_pairing(
    stream: &mut TlsStream,
    claim: &PairingClaim,
    client_auth_override: Option<&[u8]>,
) -> Result<Vec<u8>, NetworkError> {
    let (material, client_is_full) = claim.transport_material();
    let exporter = stream.exporter()?;
    let nonce_c = random_nonce();

    let mut hello = Vec::with_capacity(PAIR_HELLO_LEN);
    hello.push(MSG_PAIR_CLIENT_HELLO);
    hello.push(u8::from(client_is_full));
    hello.extend_from_slice(&nonce_c);
    write_all(stream, &hello)?;

    let rest = expect_tag(stream, MSG_PAIR_SERVER_HELLO, PAIR_HELLO_PAYLOAD)?;
    let server_is_full = rest[0] == 1;
    let nonce_s = nonce_from(&rest[1..]);

    let full_secret = client_is_full && server_is_full;
    let key = pairing_key(material, full_secret, &exporter);

    let sent_auth = match client_auth_override {
        Some(captured) => captured.to_vec(),
        None => {
            let tag = pairing_confirmation(&key, PAIR_CLIENT_DOMAIN, &nonce_c, &nonce_s);
            let mut auth = Vec::with_capacity(PAIR_AUTH_LEN);
            auth.push(MSG_PAIR_CLIENT_AUTH);
            auth.extend_from_slice(tag.as_ref());
            auth
        }
    };
    write_all(stream, &sent_auth)?;

    let server_tag = expect_tag(stream, MSG_PAIR_SERVER_AUTH, PAIR_AUTH_PAYLOAD)?;
    hmac::verify(
        &key,
        &pairing_tag_payload(PAIR_SERVER_DOMAIN, &nonce_c, &nonce_s),
        &server_tag,
    )
    .map_err(|_| NetworkError::AuthenticationFailed)?;

    Ok(sent_auth)
}

/// Server side of the pairing handshake. The registered claim is the only
/// thing that can authenticate; if there is none, every pairing attempt is
/// refused.
fn authenticate_server_pairing(
    stream: &mut TlsStream,
    claim: &PairingClaim,
) -> Result<(), NetworkError> {
    let (material, server_is_full) = claim.transport_material();
    let exporter = stream.exporter()?;

    let rest = expect_tag(stream, MSG_PAIR_CLIENT_HELLO, PAIR_HELLO_PAYLOAD)?;
    let client_is_full = rest[0] == 1;
    let nonce_c = nonce_from(&rest[1..]);

    let nonce_s = random_nonce();
    let mut hello = Vec::with_capacity(PAIR_HELLO_LEN);
    hello.push(MSG_PAIR_SERVER_HELLO);
    hello.push(u8::from(server_is_full));
    hello.extend_from_slice(&nonce_s);
    write_all(stream, &hello)?;

    let full_secret = client_is_full && server_is_full;
    let key = pairing_key(material, full_secret, &exporter);

    let client_tag = expect_tag(stream, MSG_PAIR_CLIENT_AUTH, PAIR_AUTH_PAYLOAD)?;
    if hmac::verify(
        &key,
        &pairing_tag_payload(PAIR_CLIENT_DOMAIN, &nonce_c, &nonce_s),
        &client_tag,
    )
    .is_err()
    {
        return Err(NetworkError::AuthenticationFailed);
    }

    let tag = pairing_confirmation(&key, PAIR_SERVER_DOMAIN, &nonce_c, &nonce_s);
    let mut auth = Vec::with_capacity(PAIR_AUTH_LEN);
    auth.push(MSG_PAIR_SERVER_AUTH);
    auth.extend_from_slice(tag.as_ref());
    write_all(stream, &auth)
}

// ============ The network ============

struct Inbound {
    sync: Mutex<Option<Receiver<Box<dyn PeerChannel>>>>,
    pairing: Mutex<Option<Receiver<Box<dyn PeerChannel>>>>,
}

impl Default for Inbound {
    fn default() -> Self {
        Self {
            sync: Mutex::new(None),
            pairing: Mutex::new(None),
        }
    }
}

struct Running {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<()>,
    /// Kept alive for the session. Dropping the senders makes the receivers
    /// disconnect, which is exactly what stopping a session should do.
    _sync_sender: SyncSender<Box<dyn PeerChannel>>,
    _pairing_sender: SyncSender<Box<dyn PeerChannel>>,
}

/// The production [`PeerNetwork`]: mDNS discovery plus a TLS 1.3 listener on
/// the port the operating system assigned.
///
/// The listener exists for the lifetime of this object, but it only accepts
/// connections while an advertising session is open. An idle app therefore
/// advertises nothing and answers nothing.
pub struct TlsNetwork {
    device_id: DeviceId,
    listener: TcpListener,
    discovery: Arc<MdnsDiscovery>,
    config: TlsTransportConfig,
    configs: RwLock<Arc<TlsConfigs>>,
    session: Arc<Mutex<SessionState>>,
    running: Mutex<Option<Running>>,
    inbound: Inbound,
    /// Test-only observation points: the last negotiated TLS version, and the
    /// last client auth frame a successful connection sent, used to prove a
    /// captured proof cannot be replayed.
    #[cfg(test)]
    last_protocol_version: Mutex<Option<ProtocolVersion>>,
    #[cfg(test)]
    last_client_auth: Mutex<Option<Vec<u8>>>,
}

impl TlsNetwork {
    /// Bind an ephemeral port on every interface and prepare the transport.
    ///
    /// Nothing is advertised until [`PeerNetwork::start_advertising`]; the
    /// bound port is what mDNS will carry.
    pub fn new(device_id: DeviceId) -> Result<Self, NetworkError> {
        Self::with_config(device_id, TlsTransportConfig::default())
    }

    pub fn with_config(
        device_id: DeviceId,
        config: TlsTransportConfig,
    ) -> Result<Self, NetworkError> {
        Self::build(device_id, config, None)
    }

    fn build(
        device_id: DeviceId,
        config: TlsTransportConfig,
        adapter: Option<Arc<dyn crate::mdns::DiscoveryAdapter>>,
    ) -> Result<Self, NetworkError> {
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).map_err(|err| {
            transport_error(format!("the peer listener could not bind: {err}"))
        })?;
        listener.set_nonblocking(true).map_err(|err| {
            transport_error(format!("the peer listener could not be configured: {err}"))
        })?;
        let port = listener
            .local_addr()
            .map_err(|err| transport_error(format!("the bound port is unknown: {err}")))?
            .port();

        let discovery = match adapter {
            Some(adapter) => MdnsDiscovery::with_adapter(device_id, port, adapter),
            None => MdnsDiscovery::with_port(device_id, port),
        };

        Ok(Self {
            device_id,
            listener,
            discovery: Arc::new(discovery),
            config,
            configs: RwLock::new(Arc::new(generate_tls_configs()?)),
            session: Arc::new(Mutex::new(SessionState::default())),
            running: Mutex::new(None),
            inbound: Inbound::default(),
            #[cfg(test)]
            last_protocol_version: Mutex::new(None),
            #[cfg(test)]
            last_client_auth: Mutex::new(None),
        })
    }

    #[cfg(test)]
    fn with_adapter(
        device_id: DeviceId,
        config: TlsTransportConfig,
        adapter: Arc<dyn crate::mdns::DiscoveryAdapter>,
    ) -> Result<Self, NetworkError> {
        Self::build(device_id, config, Some(adapter))
    }

    /// The port this Device actually bound. Tests use it to dial loopback
    /// without multicast; mDNS advertises the same value.
    #[cfg(test)]
    pub(crate) fn local_port(&self) -> u16 {
        self.listener
            .local_addr()
            .expect("a bound listener always has a local address")
            .port()
    }

    fn configs(&self) -> Arc<TlsConfigs> {
        Arc::clone(&self
            .configs
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner()))
    }

    fn session_snapshot(&self) -> SessionSnapshot {
        self.session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .snapshot()
    }

    fn endpoint_address(peer: &DiscoveredPeer) -> Result<SocketAddr, NetworkError> {
        peer.address()
            .to_socket_addrs()
            .map_err(|_| NetworkError::PeerUnreachable("the peer address is invalid".into()))?
            .next()
            .ok_or_else(|| NetworkError::PeerUnreachable("the peer address is empty".into()))
    }

    /// Dial a peer and complete the TLS 1.3 handshake with the given ALPN.
    fn dial(&self, peer: &DiscoveredPeer, alpn: &[u8]) -> Result<TlsStream, NetworkError> {
        let address = Self::endpoint_address(peer)?;
        let socket = TcpStream::connect_timeout(&address, self.config.handshake_timeout)
            .map_err(|err| match err.kind() {
                io::ErrorKind::TimedOut => NetworkError::Timeout,
                _ => NetworkError::PeerUnreachable(peer.device_id().to_string()),
            })?;
        socket
            .set_read_timeout(Some(self.config.handshake_timeout))
            .map_err(map_io_error)?;
        socket
            .set_write_timeout(Some(self.config.handshake_timeout))
            .map_err(map_io_error)?;
        let _ = socket.set_nodelay(true);

        let mut client_config = (*self.configs().client).clone();
        client_config.alpn_protocols = vec![alpn.to_vec()];
        let server_name = ServerName::try_from("lockr.invalid".to_string())
            .map_err(|_| transport_error("the fixed peer server name is invalid".into()))?;
        let connection = ClientConnection::new(Arc::new(client_config), server_name)
            .map_err(|err| transport_error(format!("TLS could not start: {err}")))?;
        let mut stream = TlsStream::Client(StreamOwned::new(connection, socket));
        match &mut stream {
            TlsStream::Client(tls) => tls
                .conn
                .complete_io(&mut tls.sock)
                .map_err(map_handshake_error)?,
            TlsStream::Server(_) => unreachable!("a client stream was just constructed"),
        };

        if stream.alpn_protocol() != Some(alpn) {
            return Err(NetworkError::AuthenticationFailed);
        }
        // Pinning the version in the configuration already makes a downgrade
        // impossible; checking the negotiated result as well means a future
        // provider change cannot silently weaken this.
        if stream.protocol_version() != Some(ProtocolVersion::TLSv1_3) {
            return Err(NetworkError::AuthenticationFailed);
        }
        #[cfg(test)]
        {
            *self
                .last_protocol_version
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) =
                stream.protocol_version();
        }

        Ok(stream)
    }

    fn connect_sync(
        &self,
        peer: &DiscoveredPeer,
        client_auth_override: Option<&[u8]>,
    ) -> Result<Box<dyn PeerChannel>, NetworkError> {
        let mut stream = self.dial(peer, ALPN_SYNC)?;
        let session = self.session_snapshot();
        let (peer_id, sent_auth) = authenticate_client_sync(
            &mut stream,
            &session,
            self.device_id,
            peer.device_id(),
            client_auth_override,
        )?;
        #[cfg(test)]
        {
            *self
                .last_client_auth
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(sent_auth);
        }
        #[cfg(not(test))]
        let _ = sent_auth;
        Ok(Box::new(TlsChannel::new(
            stream,
            peer_id,
            self.config.frame_timeout,
        )))
    }

    fn start_listener(&self, configs: Arc<TlsConfigs>) -> Result<(), NetworkError> {
        let listener = self.listener.try_clone().map_err(|err| {
            transport_error(format!("the peer listener could not be shared: {err}"))
        })?;
        // Windows does not reliably carry non-blocking mode across a socket
        // duplication, and the session must be able to stop its accept loop.
        listener.set_nonblocking(true).map_err(|err| {
            transport_error(format!("the peer listener could not be configured: {err}"))
        })?;
        let (sync_sender, sync_receiver) = mpsc::sync_channel(MAX_PENDING_CONNECTIONS);
        let (pairing_sender, pairing_receiver) = mpsc::sync_channel(MAX_PENDING_CONNECTIONS);

        *self
            .inbound
            .sync
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(sync_receiver);
        *self
            .inbound
            .pairing
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(pairing_receiver);

        let stop = Arc::new(AtomicBool::new(false));
        let session = Arc::clone(&self.session);
        let device_id = self.device_id;
        let config = self.config.clone();
        let worker_stop = Arc::clone(&stop);
        let workers = Arc::new(AtomicUsize::new(0));

        let listener_stop = Arc::clone(&stop);
        let listener_sync_sender = sync_sender.clone();
        let listener_pairing_sender = pairing_sender.clone();
        let handle = std::thread::Builder::new()
            .name("lockr-peer-listener".to_string())
            .spawn(move || {
                let stop = listener_stop;
                let sync_sender = listener_sync_sender;
                let pairing_sender = listener_pairing_sender;
                while !stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((socket, _)) => {
                            if workers.fetch_add(1, Ordering::SeqCst) >= MAX_PENDING_CONNECTIONS {
                                workers.fetch_sub(1, Ordering::SeqCst);
                                continue;
                            }

                            let session = Arc::clone(&session);
                            let configs = Arc::clone(&configs);
                            let sync_sender = sync_sender.clone();
                            let pairing_sender = pairing_sender.clone();
                            let stop = Arc::clone(&worker_stop);
                            let config = config.clone();
                            let worker_counter = Arc::clone(&workers);

                            let spawned = std::thread::Builder::new()
                                .name("lockr-peer-handshake".to_string())
                                .spawn(move || {
                                    run_inbound_connection(
                                        socket,
                                        device_id,
                                        configs,
                                        session,
                                        config,
                                        sync_sender,
                                        pairing_sender,
                                        stop,
                                        worker_counter,
                                    );
                                });
                            if spawned.is_err() {
                                // The thread could not be created; the socket
                                // is dropped with the closure and the counter
                                // is rolled back so the ceiling stays honest.
                                workers.fetch_sub(1, Ordering::SeqCst);
                            }
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            std::thread::sleep(ACCEPT_POLL_INTERVAL);
                        }
                        Err(_) => break,
                    }
                }
            })
            .map_err(|err| transport_error(format!("the listener thread could not start: {err}")))?;

        *self
            .running
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Running {
            stop,
            handle,
            _sync_sender: sync_sender,
            _pairing_sender: pairing_sender,
        });

        Ok(())
    }

    fn stop_listener(&self) {
        let running = self
            .running
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some(running) = running {
            running.stop.store(true, Ordering::SeqCst);
            let _ = running.handle.join();
        }
        // Dropping the receivers closes any queued channel. Dropping the
        // senders (with `Running`) disconnects the rest.
        *self
            .inbound
            .sync
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        *self
            .inbound
            .pairing
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }

    /// Test-only: the negotiated TLS version of the last outbound connection.
    #[cfg(test)]
    fn negotiated_version(&self) -> Option<ProtocolVersion> {
        *self
            .last_protocol_version
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Test-only: the last successful client auth frame, captured before it
    /// was encrypted.
    #[cfg(test)]
    fn take_last_client_auth(&self) -> Option<Vec<u8>> {
        self.last_client_auth
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
    }

    /// Test-only: inject a captured client auth frame instead of computing a
    /// fresh proof. Used to prove a proof from an earlier session is refused.
    #[cfg(test)]
    fn connect_replaying(
        &self,
        peer: &DiscoveredPeer,
        captured_client_auth: &[u8],
    ) -> Result<Box<dyn PeerChannel>, NetworkError> {
        self.connect_sync(peer, Some(captured_client_auth))
    }

    /// Test-only: the pairing equivalent of [`Self::connect_replaying`].
    #[cfg(test)]
    fn connect_for_pairing_replaying(
        &self,
        peer: &DiscoveredPeer,
        claim: &PairingClaim,
        captured_client_auth: &[u8],
    ) -> Result<Box<dyn PeerChannel>, NetworkError> {
        let mut stream = self.dial(peer, ALPN_PAIR)?;
        authenticate_client_pairing(&mut stream, claim, Some(captured_client_auth))?;
        Ok(Box::new(TlsChannel::new(
            stream,
            peer.device_id(),
            self.config.frame_timeout,
        )))
    }
}

/// One inbound connection: TLS handshake, then either paired identity or
/// pairing-secret authentication, then queue the channel for its next user.
#[allow(clippy::too_many_arguments)]
fn run_inbound_connection(
    socket: TcpStream,
    device_id: DeviceId,
    configs: Arc<TlsConfigs>,
    session: Arc<Mutex<SessionState>>,
    config: TlsTransportConfig,
    sync_sender: SyncSender<Box<dyn PeerChannel>>,
    pairing_sender: SyncSender<Box<dyn PeerChannel>>,
    stop: Arc<AtomicBool>,
    workers: Arc<AtomicUsize>,
) {
    // Decrement the worker count however this function leaves.
    struct WorkerGuard(Arc<AtomicUsize>);
    impl Drop for WorkerGuard {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }
    let _guard = WorkerGuard(workers);

    if stop.load(Ordering::SeqCst) {
        return;
    }

    // The accept loop uses a non-blocking listener so it can notice the end
    // of the session. On Windows an accepted socket inherits that mode, which
    // would turn every read into `WouldBlock` and defeat the deadlines. Every
    // connection worker therefore switches its own socket back to blocking.
    let _ = socket.set_nonblocking(false);

    if socket.set_read_timeout(Some(config.handshake_timeout)).is_err()
        || socket
            .set_write_timeout(Some(config.handshake_timeout))
            .is_err()
    {
        return;
    }
    let _ = socket.set_nodelay(true);

    let connection = match ServerConnection::new(Arc::clone(&configs.server)) {
        Ok(connection) => connection,
        Err(_) => return,
    };
    let mut stream = TlsStream::Server(StreamOwned::new(connection, socket));
    let handshake = match &mut stream {
        TlsStream::Server(tls) => tls.conn.complete_io(&mut tls.sock),
        TlsStream::Client(_) => unreachable!("a server stream was just constructed"),
    };
    if handshake.is_err() {
        return;
    }

    let alpn = stream.alpn_protocol().map(|protocol| protocol.to_vec());
    if stream.protocol_version() != Some(ProtocolVersion::TLSv1_3) {
        return;
    }

    let snapshot = session
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .snapshot();

    let result = match alpn.as_deref() {
        Some(ALPN_SYNC) => authenticate_server_sync(&mut stream, &snapshot, device_id)
            .map(|peer| (peer, &sync_sender)),
        Some(ALPN_PAIR) => match snapshot.pairing_claim.as_ref() {
            // A pairing channel's peer id is not meaningful yet: the join
            // request on the channel is what names the joining Device, and
            // only after the human approves it.
            Some(claim) => authenticate_server_pairing(&mut stream, claim)
                .map(|()| (Uuid::nil(), &pairing_sender)),
            None => Err(NetworkError::AuthenticationFailed),
        },
        // A client that negotiated no protocol, or one we did not offer, gets
        // nothing. `alpn_protocols` is advertised on the server config, so this
        // is only reachable by a hand-crafted peer.
        _ => Err(NetworkError::AuthenticationFailed),
    };

    match result {
        Ok((peer, sender)) => {
            if stop.load(Ordering::SeqCst) {
                return;
            }
            let channel: Box<dyn PeerChannel> =
                Box::new(TlsChannel::new(stream, peer, config.frame_timeout));
            // A full queue drops the channel rather than blocking a worker.
            let _ = sender.try_send(channel);
        }
        Err(_) => {
            // Every failed pairing attempt counts against the transport's
            // budget, including a malformed or timed-out one: otherwise an
            // attacker could brute force the typed fallback without ever
            // producing a valid-looking confirmation.
            if alpn.as_deref() == Some(ALPN_PAIR) {
                session
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .record_pairing_failure();
            }
            refuse_connection(&mut stream);
        }
    }
}

impl PeerNetwork for TlsNetwork {
    fn start_advertising(&self) -> Result<(), NetworkError> {
        let already_running = self
            .running
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_some();
        if already_running {
            return self.discovery.start_advertising();
        }

        // A new session gets a new ephemeral certificate.
        let configs = Arc::new(generate_tls_configs()?);
        *self
            .configs
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Arc::clone(&configs);

        self.start_listener(configs)?;
        if let Err(error) = self.discovery.start_advertising() {
            // Never leave a listener answering when discovery failed to start.
            self.stop_listener();
            return Err(error);
        }
        Ok(())
    }

    fn stop_advertising(&self) -> Result<(), NetworkError> {
        self.stop_listener();
        self.discovery.stop_advertising()
    }

    fn discover(&self) -> Result<Vec<DiscoveredPeer>, NetworkError> {
        self.discovery.discover()
    }

    fn connect(&self, peer: &DiscoveredPeer) -> Result<Box<dyn PeerChannel>, NetworkError> {
        self.connect_sync(peer, None)
    }

    fn accept(&self) -> Result<Option<Box<dyn PeerChannel>>, NetworkError> {
        let guard = self
            .inbound
            .sync
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(receiver) = guard.as_ref() else {
            return Ok(None);
        };
        match receiver.recv_timeout(self.config.accept_wait) {
            Ok(channel) => Ok(Some(channel)),
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => Ok(None),
        }
    }

    fn begin_pairing(&self, claim: &PairingClaim) -> Result<(), NetworkError> {
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        session.pairing_claim = Some(claim.clone());
        session.pairing_failures = 0;
        Ok(())
    }

    fn end_pairing(&self) -> Result<(), NetworkError> {
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        session.pairing_claim = None;
        session.pairing_failures = 0;
        drop(session);

        // Close anything already queued: a pairing secret is single use, and
        // a channel waiting to be taken must not outlive the offer.
        if let Some(receiver) = self
            .inbound
            .pairing
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
        {
            while let Ok(mut channel) = receiver.try_recv() {
                channel.close();
            }
        }
        Ok(())
    }

    fn connect_for_pairing(
        &self,
        peer: &DiscoveredPeer,
        claim: &PairingClaim,
    ) -> Result<Box<dyn PeerChannel>, NetworkError> {
        let mut stream = self.dial(peer, ALPN_PAIR)?;
        let sent_auth = authenticate_client_pairing(&mut stream, claim, None)?;
        #[cfg(test)]
        {
            *self
                .last_client_auth
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(sent_auth);
        }
        #[cfg(not(test))]
        let _ = sent_auth;
        Ok(Box::new(TlsChannel::new(
            stream,
            peer.device_id(),
            self.config.frame_timeout,
        )))
    }

    fn accept_for_pairing(
        &self,
        claim: &PairingClaim,
    ) -> Result<Option<Box<dyn PeerChannel>>, NetworkError> {
        // The caller's claim must be the one this Device registered. This is a
        // constant-time comparison of the presented material, and it means a
        // caller cannot ask for a channel authenticated by a different offer.
        let registered = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pairing_claim
            .clone()
            .ok_or(NetworkError::AuthenticationFailed)?;
        if !registered.matches_claim(claim) {
            return Err(NetworkError::AuthenticationFailed);
        }

        let guard = self
            .inbound
            .pairing
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(receiver) = guard.as_ref() else {
            return Ok(None);
        };
        match receiver.recv_timeout(self.config.accept_wait) {
            Ok(channel) => Ok(Some(channel)),
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => Ok(None),
        }
    }

    fn set_session(&self, session: &TransportSession) -> Result<(), NetworkError> {
        if session.device_id != self.device_id {
            return Err(transport_error(
                "the session identity belongs to a different Device".into(),
            ));
        }
        let mut state = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.keypair = Some(Arc::clone(&session.keypair));
        state.devices = Some(Arc::clone(&session.devices));
        Ok(())
    }

    fn clear_session(&self) {
        let mut state = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.keypair = None;
        state.devices = None;
        state.pairing_claim = None;
        state.pairing_failures = 0;
    }
}

impl Drop for TlsNetwork {
    fn drop(&mut self) {
        let _ = self.stop_advertising();
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Mutex as StdMutex;

    use chrono::Utc;

    use super::*;
    use crate::mdns::{DiscoveryAdapter, DiscoveryEvent, ServiceRecord};
    use crate::network::{PeerAdvertisement, PeerEndpoint};
    use crate::pairing::{PairingCode, PairingSecret};
    use crate::registry::{DeviceRecord, DeviceRegistry};
    use crate::sync::{SyncCredentials, SyncEngine, SyncProgress, SyncStateStore};
    use crate::user_manager::UserManager;
    use crate::vault_manager::VaultManager;
    use crate::{
        AccountData, ItemKind, NewItemData, NoteData, SecretInput, UserId, CURRENT_SCHEMA_VERSION,
    };

    const MASTER_PW: &str = "Correct-Horse-Battery-Staple-9";
    const USERNAME: &str = "alice@example.test";
    const PASSWORD: &str = "hunter2-the-real-secret";
    const NOTE_BODY: &str = "the eagle lands at dawn";

    fn test_config() -> TlsTransportConfig {
        TlsTransportConfig {
            handshake_timeout: Duration::from_secs(3),
            frame_timeout: Duration::from_secs(3),
            accept_wait: Duration::from_millis(50),
        }
    }

    /// Stands in for mDNS so the transport can be exercised without
    /// multicast. It records exactly what would have been published.
    #[derive(Default)]
    struct NullDiscovery {
        advertised: StdMutex<Vec<ServiceRecord>>,
    }

    impl DiscoveryAdapter for NullDiscovery {
        fn advertise(&self, record: ServiceRecord) -> Result<(), NetworkError> {
            self.advertised.lock().unwrap().push(record);
            Ok(())
        }

        fn browse(&self) -> Result<(), NetworkError> {
            Ok(())
        }

        fn stop(&self) -> Result<(), NetworkError> {
            Ok(())
        }

        fn poll(&self) -> Vec<DiscoveryEvent> {
            Vec::new()
        }
    }

    fn test_network(device_id: DeviceId) -> TlsNetwork {
        TlsNetwork::with_adapter(
            device_id,
            test_config(),
            Arc::new(NullDiscovery::default()),
        )
        .unwrap()
    }

    fn peer_for(network: &TlsNetwork, device_id: DeviceId) -> DiscoveredPeer {
        DiscoveredPeer::new(
            PeerAdvertisement::new(device_id),
            PeerEndpoint::new(format!("127.0.0.1:{}", network.local_port())),
        )
    }

    fn session_for(user_manager: &UserManager, user_id: UserId) -> TransportSession {
        TransportSession {
            device_id: user_manager.device_id(),
            keypair: user_manager
                .session_keypair()
                .expect("the fixture is signed in"),
            devices: Arc::new(
                user_manager
                    .get_shared_profile(&user_id)
                    .expect("the shared registry verifies")
                    .devices,
            ),
        }
    }

    fn key_session(
        device_id: DeviceId,
        keypair: &Arc<DeviceKeypair>,
        registry: &DeviceRegistry,
    ) -> TransportSession {
        TransportSession {
            device_id,
            keypair: Arc::clone(keypair),
            devices: Arc::new(registry.clone()),
        }
    }

    fn registry_for(
        root: DeviceId,
        root_key: &DeviceKeypair,
        peer: DeviceId,
        peer_key: &DeviceKeypair,
    ) -> DeviceRegistry {
        let now = Utc::now();
        let mut registry = DeviceRegistry::new();
        registry
            .admit(DeviceRecord::admit(
                root,
                root_key.public_key(),
                "Root".to_string(),
                "windows".to_string(),
                root,
                now,
                root_key,
            ))
            .unwrap();
        registry
            .admit(DeviceRecord::admit(
                peer,
                peer_key.public_key(),
                "Peer".to_string(),
                "windows".to_string(),
                root,
                now,
                root_key,
            ))
            .unwrap();
        registry
    }

    fn setup_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lockr_transport_{tag}_{}", Uuid::new_v4()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|window| window == needle)
    }

    /// A TCP forwarder that records every byte it carries. Pointing both the
    /// pairing and the sync connection at it makes the wire assertions real
    /// rather than assertions about a hand-built payload.
    struct RecordingProxy {
        listener: TcpListener,
        port: u16,
        stop: Arc<AtomicBool>,
        recorded: Arc<StdMutex<Vec<u8>>>,
        thread: Option<JoinHandle<()>>,
    }

    impl RecordingProxy {
        fn start(upstream: SocketAddr) -> Self {
            let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            listener.set_nonblocking(true).unwrap();
            let port = listener.local_addr().unwrap().port();
            let stop = Arc::new(AtomicBool::new(false));
            let recorded = Arc::new(StdMutex::new(Vec::new()));

            let listener_thread = listener.try_clone().unwrap();
            let thread_stop = Arc::clone(&stop);
            let thread_recorded = Arc::clone(&recorded);
            let thread = std::thread::spawn(move || {
                while !thread_stop.load(Ordering::SeqCst) {
                    match listener_thread.accept() {
                        Ok((client, _)) => {
                            let Ok(server) = TcpStream::connect(upstream) else {
                                continue;
                            };
                            let (mut client_read, mut client_write) =
                                (client.try_clone().unwrap(), client);
                            let (mut server_read, mut server_write) =
                                (server.try_clone().unwrap(), server);

                            let to_server = Arc::clone(&thread_recorded);
                            std::thread::spawn(move || {
                                copy_recording(&mut client_read, &mut server_write, &to_server);
                            });
                            let to_client = Arc::clone(&thread_recorded);
                            std::thread::spawn(move || {
                                copy_recording(&mut server_read, &mut client_write, &to_client);
                            });
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(_) => break,
                    }
                }
            });

            Self {
                listener,
                port,
                stop,
                recorded,
                thread: Some(thread),
            }
        }

        fn recorded(&self) -> Vec<u8> {
            self.recorded.lock().unwrap().clone()
        }
    }

    impl Drop for RecordingProxy {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            // Wake the blocking/non-blocking accept loop without waiting for
            // the poll interval.
            let _ = TcpStream::connect(("127.0.0.1", self.listener.local_addr().unwrap().port()));
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    fn copy_recording<R: Read, W: Write>(
        from: &mut R,
        to: &mut W,
        recorded: &StdMutex<Vec<u8>>,
    ) {
        let mut buffer = [0u8; 8192];
        loop {
            match from.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    recorded
                        .lock()
                        .unwrap()
                        .extend_from_slice(&buffer[..read]);
                    if to.write_all(&buffer[..read]).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = to.flush();
    }

    #[test]
    fn test_the_advertised_record_carries_the_port_actually_bound() {
        let device_id = Uuid::new_v4();
        let records = Arc::new(NullDiscovery::default());
        let adapter: Arc<dyn DiscoveryAdapter> = records.clone();
        let network = TlsNetwork::with_adapter(device_id, test_config(), adapter).unwrap();

        network.start_advertising().unwrap();

        let advertised = records.advertised.lock().unwrap();
        assert_eq!(advertised.len(), 1, "a session advertises exactly once");
        assert_ne!(network.local_port(), 0, "the OS assigned a real port");
        assert_eq!(
            advertised[0].port(),
            network.local_port(),
            "mDNS must carry the port the transport is listening on"
        );

        network.stop_advertising().unwrap();
    }

    #[test]
    fn test_paired_channel_negotiates_tls_13() {
        let a_id = Uuid::new_v4();
        let b_id = Uuid::new_v4();
        let a_key = Arc::new(DeviceKeypair::generate());
        let b_key = Arc::new(DeviceKeypair::generate());
        let registry = registry_for(a_id, &a_key, b_id, &b_key);

        let a = test_network(a_id);
        let b = test_network(b_id);
        a.set_session(&key_session(a_id, &a_key, &registry)).unwrap();
        b.set_session(&key_session(b_id, &b_key, &registry)).unwrap();
        b.start_advertising().unwrap();

        let mut channel = a.connect(&peer_for(&b, b_id)).unwrap();
        assert_eq!(channel.peer_device_id(), b_id);
        assert_eq!(
            a.negotiated_version(),
            Some(ProtocolVersion::TLSv1_3),
            "a connection must never negotiate below TLS 1.3"
        );

        let mut responder = b.accept().unwrap().expect("the channel is queued");
        assert_eq!(responder.peer_device_id(), a_id);
        channel.close();
        responder.close();
    }

    #[test]
    fn test_a_captured_proof_cannot_be_replayed_into_another_session() {
        let a_id = Uuid::new_v4();
        let b_id = Uuid::new_v4();
        let a_key = Arc::new(DeviceKeypair::generate());
        let b_key = Arc::new(DeviceKeypair::generate());
        let registry = registry_for(a_id, &a_key, b_id, &b_key);

        let a = test_network(a_id);
        let b = test_network(b_id);
        a.set_session(&key_session(a_id, &a_key, &registry)).unwrap();
        b.set_session(&key_session(b_id, &b_key, &registry)).unwrap();
        b.start_advertising().unwrap();

        let peer = peer_for(&b, b_id);

        // Session one: a genuine proof, captured before it was encrypted.
        // Its success is what makes it a *valid* captured proof.
        let mut first = a.connect(&peer).unwrap();
        let captured = a.take_last_client_auth().expect("the proof was captured");
        first.close();
        assert!(b.accept().unwrap().is_some(), "session one was accepted");

        // Session two: the same bytes must not authenticate. The exporter and
        // both nonces differ, so the signature covers a different message.
        let Err(error) = a.connect_replaying(&peer, &captured) else {
            panic!("a replayed proof must not authenticate");
        };
        assert_eq!(error, NetworkError::AuthenticationFailed);
        assert!(
            b.accept().unwrap().is_none(),
            "a replayed proof must not reach the channel queue"
        );

        // The rejection is per connection, not a poisoned session.
        let second = a.connect(&peer).unwrap();
        assert_eq!(second.peer_device_id(), b_id);
        assert!(b.accept().unwrap().is_some());
    }

    #[test]
    fn test_a_device_absent_from_the_registry_is_refused_before_any_channel() {
        let a_id = Uuid::new_v4();
        let b_id = Uuid::new_v4();
        let a_key = Arc::new(DeviceKeypair::generate());
        let b_key = Arc::new(DeviceKeypair::generate());
        let empty = DeviceRegistry::new();

        let a = test_network(a_id);
        let b = test_network(b_id);
        a.set_session(&key_session(a_id, &a_key, &empty)).unwrap();
        b.set_session(&key_session(b_id, &b_key, &empty)).unwrap();
        b.start_advertising().unwrap();

        let Err(error) = a.connect(&peer_for(&b, b_id)) else {
            panic!("a Device absent from the registry must be refused");
        };
        assert_eq!(error, NetworkError::AuthenticationFailed);
        assert!(
            b.accept().unwrap().is_none(),
            "no channel may exist for a Device the registry does not admit"
        );
    }

    #[test]
    fn test_a_revoked_device_is_refused() {
        let a_id = Uuid::new_v4();
        let b_id = Uuid::new_v4();
        let a_key = Arc::new(DeviceKeypair::generate());
        let b_key = Arc::new(DeviceKeypair::generate());

        let mut registry = registry_for(a_id, &a_key, b_id, &b_key);
        registry.revoke(&a_id, Utc::now(), &b_id, &b_key).unwrap();
        assert!(registry.is_revoked(&a_id));

        let a = test_network(a_id);
        let b = test_network(b_id);
        // A still believes it is admitted; the server's registry is the one
        // that matters, and it refuses a revoked proof.
        a.set_session(&key_session(
            a_id,
            &a_key,
            &registry_for(a_id, &a_key, b_id, &b_key),
        ))
        .unwrap();
        b.set_session(&key_session(b_id, &b_key, &registry)).unwrap();
        b.start_advertising().unwrap();

        let Err(error) = a.connect(&peer_for(&b, b_id)) else {
            panic!("a revoked Device must be refused");
        };
        assert_eq!(error, NetworkError::AuthenticationFailed);
        assert!(b.accept().unwrap().is_none());
    }

    #[test]
    fn test_a_wrong_pairing_secret_is_refused_and_does_not_burn_the_offer() {
        let a_id = Uuid::new_v4();
        let b_id = Uuid::new_v4();
        let a = test_network(a_id);
        let b = test_network(b_id);
        a.start_advertising().unwrap();

        let now = Utc::now();
        let (_, offer) = crate::pairing::PairingHost::start(
            Uuid::new_v4(),
            a_id,
            CURRENT_SCHEMA_VERSION,
            [7u8; 32],
            now,
        );
        let correct = PairingCode::qr(offer.qr_payload.clone());
        let correct_claim = correct.claim().unwrap();
        a.begin_pairing(&correct_claim).unwrap();

        // A different secret never authenticates, in either form.
        let wrong = PairingCode::typed(a_id, PairingSecret::generate().typed_code());
        let wrong_claim = wrong.claim().unwrap();
        let peer = peer_for(&a, a_id);

        let Err(error) = b.connect_for_pairing(&peer, &wrong_claim) else {
            panic!("a wrong pairing secret must be refused");
        };
        assert_eq!(error, NetworkError::AuthenticationFailed);
        assert!(
            a.accept_for_pairing(&correct_claim).unwrap().is_none(),
            "a wrong secret must not queue a pairing channel"
        );

        // One failed attempt must leave a legitimate pairing working.
        assert!(b.connect_for_pairing(&peer, &correct_claim).is_ok());
        assert!(
            a.accept_for_pairing(&correct_claim).unwrap().is_some(),
            "a wrong attempt must not burn the one-time secret"
        );
    }

    #[test]
    fn test_repeated_wrong_pairing_secrets_burn_the_transport_offer() {
        let a_id = Uuid::new_v4();
        let b_id = Uuid::new_v4();
        let a = test_network(a_id);
        let b = test_network(b_id);
        a.start_advertising().unwrap();

        let now = Utc::now();
        let (_, offer) = crate::pairing::PairingHost::start(
            Uuid::new_v4(),
            a_id,
            CURRENT_SCHEMA_VERSION,
            [7u8; 32],
            now,
        );
        let claim = PairingCode::qr(offer.qr_payload.clone()).claim().unwrap();
        a.begin_pairing(&claim).unwrap();
        let peer = peer_for(&a, a_id);

        for attempt in 0..crate::pairing::PAIRING_MAX_ATTEMPTS {
            let wrong = PairingCode::typed(a_id, PairingSecret::generate().typed_code())
                .claim()
                .unwrap();
            assert!(
                b.connect_for_pairing(&peer, &wrong).is_err(),
                "wrong attempt {attempt} must be refused"
            );
        }

        // The budget is now spent. Even the genuine secret cannot open the
        // transport's copy of the offer any more.
        let Err(error) = b.connect_for_pairing(&peer, &claim) else {
            panic!("the pairing offer must be burned after its attempt budget");
        };
        assert_eq!(error, NetworkError::AuthenticationFailed);
        assert_eq!(
            a.accept_for_pairing(&claim).err(),
            Some(NetworkError::AuthenticationFailed)
        );
    }

    #[test]
    fn test_a_captured_pairing_proof_cannot_be_replayed_into_another_session() {
        let a_id = Uuid::new_v4();
        let b_id = Uuid::new_v4();
        let a = test_network(a_id);
        let b = test_network(b_id);
        a.start_advertising().unwrap();

        let now = Utc::now();
        let (_, offer) = crate::pairing::PairingHost::start(
            Uuid::new_v4(),
            a_id,
            CURRENT_SCHEMA_VERSION,
            [7u8; 32],
            now,
        );
        let claim = PairingCode::qr(offer.qr_payload.clone()).claim().unwrap();
        a.begin_pairing(&claim).unwrap();
        let peer = peer_for(&a, a_id);

        let mut first = b.connect_for_pairing(&peer, &claim).unwrap();
        let captured = b
            .take_last_client_auth()
            .expect("the pairing confirmation was captured");
        first.close();
        assert!(a.accept_for_pairing(&claim).unwrap().is_some());

        let Err(error) = b.connect_for_pairing_replaying(&peer, &claim, &captured) else {
            panic!("a replayed pairing proof must not authenticate");
        };
        assert_eq!(error, NetworkError::AuthenticationFailed);
        assert!(a.accept_for_pairing(&claim).unwrap().is_none());

        // One failed attempt does not invalidate the live offer.
        let mut second = b.connect_for_pairing(&peer, &claim).unwrap();
        assert!(a.accept_for_pairing(&claim).unwrap().is_some());
        second.close();
    }

    #[test]
    fn test_a_reused_pairing_secret_is_refused() {
        let a_id = Uuid::new_v4();
        let b_id = Uuid::new_v4();
        let a = test_network(a_id);
        let b = test_network(b_id);
        a.start_advertising().unwrap();

        let now = Utc::now();
        let (_, offer) = crate::pairing::PairingHost::start(
            Uuid::new_v4(),
            a_id,
            CURRENT_SCHEMA_VERSION,
            [7u8; 32],
            now,
        );
        let claim = PairingCode::qr(offer.qr_payload.clone()).claim().unwrap();
        a.begin_pairing(&claim).unwrap();
        let peer = peer_for(&a, a_id);

        let mut first = b.connect_for_pairing(&peer, &claim).unwrap();
        let mut responder = a.accept_for_pairing(&claim).unwrap().unwrap();
        // Prove this is a real, working pairing channel before ending it.
        first.send(SyncMessage::Complete).unwrap();
        assert_eq!(responder.receive().unwrap(), SyncMessage::Complete);
        first.close();
        responder.close();

        a.end_pairing().unwrap();
        let Err(error) = b.connect_for_pairing(&peer, &claim) else {
            panic!("a reused pairing secret must be refused");
        };
        assert_eq!(error, NetworkError::AuthenticationFailed);
        assert_eq!(
            a.accept_for_pairing(&claim).err(),
            Some(NetworkError::AuthenticationFailed),
            "the transport no longer holds the secret"
        );
    }

    #[test]
    fn test_a_silent_peer_does_not_block_past_the_handshake_deadline() {
        // A server that accepts TCP and then sends nothing at all. It is not
        // speaking TLS, but the point is that the client gives up on time.
        let silent = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = silent.local_addr().unwrap().port();
        let accepted = Arc::new(AtomicBool::new(false));
        let accepted_thread = Arc::clone(&accepted);
        std::thread::spawn(move || {
            if let Ok((_socket, _)) = silent.accept() {
                accepted_thread.store(true, Ordering::SeqCst);
                std::thread::sleep(Duration::from_secs(2));
            }
        });

        let device_id = Uuid::new_v4();
        let network = TlsNetwork::with_adapter(
            device_id,
            TlsTransportConfig {
                handshake_timeout: Duration::from_millis(300),
                ..test_config()
            },
            Arc::new(NullDiscovery::default()),
        )
        .unwrap();
        // The client identity exists; the deadline must fire during the TLS
        // handshake, before any application proof is exchanged.
        let keypair = Arc::new(DeviceKeypair::generate());
        network
            .set_session(&TransportSession {
                device_id,
                keypair,
                devices: Arc::new(DeviceRegistry::new()),
            })
            .unwrap();
        let peer = DiscoveredPeer::new(
            PeerAdvertisement::new(Uuid::new_v4()),
            PeerEndpoint::new(format!("127.0.0.1:{port}")),
        );

        let started = std::time::Instant::now();
        let Err(error) = network.connect(&peer) else {
            panic!("a silent peer must not produce a channel");
        };
        assert_eq!(error, NetworkError::Timeout);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "the handshake deadline must end a silent connection: {:?}",
            started.elapsed()
        );
        assert!(
            accepted.load(Ordering::SeqCst),
            "the peer really accepted the connection"
        );
    }

    #[test]
    fn test_receive_timeout_ends_a_read_from_a_silent_authenticated_peer() {
        let a_id = Uuid::new_v4();
        let b_id = Uuid::new_v4();
        let a_key = Arc::new(DeviceKeypair::generate());
        let b_key = Arc::new(DeviceKeypair::generate());
        let registry = registry_for(a_id, &a_key, b_id, &b_key);

        let a = test_network(a_id);
        let b = test_network(b_id);
        a.set_session(&key_session(a_id, &a_key, &registry)).unwrap();
        b.set_session(&key_session(b_id, &b_key, &registry)).unwrap();
        b.start_advertising().unwrap();

        let mut channel = a.connect(&peer_for(&b, b_id)).unwrap();
        let started = std::time::Instant::now();
        assert_eq!(
            channel.receive_timeout(Duration::from_millis(100)),
            Err(NetworkError::Timeout)
        );
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    /// The acceptance test: two managers pair and complete a sync over real
    /// TCP and TLS, and the wire was inspected.
    #[test]
    fn test_two_managers_pair_and_sync_over_real_loopback_sockets() {
        let a_dir = setup_dir("a");
        let b_dir = setup_dir("b");

        let mut source_user = UserManager::new(a_dir.clone()).unwrap();
        let user = source_user
            .register("Alice".to_string(), MASTER_PW)
            .unwrap();
        let source_key = source_user.login(&user.id, MASTER_PW).unwrap();

        let mut source_vaults = VaultManager::new(a_dir.clone()).unwrap();
        let vault = source_vaults
            .create_vault(
                "Personal".to_string(),
                "#6240BF".to_string(),
                MASTER_PW,
                &source_key,
            )
            .unwrap();
        source_user.add_vault_to_user(vault.id).unwrap();

        let account = source_vaults
            .add_item(
                vault.id,
                Some("Mail".to_string()),
                None,
                NewItemData::Account(AccountData {
                    username: USERNAME.to_string(),
                    email: None,
                }),
                Some(vec![SecretInput {
                    key: "password".to_string(),
                    value: PASSWORD.to_string(),
                }]),
            )
            .unwrap();
        let account_id = match account {
            crate::SafeItem::Account(account) => account.id,
            other => panic!("expected an account, got {other:?}"),
        };
        source_vaults
            .add_item(
                vault.id,
                Some("Recovery".to_string()),
                None,
                NewItemData::Note(NoteData {
                    title: Some("Recovery codes".to_string()),
                    body: NOTE_BODY.to_string(),
                }),
                None,
            )
            .unwrap();

        let mut joiner_user = UserManager::new(b_dir.clone()).unwrap();
        let mut joiner_vaults = VaultManager::new(b_dir.clone()).unwrap();

        let source_id = source_user.device_id();
        let joiner_id = joiner_user.device_id();

        let source_net = test_network(source_id);
        let joiner_net = test_network(joiner_id);
        let proxy = RecordingProxy::start(
            format!("127.0.0.1:{}", source_net.local_port())
                .parse()
                .unwrap(),
        );
        let source_at_proxy = DiscoveredPeer::new(
            PeerAdvertisement::new(source_id),
            PeerEndpoint::new(format!("127.0.0.1:{}", proxy.port)),
        );

        source_net.start_advertising().unwrap();

        // ---- Pair over the real pairing channel ----
        let now = Utc::now();
        let offer = source_user
            .start_pairing(&source_key, CURRENT_SCHEMA_VERSION, now)
            .unwrap();
        let code = PairingCode::qr(offer.qr_payload.clone());
        let claim = code.claim().unwrap();
        source_net.begin_pairing(&claim).unwrap();

        let join = joiner_user
            .join_pairing(&code, CURRENT_SCHEMA_VERSION, now)
            .unwrap();
        let mut initiator = joiner_net
            .connect_for_pairing(&source_at_proxy, &claim)
            .unwrap();
        let mut responder = source_net
            .accept_for_pairing(&claim)
            .unwrap()
            .expect("the join is waiting");
        assert_eq!(initiator.peer_device_id(), source_id);

        initiator.send(SyncMessage::Pairing(join)).unwrap();
        let request = match responder.receive().unwrap() {
            SyncMessage::Pairing(message) => message,
            other => panic!("expected a pairing request, got {other:?}"),
        };
        let prompt = source_user.receive_pairing_request(request, now).unwrap();
        let transfer = source_user.approve_pairing(prompt.device_id, now).unwrap();
        responder.send(SyncMessage::Pairing(transfer)).unwrap();
        let transfer = match initiator.receive().unwrap() {
            SyncMessage::Pairing(message) => message,
            other => panic!("expected a pairing transfer, got {other:?}"),
        };
        joiner_user
            .complete_pairing(transfer, MASTER_PW, now)
            .unwrap();
        source_net.end_pairing().unwrap();

        let joiner_key = joiner_user.login(&user.id, MASTER_PW).unwrap();
        assert_eq!(
            joiner_key, source_key,
            "the User's secret key must arrive intact and never in plaintext"
        );

        // ---- Configure the paired transport and share the vault ----
        source_net
            .set_session(&session_for(&source_user, user.id))
            .unwrap();
        joiner_net
            .set_session(&session_for(&joiner_user, user.id))
            .unwrap();

        // Deliberately *not* copied by hand: the vault file must arrive over
        // the synced transport. Pairing only carried the registries, so the
        // joiner knows the Vault exists but holds no file for it yet.

        let source_sync = SyncStateStore::new(&a_dir).unwrap();
        let joiner_sync = SyncStateStore::new(&b_dir).unwrap();

        // ---- Sync over the same real transport ----
        //
        // The source is the initiator here, so the joiner is the responder:
        // the sync connection goes to the joiner's listener. A second
        // recording proxy keeps the wire assertions covering this direction
        // too.
        joiner_net.start_advertising().unwrap();
        let sync_proxy = RecordingProxy::start(
            format!("127.0.0.1:{}", joiner_net.local_port())
                .parse()
                .unwrap(),
        );
        let joiner_at_proxy = DiscoveredPeer::new(
            PeerAdvertisement::new(joiner_id),
            PeerEndpoint::new(format!("127.0.0.1:{}", sync_proxy.port)),
        );

        let mut noop = |_: SyncProgress| {};
        let mut source_engine = SyncEngine::new(
            &source_net,
            &mut source_vaults,
            &source_user,
            source_sync.scope(&user.id),
            SyncCredentials {
                master_password: MASTER_PW,
                secret_key: &source_key,
            },
        );
        let start = source_engine
            .start_with_peer(&joiner_at_proxy, &mut noop)
            .unwrap();

        let mut joiner_engine = SyncEngine::new(
            &joiner_net,
            &mut joiner_vaults,
            &joiner_user,
            joiner_sync.scope(&user.id),
            SyncCredentials {
                master_password: MASTER_PW,
                secret_key: &joiner_key,
            },
        );
        let accepted = joiner_engine
            .accept(&mut noop)
            .unwrap()
            .expect("the initiator is waiting");

        let sent = source_engine.send_offers(start, &mut noop).unwrap();
        let joiner_outcome = joiner_engine
            .receive_offers(accepted, &mut noop)
            .unwrap();
        let source_outcome = source_engine.collect(sent, &mut noop).unwrap();
        assert_eq!(
            source_outcome.report.vault_ids,
            Vec::<crate::VaultId>::new(),
            "the joining Device held no vault file to send back"
        );
        assert_eq!(
            joiner_outcome.report.vault_ids,
            vec![vault.id],
            "the Vault arrived over the transport"
        );

        // Both engines hold a mutable borrow of their VaultManager; release
        // them before asserting on the resulting vaults.
        drop(source_engine);
        drop(joiner_engine);

        // ---- What actually arrived ----
        assert_eq!(
            joiner_vaults
                .get_item_secret(vault.id, account_id, "password")
                .unwrap()
                .as_deref(),
            Some(PASSWORD),
            "the secret value must survive the real transport"
        );
        let joiner_vault = joiner_vaults.get_vault(vault.id).unwrap();
        assert!(
            joiner_vault.items.iter().any(|item| matches!(
                &item.kind,
                Some(ItemKind::Note(note)) if note.body == NOTE_BODY
            )),
            "vault content must survive the real transport"
        );
        assert!(joiner_vault
            .items
            .iter()
            .any(|item| { matches!(&item.kind, Some(ItemKind::Account(account)) if account.username == USERNAME) }));

        // ---- The wire itself ----
        let mut recorded = proxy.recorded();
        recorded.extend_from_slice(&sync_proxy.recorded());
        assert!(
            recorded.len() > 10_000,
            "the proxy must have carried a real session, not nothing ({} bytes)",
            recorded.len()
        );
        for (label, needle) in [
            ("the master password", MASTER_PW.as_bytes()),
            ("a secret value", PASSWORD.as_bytes()),
            ("vault content", NOTE_BODY.as_bytes()),
            ("an item username", USERNAME.as_bytes()),
            ("the User's secret key", source_key.as_slice()),
        ] {
            assert!(
                !contains(&recorded, needle),
                "{label} appeared in plaintext in {} bytes carried by the real connection",
                recorded.len()
            );
        }

        std::fs::remove_dir_all(a_dir).ok();
        std::fs::remove_dir_all(b_dir).ok();
    }
}
