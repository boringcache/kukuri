use super::*;

use kukuri_core::{
    BlobHash, KukuriKeys, RECEIVE_OFFER_MAX_BYTES, ReceiveOfferReferenceV1, ReceiveOfferScopeV1,
    SealedReceiveOfferV1, receive_epoch_key_id, seal_private_receive_payload, seal_receive_offer,
};

struct NotifyOnDrop(Arc<Notify>);

impl Drop for NotifyOnDrop {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn account_receive_offer_crosses_real_gossip_with_one_recipient_route() {
    let mut left = IrohGossipTransport::bind_local().await.unwrap();
    let mut right = IrohGossipTransport::bind_local().await.unwrap();
    left.discovery.add_endpoint_info(right.endpoint.addr());
    right.discovery.add_endpoint_info(left.endpoint.addr());
    let sender = KukuriKeys::generate();
    let recipient = KukuriKeys::generate();
    let (lease, mut incoming, _) = right
        .subscribe_receive_offers(&recipient.public_key())
        .await
        .unwrap();

    let epoch_secret = [7; 32];
    let private_payload =
        seal_private_receive_payload(&epoch_secret, "channel-a", "epoch-1", &vec![42; 16_384])
            .unwrap();
    let payload_bytes = private_payload.encode().unwrap();
    assert!(
        payload_bytes.len() > 4096,
        "the referenced payload cannot fit in gossip"
    );
    let epoch_key_id = receive_epoch_key_id(&epoch_secret, "channel-a", "epoch-1").unwrap();
    let scopes = [
        ReceiveOfferScopeV1::PublicSource,
        ReceiveOfferScopeV1::DirectMessage,
        ReceiveOfferScopeV1::PrivateSource {
            epoch_key_id: epoch_key_id.clone(),
        },
        ReceiveOfferScopeV1::EpochControl { epoch_key_id },
    ];
    for scope in scopes {
        let now = chrono::Utc::now().timestamp_millis();
        let reference = ReceiveOfferReferenceV1 {
            provider_endpoint_id: left.endpoint.id().to_string(),
            payload_hash: BlobHash(blake3::hash(&payload_bytes).to_hex().to_string()),
            payload_bytes: payload_bytes.len() as u32,
            scope,
        };
        let offer = seal_receive_offer(
            &sender,
            &recipient.public_key(),
            reference.clone(),
            now,
            now + 60_000,
        )
        .unwrap();
        let wire = offer.encode().unwrap();
        assert!(wire.len() <= RECEIVE_OFFER_MAX_BYTES);
        left.publish_receive_offer(&recipient.public_key(), right.endpoint.addr(), offer)
            .await
            .unwrap();
        let received = timeout(Duration::from_secs(5), incoming.next())
            .await
            .expect("receive offer timeout")
            .expect("receive offer stream ended");
        assert_eq!(received.source_peer, left.endpoint.id().to_string());
        let sealed = SealedReceiveOfferV1::decode(&received.offer.encode().unwrap()).unwrap();
        let opened = sealed.open(&recipient, now).unwrap();
        assert_eq!(opened.sender(), &sender.public_key());
        assert_eq!(opened.reference(), &reference);
        assert!(sealed.open(&KukuriKeys::generate(), now).is_err());
    }

    right
        .unsubscribe_receive_offers(&recipient.public_key(), lease)
        .await
        .unwrap();
    drop(incoming);
    left.shutdown().await;
    right.shutdown().await;
    left._router.take().unwrap().shutdown().await.unwrap();
    right._router.take().unwrap().shutdown().await.unwrap();
}

#[tokio::test]
async fn account_receive_route_replaces_the_previous_account_subscription() {
    let mut transport = IrohGossipTransport::bind_local().await.unwrap();
    let old = KukuriKeys::generate().public_key();
    let current = KukuriKeys::generate().public_key();
    let (old_lease, _old_stream, _) = transport.subscribe_receive_offers(&old).await.unwrap();
    let (current_lease, mut current_stream, _) =
        transport.subscribe_receive_offers(&current).await.unwrap();
    assert_eq!(
        transport
            .receive_offer_topic
            .lock()
            .await
            .as_ref()
            .unwrap()
            .route,
        kukuri_core::receive_route_for_account(&current)
            .unwrap()
            .as_str()
    );
    transport
        .unsubscribe_receive_offers(&old, old_lease)
        .await
        .unwrap();
    assert!(transport.receive_offer_topic.lock().await.is_some());
    transport
        .unsubscribe_receive_offers(&current, current_lease)
        .await
        .unwrap();
    assert!(transport.receive_offer_topic.lock().await.is_none());
    assert!(
        timeout(Duration::from_millis(100), current_stream.next())
            .await
            .unwrap()
            .is_none()
    );
    transport.shutdown().await;
    transport._router.take().unwrap().shutdown().await.unwrap();
}

#[tokio::test]
async fn stale_same_account_lease_cannot_unsubscribe_a_new_receiver() {
    let mut transport = IrohGossipTransport::bind_local().await.unwrap();
    let recipient = KukuriKeys::generate().public_key();
    let (old_lease, mut old_stream, _) = transport
        .subscribe_receive_offers(&recipient)
        .await
        .unwrap();
    let (new_lease, mut new_stream, _) = transport
        .subscribe_receive_offers(&recipient)
        .await
        .unwrap();
    assert_ne!(old_lease, new_lease);
    assert!(
        timeout(Duration::from_millis(100), old_stream.next())
            .await
            .unwrap()
            .is_none()
    );
    transport
        .unsubscribe_receive_offers(&recipient, old_lease)
        .await
        .unwrap();
    assert!(transport.receive_offer_topic.lock().await.is_some());
    assert!(
        transport
            .resubscribe_receive_offers_if_current(&recipient, old_lease)
            .await
            .unwrap()
            .is_none(),
        "superseded receiver cannot reclaim the route"
    );
    let (next_lease, mut next_stream, _) = transport
        .resubscribe_receive_offers_if_current(&recipient, new_lease)
        .await
        .unwrap()
        .expect("current receiver may restart");
    assert_ne!(new_lease, next_lease);
    transport
        .unsubscribe_receive_offers(&recipient, new_lease)
        .await
        .unwrap();
    assert!(transport.receive_offer_topic.lock().await.is_some());
    transport
        .unsubscribe_receive_offers(&recipient, next_lease)
        .await
        .unwrap();
    assert!(transport.receive_offer_topic.lock().await.is_none());
    assert!(
        timeout(Duration::from_millis(100), new_stream.next())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        timeout(Duration::from_millis(100), next_stream.next())
            .await
            .unwrap()
            .is_none()
    );
    transport.shutdown().await;
    transport._router.take().unwrap().shutdown().await.unwrap();
}

#[tokio::test]
async fn oversized_receive_offer_is_rejected_before_joining_a_route() {
    let mut transport = IrohGossipTransport::bind_local().await.unwrap();
    let sender = KukuriKeys::generate();
    let recipient = KukuriKeys::generate();
    let now = chrono::Utc::now().timestamp_millis();
    let mut offer = seal_receive_offer(
        &sender,
        &recipient.public_key(),
        ReceiveOfferReferenceV1 {
            provider_endpoint_id: transport.endpoint.id().to_string(),
            payload_hash: BlobHash("11".repeat(32)),
            payload_bytes: 1,
            scope: ReceiveOfferScopeV1::PublicSource,
        },
        now,
        now + 60_000,
    )
    .unwrap();
    offer.ciphertext_hex = "ab".repeat(RECEIVE_OFFER_MAX_BYTES);
    assert!(
        transport
            .publish_receive_offer(&recipient.public_key(), transport.endpoint.addr(), offer,)
            .await
            .is_err()
    );
    assert!(transport.outbound_offer_holds.lock().await.is_empty());
    transport.shutdown().await;
    transport._router.take().unwrap().shutdown().await.unwrap();
}

#[tokio::test]
async fn account_route_bootstrap_window_is_independent_of_imported_history() {
    let mut transport = IrohGossipTransport::bind_local().await.unwrap();
    for history in [100_usize, 1_000] {
        let start = transport.imported_peers.lock().await.len();
        for index in start..history {
            let mut secret = [0_u8; 32];
            secret[..8].copy_from_slice(&(index as u64 + 1).to_be_bytes());
            let peer = iroh::SecretKey::from_bytes(&secret).public();
            transport
                .imported_peers
                .lock()
                .await
                .insert(peer.to_string(), EndpointAddr::new(peer));
        }
        let selected = transport.offer_bootstrap_window().await;
        assert_eq!(selected.len(), 4, "history={history}");
    }
    transport.shutdown().await;
    transport._router.take().unwrap().shutdown().await.unwrap();
}

#[tokio::test]
async fn cancelling_offer_subscribe_before_registration_leaves_no_receiver_task() {
    let transport = Arc::new(IrohGossipTransport::bind_local().await.unwrap());
    let recipient = KukuriKeys::generate().public_key();
    let registration_guard = transport.configured_seed_peers.lock().await;
    let subscriber = tokio::spawn({
        let transport = Arc::clone(&transport);
        async move { transport.subscribe_receive_offers(&recipient).await }
    });
    timeout(Duration::from_secs(2), async {
        while transport.receive_offer_topic.try_lock().is_ok() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    sleep(Duration::from_millis(100)).await;
    assert_eq!(
        transport.offer_receiver_tasks.load(Ordering::SeqCst),
        0,
        "a receiver task cannot start before its owner can register it"
    );
    subscriber.abort();
    let _ = subscriber.await;
    drop(registration_guard);
    assert_eq!(transport.offer_receiver_tasks.load(Ordering::SeqCst), 0);
    let mut transport = Arc::try_unwrap(transport).ok().unwrap();
    transport.shutdown().await;
    transport._router.take().unwrap().shutdown().await.unwrap();
}

#[tokio::test]
async fn cancelling_offer_publish_before_registration_leaves_no_hold_task() {
    let left = Arc::new(IrohGossipTransport::bind_local().await.unwrap());
    let mut right = IrohGossipTransport::bind_local().await.unwrap();
    left.discovery.add_endpoint_info(right.endpoint.addr());
    right.discovery.add_endpoint_info(left.endpoint.addr());
    let sender = KukuriKeys::generate();
    let recipient = KukuriKeys::generate();
    let _incoming = right
        .subscribe_receive_offers(&recipient.public_key())
        .await
        .unwrap();
    let now = chrono::Utc::now().timestamp_millis();
    let offer = seal_receive_offer(
        &sender,
        &recipient.public_key(),
        ReceiveOfferReferenceV1 {
            provider_endpoint_id: left.endpoint.id().to_string(),
            payload_hash: BlobHash("11".repeat(32)),
            payload_bytes: 1,
            scope: ReceiveOfferScopeV1::PublicSource,
        },
        now,
        now + 60_000,
    )
    .unwrap();
    let registration_guard = left.outbound_offer_holds.lock().await;
    let publisher = tokio::spawn({
        let left = Arc::clone(&left);
        let recipient = recipient.public_key();
        let destination = right.endpoint.addr();
        async move {
            left.publish_receive_offer(&recipient, destination, offer)
                .await
        }
    });
    sleep(Duration::from_millis(500)).await;
    assert_eq!(
        left.offer_hold_tasks.load(Ordering::SeqCst),
        0,
        "a hold task cannot start before its owner can register it"
    );
    publisher.abort();
    let _ = publisher.await;
    drop(registration_guard);
    assert_eq!(left.offer_hold_tasks.load(Ordering::SeqCst), 0);
    let mut left = Arc::try_unwrap(left).ok().unwrap();
    left.shutdown().await;
    right.shutdown().await;
    left._router.take().unwrap().shutdown().await.unwrap();
    right._router.take().unwrap().shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_offer_shutdown_aborts_every_detached_hold_before_waiting() {
    let transport = Arc::new(IrohGossipTransport::bind_local().await.unwrap());
    let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    let blocked = tokio::task::spawn_blocking(move || {
        started_tx.send(()).unwrap();
        release_rx.recv().unwrap();
    });
    started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let dropped = Arc::new(Notify::new());
    let (running_tx, running_rx) = tokio::sync::oneshot::channel();
    let pending = tokio::spawn({
        let dropped = Arc::clone(&dropped);
        async move {
            let _drop_signal = NotifyOnDrop(dropped);
            let _ = running_tx.send(());
            std::future::pending::<()>().await;
        }
    });
    running_rx.await.unwrap();
    let expires_at = tokio::time::Instant::now() + Duration::from_secs(30);
    {
        let mut holds = transport.outbound_offer_holds.lock().await;
        holds.push_back(OutboundOfferHold {
            expires_at,
            task: blocked,
        });
        holds.push_back(OutboundOfferHold {
            expires_at,
            task: pending,
        });
    }
    let shutdown = tokio::spawn({
        let transport = Arc::clone(&transport);
        async move { transport.shutdown().await }
    });
    timeout(Duration::from_secs(2), async {
        while !transport.outbound_offer_holds.lock().await.is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    shutdown.abort();
    let _ = shutdown.await;
    release_tx.send(()).unwrap();
    timeout(Duration::from_secs(2), dropped.notified())
        .await
        .expect("all holds must be aborted before the first await");
    transport.shutdown().await;
    let mut transport = Arc::try_unwrap(transport).ok().unwrap();
    transport._router.take().unwrap().shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offer_publish_waiting_for_registration_cannot_revive_after_shutdown() {
    let left = Arc::new(IrohGossipTransport::bind_local().await.unwrap());
    let mut right = IrohGossipTransport::bind_local().await.unwrap();
    left.discovery.add_endpoint_info(right.endpoint.addr());
    right.discovery.add_endpoint_info(left.endpoint.addr());
    let sender = KukuriKeys::generate();
    let recipient = KukuriKeys::generate();
    let _incoming = right
        .subscribe_receive_offers(&recipient.public_key())
        .await
        .unwrap();
    let now = chrono::Utc::now().timestamp_millis();
    let offer = seal_receive_offer(
        &sender,
        &recipient.public_key(),
        ReceiveOfferReferenceV1 {
            provider_endpoint_id: left.endpoint.id().to_string(),
            payload_hash: BlobHash("11".repeat(32)),
            payload_bytes: 1,
            scope: ReceiveOfferScopeV1::PublicSource,
        },
        now,
        now + 60_000,
    )
    .unwrap();
    let registration_guard = left.outbound_offer_holds.lock().await;
    let publish = tokio::spawn({
        let left = Arc::clone(&left);
        let recipient = recipient.public_key();
        let destination = right.endpoint.addr();
        async move {
            left.publish_receive_offer(&recipient, destination, offer)
                .await
        }
    });
    timeout(Duration::from_secs(5), left.offer_publish_joined.notified())
        .await
        .expect("publisher must join before shutdown");
    let shutdown = tokio::spawn({
        let left = Arc::clone(&left);
        async move { left.shutdown().await }
    });
    timeout(Duration::from_secs(2), async {
        while !left.offer_closed.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    drop(registration_guard);
    assert!(publish.await.unwrap().is_err());
    shutdown.await.unwrap();
    assert!(left.outbound_offer_holds.lock().await.is_empty());
    let mut left = Arc::try_unwrap(left).ok().unwrap();
    right.shutdown().await;
    left._router.take().unwrap().shutdown().await.unwrap();
    right._router.take().unwrap().shutdown().await.unwrap();
}

#[tokio::test]
async fn offer_subscribe_waiting_for_registration_cannot_revive_after_shutdown() {
    let transport = Arc::new(IrohGossipTransport::bind_local().await.unwrap());
    let recipient = KukuriKeys::generate().public_key();
    let registration_guard = transport.configured_seed_peers.lock().await;
    let subscribe = tokio::spawn({
        let transport = Arc::clone(&transport);
        async move { transport.subscribe_receive_offers(&recipient).await }
    });
    timeout(Duration::from_secs(2), async {
        while transport.receive_offer_topic.try_lock().is_ok() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let shutdown = tokio::spawn({
        let transport = Arc::clone(&transport);
        async move { transport.shutdown().await }
    });
    timeout(Duration::from_secs(2), async {
        while !transport.offer_closed.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    drop(registration_guard);
    assert!(subscribe.await.unwrap().is_err());
    shutdown.await.unwrap();
    assert_eq!(transport.offer_receiver_tasks.load(Ordering::SeqCst), 0);
    let mut transport = Arc::try_unwrap(transport).ok().unwrap();
    transport._router.take().unwrap().shutdown().await.unwrap();
}

#[tokio::test]
async fn offer_join_wait_stops_when_transport_shuts_down() {
    let transport = Arc::new(IrohGossipTransport::bind_local().await.unwrap());
    let sender = KukuriKeys::generate();
    let recipient = KukuriKeys::generate();
    let now = chrono::Utc::now().timestamp_millis();
    let offer = seal_receive_offer(
        &sender,
        &recipient.public_key(),
        ReceiveOfferReferenceV1 {
            provider_endpoint_id: transport.endpoint.id().to_string(),
            payload_hash: BlobHash("11".repeat(32)),
            payload_bytes: 1,
            scope: ReceiveOfferScopeV1::PublicSource,
        },
        now,
        now + 60_000,
    )
    .unwrap();
    let unavailable = iroh::SecretKey::from_bytes(&[93; 32]).public();
    let publish = tokio::spawn({
        let transport = Arc::clone(&transport);
        let recipient = recipient.public_key();
        async move {
            transport
                .publish_receive_offer(&recipient, EndpointAddr::new(unavailable), offer)
                .await
        }
    });
    timeout(
        Duration::from_secs(2),
        transport.offer_publish_join_started.notified(),
    )
    .await
    .expect("publisher must start waiting for route join");
    transport.shutdown().await;
    assert!(
        timeout(Duration::from_secs(2), publish)
            .await
            .expect("shutdown must stop the join wait")
            .unwrap()
            .is_err()
    );
    assert!(transport.outbound_offer_holds.lock().await.is_empty());
    let mut transport = Arc::try_unwrap(transport).ok().unwrap();
    transport._router.take().unwrap().shutdown().await.unwrap();
}
