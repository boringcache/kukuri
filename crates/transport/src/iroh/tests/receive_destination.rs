use super::*;
use crate::receive_binding::{RECEIVE_BINDING_ALPN, ReceiveBindingProtocol};
use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler};
use kukuri_core::{KukuriKeys, ReceiveEndpointBindingV1};

#[derive(Debug)]
struct StalledDestinationBinding {
    requested: Arc<Notify>,
    closed: Arc<Notify>,
}

impl ProtocolHandler for StalledDestinationBinding {
    async fn accept(&self, connection: Connection) -> std::result::Result<(), AcceptError> {
        let (_send, mut recv) = connection.accept_bi().await?;
        recv.read_to_end(1).await.map_err(AcceptError::from_err)?;
        self.requested.notify_one();
        connection.closed().await;
        self.closed.notify_one();
        Ok(())
    }
}

#[test]
fn destination_window_rotates_through_large_peer_history_in_four_candidate_steps() {
    let mut peers = BTreeMap::new();
    for index in 0..1_000u32 {
        let mut secret = [0u8; 32];
        secret[..4].copy_from_slice(&index.to_le_bytes());
        let endpoint_id = SecretKey::from_bytes(&secret).public();
        peers.insert(endpoint_id.to_string(), EndpointAddr::new(endpoint_id));
    }
    let recipient = Pubkey::from("account-a");
    let mut state = DestinationWindow::default();
    let mut observed = BTreeSet::new();
    for _ in 0..250 {
        let (candidates, _) =
            state.select(&recipient, [&peers, &BTreeMap::new(), &BTreeMap::new()]);
        assert!(candidates.len() <= CANDIDATES_PER_LOOKUP);
        observed.extend(candidates.into_iter().map(|candidate| candidate.id));
    }
    assert_eq!(observed.len(), 1_000);
}

#[test]
fn invalidation_rejects_stale_lookup_and_state_has_a_fixed_account_cap() {
    let recipient = Pubkey::from("account-a");
    let endpoint_id = SecretKey::from_bytes(&[7; 32]).public();
    let address = EndpointAddr::new(endpoint_id);
    let mut peers = BTreeMap::new();
    peers.insert(endpoint_id.to_string(), address.clone());
    let mut state = DestinationWindow::default();
    let (_, revision) = state.select(&recipient, [&peers, &BTreeMap::new(), &BTreeMap::new()]);
    state.invalidate(&recipient, &endpoint_id.to_string());
    assert!(!state.store_verified(
        &recipient,
        revision,
        address.clone(),
        i64::MAX,
        Instant::now() + Duration::from_secs(1),
    ));
    for index in 0..MAX_DESTINATION_ACCOUNTS + 1 {
        state.touch(&Pubkey::from(format!("account-{index}")));
    }
    assert_eq!(state.entries.len(), MAX_DESTINATION_ACCOUNTS);
    assert!(!state.entries.contains_key(&recipient));
}

#[test]
fn destination_cursor_reaches_old_peer_during_new_inserts_and_deletes() {
    let recipient = Pubkey::from("account-b");
    let mut peers = BTreeMap::new();
    let mut old = None;
    for index in 0..1_000u32 {
        let mut secret = [0u8; 32];
        secret[..4].copy_from_slice(&index.to_le_bytes());
        let id = SecretKey::from_bytes(&secret).public();
        peers.insert(id.to_string(), EndpointAddr::new(id));
        if index == 999 {
            old = Some(id);
        }
    }
    let target = old.unwrap();
    let mut state = DestinationWindow::default();
    let mut reached = false;
    for index in 1_000..1_350u32 {
        let mut secret = [0u8; 32];
        secret[..4].copy_from_slice(&index.to_le_bytes());
        let id = SecretKey::from_bytes(&secret).public();
        peers.insert(id.to_string(), EndpointAddr::new(id));
        let (candidates, _) =
            state.select(&recipient, [&peers, &BTreeMap::new(), &BTreeMap::new()]);
        reached |= candidates.iter().any(|candidate| candidate.id == target);
        let first = peers.keys().next().unwrap().clone();
        if first != target.to_string() {
            peers.remove(&first);
        }
    }
    assert!(reached, "an old unprocessed peer must not starve");
}

#[test]
fn cache_expires_and_invalidating_another_endpoint_preserves_current_binding() {
    let recipient = Pubkey::from("account-c");
    let id = SecretKey::from_bytes(&[8; 32]).public();
    let address = EndpointAddr::new(id);
    let mut state = DestinationWindow::default();
    let (_, revision) = state.select(
        &recipient,
        [&BTreeMap::new(), &BTreeMap::new(), &BTreeMap::new()],
    );
    assert!(state.store_verified(
        &recipient,
        revision,
        address.clone(),
        100,
        Instant::now() + Duration::from_secs(1),
    ));
    state.invalidate(
        &recipient,
        &SecretKey::from_bytes(&[9; 32]).public().to_string(),
    );
    assert_eq!(state.cached(&recipient, 99).unwrap().id, id);
    assert!(state.cached(&recipient, 100).is_none());
    let (_, revision) = state.select(
        &recipient,
        [&BTreeMap::new(), &BTreeMap::new(), &BTreeMap::new()],
    );
    assert!(state.store_verified(
        &recipient,
        revision,
        address,
        i64::MAX,
        Instant::now() - Duration::from_secs(1),
    ));
    assert!(state.cached(&recipient, 99).is_none());
}

#[test]
fn invalidating_old_probe_cannot_replace_newer_verified_endpoint() {
    let recipient = Pubkey::from("account-d");
    let old_id = SecretKey::from_bytes(&[10; 32]).public();
    let new_id = SecretKey::from_bytes(&[11; 32]).public();
    let mut state = DestinationWindow::default();
    let (_, old_revision) = state.select(
        &recipient,
        [&BTreeMap::new(), &BTreeMap::new(), &BTreeMap::new()],
    );
    assert!(state.store_verified(
        &recipient,
        old_revision,
        EndpointAddr::new(new_id),
        i64::MAX,
        Instant::now() + Duration::from_secs(1),
    ));
    state.invalidate(&recipient, &old_id.to_string());
    assert!(!state.store_verified(
        &recipient,
        old_revision,
        EndpointAddr::new(old_id),
        i64::MAX,
        Instant::now() + Duration::from_secs(1),
    ));
    assert_eq!(state.cached(&recipient, 0).unwrap().id, new_id);
}

#[tokio::test]
async fn shutdown_during_cache_lock_wait_does_not_return_cached_destination() {
    let transport = Arc::new(IrohGossipTransport::bind_local().await.unwrap());
    let recipient = KukuriKeys::generate().public_key();
    let id = SecretKey::from_bytes(&[12; 32]).public();
    let mut state = transport.receive_destinations.lock().await;
    let (_, revision) = state.select(
        &recipient,
        [&BTreeMap::new(), &BTreeMap::new(), &BTreeMap::new()],
    );
    assert!(state.store_verified(
        &recipient,
        revision,
        EndpointAddr::new(id),
        i64::MAX,
        Instant::now() + Duration::from_secs(10),
    ));
    let resolving = Arc::clone(&transport);
    let account = recipient.clone();
    let mut task =
        tokio::spawn(async move { resolving.resolve_receive_destination(&account).await });
    assert!(timeout(Duration::from_millis(20), &mut task).await.is_err());
    transport.shutdown().await;
    drop(state);
    assert!(task.await.unwrap().unwrap().is_none());
    let mut transport = Arc::try_unwrap(transport).ok().unwrap();
    transport._router.take().unwrap().shutdown().await.unwrap();
}

#[tokio::test]
async fn shutdown_cancels_active_binding_probe_and_closes_connection() {
    let transport = Arc::new(IrohGossipTransport::bind_local().await.unwrap());
    let receiver = Endpoint::builder(iroh::endpoint::presets::Minimal)
        .relay_mode(RelayMode::Disabled)
        .bind_addr("127.0.0.1:0".parse::<SocketAddr>().unwrap())
        .unwrap()
        .bind()
        .await
        .unwrap();
    let requested = Arc::new(Notify::new());
    let closed = Arc::new(Notify::new());
    let router = Router::builder(receiver.clone())
        .accept(
            RECEIVE_BINDING_ALPN,
            StalledDestinationBinding {
                requested: requested.clone(),
                closed: closed.clone(),
            },
        )
        .spawn();
    transport
        .imported_peers
        .lock()
        .await
        .insert(receiver.id().to_string(), receiver.addr());
    let recipient = KukuriKeys::generate().public_key();
    let resolving = Arc::clone(&transport);
    let task = tokio::spawn(async move { resolving.resolve_receive_destination(&recipient).await });
    timeout(Duration::from_secs(5), requested.notified())
        .await
        .unwrap();
    transport.shutdown().await;
    assert!(
        timeout(Duration::from_millis(500), task)
            .await
            .expect("shutdown must cancel lookup before candidate deadline")
            .unwrap()
            .unwrap()
            .is_none()
    );
    timeout(Duration::from_secs(2), closed.notified())
        .await
        .expect("shutdown must close the active binding connection");
    router.shutdown().await.unwrap();
    let mut transport = Arc::try_unwrap(transport).ok().unwrap();
    transport._router.take().unwrap().shutdown().await.unwrap();
}

#[tokio::test]
async fn destination_requires_live_binding_for_the_exact_account_and_invalidates_cache() {
    let mut transport = IrohGossipTransport::bind_local().await.unwrap();
    let receiver = Endpoint::builder(iroh::endpoint::presets::Minimal)
        .relay_mode(RelayMode::Disabled)
        .bind_addr("127.0.0.1:0".parse::<SocketAddr>().unwrap())
        .unwrap()
        .bind()
        .await
        .unwrap();
    let recipient = KukuriKeys::generate();
    let other = KukuriKeys::generate();
    let now = Utc::now().timestamp_millis();
    let binding =
        ReceiveEndpointBindingV1::sign(&recipient, &receiver.id().to_string(), now, now + 60_000)
            .unwrap();
    let handler = ReceiveBindingProtocol::new(receiver.id(), binding).unwrap();
    let router = Router::builder(receiver.clone())
        .accept(RECEIVE_BINDING_ALPN, handler)
        .spawn();
    transport
        .imported_peers
        .lock()
        .await
        .insert(receiver.id().to_string(), receiver.addr());

    assert!(
        transport
            .resolve_receive_destination(&other.public_key())
            .await
            .unwrap()
            .is_none()
    );
    let resolved = transport
        .resolve_receive_destination(&recipient.public_key())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resolved.id, receiver.id());
    transport
        .verify_receive_provider(&recipient.public_key(), receiver.addr())
        .await
        .unwrap();
    assert!(
        transport
            .verify_receive_provider(&other.public_key(), receiver.addr())
            .await
            .is_err()
    );
    assert_eq!(
        transport
            .resolve_receive_destination(&recipient.public_key())
            .await
            .unwrap()
            .unwrap()
            .id,
        receiver.id()
    );

    transport
        .invalidate_receive_destination(&recipient.public_key(), &receiver.id().to_string())
        .await
        .unwrap();
    assert!(
        transport
            .receive_destinations
            .lock()
            .await
            .cached(&recipient.public_key(), Utc::now().timestamp_millis())
            .is_none()
    );
    router.shutdown().await.unwrap();
    transport.shutdown().await;
    transport._router.take().unwrap().shutdown().await.unwrap();
}

#[tokio::test]
async fn saturated_probe_budget_defers_without_queuing() {
    let mut transport = IrohGossipTransport::bind_local().await.unwrap();
    let permits = transport
        .receive_destination_probes
        .acquire_many(2)
        .await
        .unwrap();
    let recipient = KukuriKeys::generate().public_key();
    assert!(
        transport
            .resolve_receive_destination(&recipient)
            .await
            .unwrap()
            .is_none()
    );
    drop(permits);
    transport.shutdown().await;
    transport._router.take().unwrap().shutdown().await.unwrap();
}
