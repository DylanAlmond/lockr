//! mDNS advertisement and browsing: the production discovery half of the
//! network seam.
//!
//! Discovery is deliberately minimal and session-scoped. The published record
//! carries the service type and an opaque [`DeviceId`] in its only TXT
//! property, and nothing else: no User name, no Vault names, and no Vault
//! count. The machine's host name is never published either, because it is
//! user-chosen and can be identifying; the mDNS host label is derived from the
//! same opaque Device id.
//!
//! An [`MdnsDiscovery`] is inert until a session starts. Starting creates the
//! mDNS responder and browser; stopping unregisters the service and shuts the
//! daemon down, so an idle app leaves nothing listening or answering on the
//! network. [`DiscoverySession`] is the RAII form of that lifecycle, so an
//! early return or panic cannot leave a responder behind.

use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use uuid::Uuid;

use crate::network::{
    DiscoveredPeer, NetworkError, PeerAdvertisement, PeerChannel, PeerEndpoint, PeerNetwork,
    SYNC_SERVICE_TYPE,
};
use crate::DeviceId;

/// The fallback port used by [`MdnsDiscovery::new`].
///
/// Production no longer uses a fixed port: the TLS transport binds port zero
/// for the session and advertises the port the operating system assigned via
/// [`MdnsDiscovery::with_port`]. This constant remains for callers that have no
/// transport of their own (and for the discovery-only tests).
pub const SYNC_TRANSPORT_PORT: u16 = 47821;

/// The one TXT property Lockr publishes. Nothing else belongs in it.
const DEVICE_ID_KEY: &str = "device_id";

/// What an mDNS advertisement is allowed to contain, independent of the
/// `mdns-sd` crate so the payload can be asserted directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServiceRecord {
    service_type: String,
    instance_name: String,
    host_name: String,
    port: u16,
    properties: Vec<(String, String)>,
}

impl ServiceRecord {
    /// The port this record advertises. The transport asserts this is the
    /// port it actually bound before the session starts.
    #[cfg(test)]
    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    pub(crate) fn from_advertisement(advertisement: &PeerAdvertisement, port: u16) -> Self {
        let device_id = advertisement.device_id();
        Self {
            service_type: advertisement.service_type().to_string(),
            instance_name: format!("lockr-{device_id}"),
            host_name: format!("lockr-{device_id}.local."),
            port,
            properties: vec![(DEVICE_ID_KEY.to_string(), device_id.to_string())],
        }
    }
}

/// Turn the minimal record into the crate's service description. Addresses
/// are left for the daemon to fill from the interfaces, so no local address
/// or interface name has to be baked into the payload.
fn service_info(record: &ServiceRecord) -> Result<mdns_sd::ServiceInfo, NetworkError> {
    let info = mdns_sd::ServiceInfo::new(
        &record.service_type,
        &record.instance_name,
        &record.host_name,
        "",
        record.port,
        &record.properties[..],
    )
    .map_err(|err| NetworkError::DiscoveryFailed(format!("invalid service record: {err}")))?;

    Ok(info.enable_addr_auto())
}

/// One resolved service, stripped of `mdns-sd` types so the discovery logic
/// and its tests never depend on the crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedService {
    pub(crate) service_type: String,
    pub(crate) fullname: String,
    pub(crate) port: u16,
    pub(crate) addresses: Vec<IpAddr>,
    pub(crate) properties: Vec<(String, String)>,
}

/// A browse event, stripped of `mdns-sd` types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DiscoveryEvent {
    Resolved(ResolvedService),
    Removed { fullname: String },
}

/// The thin adapter around the mDNS crate.
///
/// Everything that needs a real socket or a background thread lives behind
/// this trait. The discovery logic above it is a plain state machine, so its
/// payload and lifecycle can be tested without multicast.
pub(crate) trait DiscoveryAdapter: Send + Sync {
    fn advertise(&self, record: ServiceRecord) -> Result<(), NetworkError>;

    fn browse(&self) -> Result<(), NetworkError>;

    fn stop(&self) -> Result<(), NetworkError>;

    fn poll(&self) -> Vec<DiscoveryEvent>;
}

struct ActiveMdns {
    daemon: mdns_sd::ServiceDaemon,
    events: Option<mdns_sd::Receiver<mdns_sd::ServiceEvent>>,
    fullname: String,
}

/// The production adapter: one `mdns-sd` daemon per session, created on
/// advertisement and shut down when the session ends.
#[derive(Default)]
pub(crate) struct MdnsSdAdapter {
    session: Mutex<Option<ActiveMdns>>,
}

impl MdnsSdAdapter {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn lock_session(&self) -> std::sync::MutexGuard<'_, Option<ActiveMdns>> {
        self.session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl DiscoveryAdapter for MdnsSdAdapter {
    fn advertise(&self, record: ServiceRecord) -> Result<(), NetworkError> {
        let mut session = self.lock_session();

        if session.is_some() {
            return Ok(());
        }

        let info = service_info(&record)?;
        let fullname = info.get_fullname().to_string();

        let daemon = mdns_sd::ServiceDaemon::new().map_err(|err| {
            NetworkError::DiscoveryFailed(format!("mDNS daemon could not start: {err}"))
        })?;

        if let Err(err) = daemon.register(info) {
            let _ = daemon.shutdown();
            return Err(NetworkError::DiscoveryFailed(format!(
                "mDNS service could not be registered: {err}"
            )));
        }

        *session = Some(ActiveMdns {
            daemon,
            events: None,
            fullname,
        });

        // The receiver is installed by `browse`, which is always called next.
        Ok(())
    }

    fn browse(&self) -> Result<(), NetworkError> {
        let mut session = self.lock_session();

        let active = session.as_mut().ok_or_else(|| {
            NetworkError::DiscoveryFailed("cannot browse before advertising".into())
        })?;

        let events = active.daemon.browse(SYNC_SERVICE_TYPE).map_err(|err| {
            NetworkError::DiscoveryFailed(format!("mDNS browse could not start: {err}"))
        })?;
        active.events = Some(events);
        Ok(())
    }

    fn stop(&self) -> Result<(), NetworkError> {
        let active = self.lock_session().take();

        let Some(active) = active else {
            return Ok(());
        };

        // Unregister first so the goodbye packet goes out, then stop the
        // daemon and its thread. Both are best effort: a failed goodbye must
        // not keep the responder alive.
        if let Ok(status) = active.daemon.unregister(&active.fullname) {
            let _ = status.recv_timeout(Duration::from_millis(250));
        }
        let _ = active.daemon.shutdown();
        Ok(())
    }

    fn poll(&self) -> Vec<DiscoveryEvent> {
        let session = self.lock_session();
        let Some(active) = session.as_ref() else {
            return Vec::new();
        };
        let Some(events) = active.events.as_ref() else {
            return Vec::new();
        };

        events.try_iter().filter_map(event_from_mdns).collect()
    }
}

fn event_from_mdns(event: mdns_sd::ServiceEvent) -> Option<DiscoveryEvent> {
    match event {
        mdns_sd::ServiceEvent::ServiceResolved(service) => {
            Some(DiscoveryEvent::Resolved(ResolvedService {
                service_type: service.ty_domain.clone(),
                fullname: service.fullname.clone(),
                port: service.port,
                addresses: service
                    .addresses
                    .iter()
                    .map(mdns_sd::ScopedIp::to_ip_addr)
                    .collect(),
                properties: service
                    .txt_properties
                    .iter()
                    .map(|property| (property.key().to_string(), property.val_str().to_string()))
                    .collect(),
            }))
        }
        mdns_sd::ServiceEvent::ServiceRemoved(_service_type, fullname) => {
            Some(DiscoveryEvent::Removed { fullname })
        }
        // Browse bookkeeping events carry no peer information.
        _ => None,
    }
}

/// Turn a resolved mDNS service into a peer, or reject it.
///
/// Discovery is unauthenticated by design, so everything here is untrusted
/// input: a record with no usable device id or address is ignored rather than
/// surfaced as a peer.
fn peer_from_service(
    service: &ResolvedService,
    own_device_id: DeviceId,
) -> Option<(DeviceId, PeerEndpoint)> {
    if !service.service_type.eq_ignore_ascii_case(SYNC_SERVICE_TYPE) {
        return None;
    }

    let device_id = service
        .properties
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(DEVICE_ID_KEY))
        .and_then(|(_, value)| Uuid::parse_str(value.trim()).ok())?;

    // Our own advertisement is not a peer.
    if device_id == own_device_id {
        return None;
    }

    // Prefer IPv4 when a host publishes both, and order the rest
    // deterministically so discovery output is stable.
    let address = service
        .addresses
        .iter()
        .copied()
        .min_by_key(|address| (address.is_ipv6(), *address))?;

    Some((device_id, endpoint_for(&address, service.port)))
}

fn endpoint_for(address: &IpAddr, port: u16) -> PeerEndpoint {
    match address {
        IpAddr::V4(address) => PeerEndpoint::new(format!("{address}:{port}")),
        IpAddr::V6(address) => PeerEndpoint::new(format!("[{address}]:{port}")),
    }
}

#[derive(Default)]
struct SessionState {
    active: bool,
    peers: BTreeMap<DeviceId, DiscoveredPeer>,
    fullnames: HashMap<String, DeviceId>,
}

/// mDNS discovery for one Device.
///
/// Inert until [`PeerNetwork::start_advertising`] (or
/// [`MdnsDiscovery::start_session`]) is called, and inert again after
/// [`PeerNetwork::stop_advertising`]. No daemon, socket, or background thread
/// exists outside an active session.
pub struct MdnsDiscovery {
    device_id: DeviceId,
    port: u16,
    adapter: Arc<dyn DiscoveryAdapter>,
    state: Mutex<SessionState>,
}

impl MdnsDiscovery {
    pub fn new(device_id: DeviceId) -> Self {
        Self::with_port(device_id, SYNC_TRANSPORT_PORT)
    }

    /// Announce a specific transport port. The peer transport will use this
    /// once it exists; the default is [`SYNC_TRANSPORT_PORT`].
    pub fn with_port(device_id: DeviceId, port: u16) -> Self {
        Self::with_adapter(device_id, port, Arc::new(MdnsSdAdapter::new()))
    }

    pub(crate) fn with_adapter(
        device_id: DeviceId,
        port: u16,
        adapter: Arc<dyn DiscoveryAdapter>,
    ) -> Self {
        Self {
            device_id,
            port,
            adapter,
            state: Mutex::new(SessionState::default()),
        }
    }

    /// Start advertising and browsing for the duration of a session.
    pub fn start_session(self: &Arc<Self>) -> Result<DiscoverySession, NetworkError> {
        let network: Arc<dyn PeerNetwork> = self.clone();
        DiscoverySession::start(&network)
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, SessionState> {
        // The state is a small, self-contained state machine: recovering a
        // poisoned lock is safe, and it means a panic elsewhere can never
        // leave a responder running because Drop could not take the lock.
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl PeerNetwork for MdnsDiscovery {
    fn start_advertising(&self) -> Result<(), NetworkError> {
        let mut state = self.lock_state();
        if state.active {
            return Ok(());
        }

        let record =
            ServiceRecord::from_advertisement(&PeerAdvertisement::new(self.device_id), self.port);

        // Advertising and browsing are one session. If either half fails,
        // tear down whatever was created so a half-started session cannot
        // leave a responder behind.
        if let Err(err) = self.adapter.advertise(record) {
            let _ = self.adapter.stop();
            return Err(err);
        }
        if let Err(err) = self.adapter.browse() {
            let _ = self.adapter.stop();
            return Err(err);
        }

        state.active = true;
        state.peers.clear();
        state.fullnames.clear();
        Ok(())
    }

    fn stop_advertising(&self) -> Result<(), NetworkError> {
        let mut state = self.lock_state();
        if !state.active {
            return Ok(());
        }

        state.active = false;
        state.peers.clear();
        state.fullnames.clear();
        self.adapter.stop()
    }

    fn discover(&self) -> Result<Vec<DiscoveredPeer>, NetworkError> {
        let mut state = self.lock_state();
        if !state.active {
            return Ok(Vec::new());
        }

        for event in self.adapter.poll() {
            match event {
                DiscoveryEvent::Resolved(service) => {
                    let Some((device_id, endpoint)) = peer_from_service(&service, self.device_id)
                    else {
                        continue;
                    };
                    state
                        .fullnames
                        .insert(service.fullname.to_ascii_lowercase(), device_id);
                    state.peers.insert(
                        device_id,
                        DiscoveredPeer::new(PeerAdvertisement::new(device_id), endpoint),
                    );
                }
                DiscoveryEvent::Removed { fullname } => {
                    if let Some(device_id) = state.fullnames.remove(&fullname.to_ascii_lowercase())
                    {
                        // A renamed instance can briefly leave two names for
                        // one Device; only forget the peer when none remain.
                        if !state.fullnames.values().any(|id| *id == device_id) {
                            state.peers.remove(&device_id);
                        }
                    }
                }
            }
        }

        Ok(state.peers.values().cloned().collect())
    }

    fn connect(&self, _peer: &DiscoveredPeer) -> Result<Box<dyn PeerChannel>, NetworkError> {
        Err(NetworkError::TransportUnavailable(
            "the mDNS implementation discovers peers; the secure peer transport is not wired up"
                .into(),
        ))
    }

    fn accept(&self) -> Result<Option<Box<dyn PeerChannel>>, NetworkError> {
        Err(NetworkError::TransportUnavailable(
            "the mDNS implementation discovers peers; the secure peer transport is not wired up"
                .into(),
        ))
    }
}

impl Drop for MdnsDiscovery {
    fn drop(&mut self) {
        let _ = self.stop_advertising();
    }
}

/// A scoped discovery session: constructing it starts advertising and
/// browsing, dropping it stops both. Holding this guard is the only way the
/// app should keep discovery open, so an idle app cannot leave a responder
/// behind.
///
/// It works over the [`PeerNetwork`] seam rather than a concrete mDNS
/// implementation, so tests (and any future transport) get the same
/// start-and-stop guarantee.
pub struct DiscoverySession {
    network: Arc<dyn PeerNetwork>,
}

impl DiscoverySession {
    pub fn start(network: &Arc<dyn PeerNetwork>) -> Result<Self, NetworkError> {
        network.start_advertising()?;
        Ok(Self {
            network: Arc::clone(network),
        })
    }

    pub fn network(&self) -> &Arc<dyn PeerNetwork> {
        &self.network
    }

    /// End the session explicitly. Dropping has the same effect.
    pub fn finish(self) {
        let _ = self.network.stop_advertising();
    }
}

impl Drop for DiscoverySession {
    fn drop(&mut self) {
        let _ = self.network.stop_advertising();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn advertisement() -> PeerAdvertisement {
        PeerAdvertisement::new(
            uuid::Uuid::parse_str("6f9619ff-8b86-d011-b42d-00c04fc964ff").unwrap(),
        )
    }

    #[derive(Default)]
    struct FakeState {
        advertised: Vec<ServiceRecord>,
        browsed: usize,
        stops: usize,
        events: Vec<DiscoveryEvent>,
        fail_advertise: bool,
        fail_browse: bool,
    }

    /// Records what the discovery logic asks of mDNS, so lifecycle can be
    /// asserted without a socket or a multicast packet.
    #[derive(Default)]
    struct FakeAdapter {
        state: Mutex<FakeState>,
    }

    impl FakeAdapter {
        fn state(&self) -> std::sync::MutexGuard<'_, FakeState> {
            self.state.lock().unwrap()
        }

        fn advertised(&self) -> Vec<ServiceRecord> {
            self.state().advertised.clone()
        }

        fn browse_count(&self) -> usize {
            self.state().browsed
        }

        fn stop_count(&self) -> usize {
            self.state().stops
        }

        fn push_event(&self, event: DiscoveryEvent) {
            self.state().events.push(event);
        }

        fn fail_next_advertise(&self) {
            self.state().fail_advertise = true;
        }

        fn fail_next_browse(&self) {
            self.state().fail_browse = true;
        }
    }

    impl DiscoveryAdapter for FakeAdapter {
        fn advertise(&self, record: ServiceRecord) -> Result<(), NetworkError> {
            let mut state = self.state();
            if state.fail_advertise {
                state.fail_advertise = false;
                return Err(NetworkError::DiscoveryFailed("advertise failed".into()));
            }
            state.advertised.push(record);
            Ok(())
        }

        fn browse(&self) -> Result<(), NetworkError> {
            let mut state = self.state();
            if state.fail_browse {
                state.fail_browse = false;
                return Err(NetworkError::DiscoveryFailed("browse failed".into()));
            }
            state.browsed += 1;
            Ok(())
        }

        fn stop(&self) -> Result<(), NetworkError> {
            self.state().stops += 1;
            Ok(())
        }

        fn poll(&self) -> Vec<DiscoveryEvent> {
            std::mem::take(&mut self.state().events)
        }
    }

    fn network(adapter: Arc<FakeAdapter>) -> Arc<MdnsDiscovery> {
        Arc::new(MdnsDiscovery::with_adapter(
            advertisement().device_id(),
            SYNC_TRANSPORT_PORT,
            adapter,
        ))
    }

    fn resolved_peer(device_id: DeviceId, address: &str, port: u16) -> DiscoveryEvent {
        DiscoveryEvent::Resolved(ResolvedService {
            service_type: SYNC_SERVICE_TYPE.to_string(),
            fullname: format!("lockr-{device_id}.{SYNC_SERVICE_TYPE}"),
            port,
            addresses: vec![address.parse().unwrap()],
            properties: vec![("device_id".to_string(), device_id.to_string())],
        })
    }

    #[test]
    fn test_the_advertised_record_carries_a_service_type_and_an_opaque_device_id_and_nothing_else()
    {
        let advertisement = advertisement();
        let device_id = advertisement.device_id();

        let record = ServiceRecord::from_advertisement(&advertisement, SYNC_TRANSPORT_PORT);

        assert_eq!(record.service_type, SYNC_SERVICE_TYPE);
        assert_eq!(record.port, SYNC_TRANSPORT_PORT);
        assert_eq!(
            record.properties,
            vec![("device_id".to_string(), device_id.to_string())],
            "the TXT record may carry a device id and nothing else"
        );

        // The instance and host labels are structural mDNS names, and are
        // derived only from the opaque id. In particular the machine name is
        // never published.
        assert_eq!(record.instance_name, format!("lockr-{device_id}"));
        assert_eq!(record.host_name, format!("lockr-{device_id}.local."));
    }

    #[test]
    fn test_the_published_dns_record_has_exactly_one_device_id_txt_property() {
        let advertisement = advertisement();
        let device_id = advertisement.device_id();
        let record = ServiceRecord::from_advertisement(&advertisement, SYNC_TRANSPORT_PORT);

        let info = service_info(&record).unwrap();

        assert_eq!(info.get_type(), SYNC_SERVICE_TYPE);
        assert_eq!(info.get_port(), SYNC_TRANSPORT_PORT);
        assert_eq!(
            info.get_property_val_str("device_id"),
            Some(device_id.to_string().as_str())
        );
        assert_eq!(
            info.get_properties().len(),
            1,
            "a passive observer must learn only that Lockr is running and which opaque Device it is"
        );
        assert_eq!(
            info.get_hostname(),
            format!("lockr-{device_id}.local."),
            "the machine host name must never be published"
        );
    }

    #[test]
    fn test_advertising_and_browsing_start_with_a_session_and_stop_when_it_ends() {
        let adapter = Arc::new(FakeAdapter::default());
        let network = network(Arc::clone(&adapter));

        // Idle: no responder has been created and nothing is discovered.
        assert!(adapter.advertised().is_empty());
        assert_eq!(adapter.browse_count(), 0);
        assert!(network.discover().unwrap().is_empty());

        let session = network.start_session().unwrap();

        let advertised = adapter.advertised();
        assert_eq!(advertised.len(), 1, "the session advertises exactly once");
        assert_eq!(
            advertised[0].properties,
            vec![(
                "device_id".to_string(),
                advertisement().device_id().to_string()
            )]
        );
        assert_eq!(
            adapter.browse_count(),
            1,
            "a discovery session browses as well as advertises"
        );

        session.finish();

        assert_eq!(adapter.stop_count(), 1, "the session must end advertising");

        // Events that arrive after the session cannot resurrect it.
        adapter.push_event(resolved_peer(Uuid::new_v4(), "192.168.1.20", 47821));
        assert!(network.discover().unwrap().is_empty());
    }

    #[test]
    fn test_dropping_the_session_stops_advertising() {
        let adapter = Arc::new(FakeAdapter::default());
        let network = network(Arc::clone(&adapter));
        let network: Arc<dyn PeerNetwork> = network;

        {
            let _session = DiscoverySession::start(&network).unwrap();
            assert_eq!(adapter.browse_count(), 1);
        }

        assert_eq!(
            adapter.stop_count(),
            1,
            "dropping the guard must shut the responder down"
        );
    }

    #[test]
    fn test_a_discovered_peer_resolves_to_an_address_and_an_opaque_device_id() {
        let adapter = Arc::new(FakeAdapter::default());
        let network = network(Arc::clone(&adapter));
        let network: Arc<dyn PeerNetwork> = network;
        let peer_id = Uuid::new_v4();

        let session = DiscoverySession::start(&network).unwrap();
        adapter.push_event(resolved_peer(peer_id, "192.168.1.42", 47821));

        let peers = session.network().discover().unwrap();

        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].device_id(), peer_id);
        assert_eq!(peers[0].advertisement().service_type(), SYNC_SERVICE_TYPE);
        assert_eq!(peers[0].advertisement().device_id(), peer_id);

        // The address travels with the peer to connect(), but is deliberately
        // absent from everything the sync engine can print or inspect.
        assert_eq!(
            peer_from_service(
                &ResolvedService {
                    service_type: SYNC_SERVICE_TYPE.to_string(),
                    fullname: format!("lockr-{peer_id}.{SYNC_SERVICE_TYPE}"),
                    port: 47821,
                    addresses: vec!["192.168.1.42".parse().unwrap()],
                    properties: vec![("device_id".to_string(), peer_id.to_string())],
                },
                advertisement().device_id(),
            )
            .unwrap()
            .1
            .as_str(),
            "192.168.1.42:47821"
        );
        assert!(!format!("{:?}", peers[0]).contains("192.168.1.42"));
    }

    #[test]
    fn test_ipv4_is_preferred_and_ipv6_endpoints_are_bracketed() {
        let peer_id = Uuid::new_v4();
        let service = ResolvedService {
            service_type: SYNC_SERVICE_TYPE.to_string(),
            fullname: format!("lockr-{peer_id}.{SYNC_SERVICE_TYPE}"),
            port: 47821,
            addresses: vec!["fe80::1".parse().unwrap(), "192.168.1.42".parse().unwrap()],
            properties: vec![("device_id".to_string(), peer_id.to_string())],
        };

        let (_, endpoint) = peer_from_service(&service, advertisement().device_id()).unwrap();
        assert_eq!(endpoint.as_str(), "192.168.1.42:47821");

        let v6: IpAddr = "fe80::1".parse().unwrap();
        assert_eq!(endpoint_for(&v6, 47821).as_str(), "[fe80::1]:47821");
    }

    #[test]
    fn test_a_removed_service_stops_being_discoverable() {
        let adapter = Arc::new(FakeAdapter::default());
        let network = network(Arc::clone(&adapter));
        let peer_id = Uuid::new_v4();

        let _session = network.start_session().unwrap();
        adapter.push_event(resolved_peer(peer_id, "192.168.1.42", 47821));
        assert_eq!(network.discover().unwrap().len(), 1);

        adapter.push_event(DiscoveryEvent::Removed {
            fullname: format!("lockr-{peer_id}.{SYNC_SERVICE_TYPE}"),
        });

        assert!(network.discover().unwrap().is_empty());
    }

    #[test]
    fn test_a_failed_start_leaves_the_app_invisible() {
        let adapter = Arc::new(FakeAdapter::default());
        adapter.fail_next_browse();
        let network = network(Arc::clone(&adapter));

        let error = network.start_session().err().unwrap();

        assert!(matches!(error, NetworkError::DiscoveryFailed(_)));
        assert_eq!(
            adapter.stop_count(),
            1,
            "whatever started must be torn down again"
        );
        assert!(network.discover().unwrap().is_empty());

        // The failed session never became active, so ending it is a no-op.
        network.stop_advertising().unwrap();
        assert_eq!(adapter.stop_count(), 1);
    }

    #[test]
    fn test_a_failed_advertisement_leaves_the_app_invisible() {
        let adapter = Arc::new(FakeAdapter::default());
        adapter.fail_next_advertise();
        let network = network(Arc::clone(&adapter));

        let error = network.start_session().err().unwrap();

        assert!(matches!(error, NetworkError::DiscoveryFailed(_)));
        assert_eq!(adapter.browse_count(), 0, "browsing must not start");
        assert_eq!(adapter.stop_count(), 1);
        assert!(network.discover().unwrap().is_empty());
    }

    #[test]
    fn test_records_without_a_usable_device_id_are_ignored() {
        let adapter = Arc::new(FakeAdapter::default());
        let network = network(Arc::clone(&adapter));
        let own_id = advertisement().device_id();

        let _session = network.start_session().unwrap();

        let mut missing_id = resolved_peer(Uuid::new_v4(), "192.168.1.10", 47821);
        if let DiscoveryEvent::Resolved(service) = &mut missing_id {
            service.properties.clear();
        }

        let mut malformed_id = resolved_peer(Uuid::new_v4(), "192.168.1.11", 47821);
        if let DiscoveryEvent::Resolved(service) = &mut malformed_id {
            service.properties = vec![("device_id".to_string(), "not-a-uuid".to_string())];
        }

        let mut wrong_service = resolved_peer(Uuid::new_v4(), "192.168.1.12", 47821);
        if let DiscoveryEvent::Resolved(service) = &mut wrong_service {
            service.service_type = "_something-else._tcp.local.".to_string();
        }

        let own_advertisement = resolved_peer(own_id, "192.168.1.13", 47821);

        for event in [missing_id, malformed_id, wrong_service, own_advertisement] {
            adapter.push_event(event);
        }

        assert!(network.discover().unwrap().is_empty());
    }
}
