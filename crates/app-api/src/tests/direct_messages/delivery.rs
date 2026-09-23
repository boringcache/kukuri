use super::super::*;
use super::receive_offer_doubles::OfferBlobService;

#[tokio::test]
async fn dm_outbox_page_sends_sealed_account_offer_without_consuming_protected_row() {
    use kukuri_core::{BlobHash, ReceiveOfferScopeV1};
    use kukuri_store::{DirectMessageOutboxRow, DirectMessageStore};
    use kukuri_transport::EndpointAddr;

    let store = Arc::new(MemoryStore::default());
    let sender = generate_keys();
    let recipient = generate_keys();
    let local = sender.public_key_hex();
    let peer = recipient.public_key_hex();
    SocialProjectionStore::rebuild_author_relationships(
        store.as_ref(),
        &local,
        vec![AuthorRelationshipProjectionRow {
            local_author_pubkey: local.clone(),
            author_pubkey: peer.clone(),
            following: true,
            followed_by: true,
            mutual: true,
            friend_of_friend: false,
            friend_of_friend_via_pubkeys: Vec::new(),
            derived_at: 1,
        }],
    )
    .await
    .unwrap();
    let row = DirectMessageOutboxRow {
        dm_id: direct_message_id_for_participants(&sender.public_key(), &recipient.public_key()),
        message_id: "account-offer-message".into(),
        peer_pubkey: peer.clone(),
        frame_blob_hash: BlobHash::new("aa".repeat(32)),
        created_at: 42,
        last_attempt_at: None,
    };
    store.put_direct_message_outbox(row.clone()).await.unwrap();
    let destination = EndpointAddr::new(iroh::SecretKey::from_bytes(&[23; 32]).public());
    let provider = iroh::SecretKey::from_bytes(&[24; 32]).public();
    let hints = Arc::new(TrackingHintTransport::default());
    *hints.resolved_destination.lock().await = Some(destination.clone());
    let blobs = Arc::new(OfferBlobService::new(
        Arc::new(MemoryBlobService::default()),
    ));
    let services = ServiceHandles::new(
        store.clone(),
        store.clone(),
        Arc::new(
            StaticTransport::new(PeerSnapshot::default())
                .with_local_endpoint_id(provider.to_string()),
        ),
        hints.clone(),
        Arc::new(MemoryDocsSync::default()),
        blobs.clone(),
        sender.clone(),
    );
    let published = AppService::flush_due_direct_message_outbox(&services, 1_000)
        .await
        .unwrap();
    assert_eq!(published, 1);
    assert_eq!(hints.resolved_count.load(Ordering::SeqCst), 1);
    let offers = hints.offers.lock().await;
    assert_eq!(offers.len(), 1);
    assert_eq!(offers[0].0, recipient.public_key());
    assert_eq!(offers[0].1.id, destination.id);
    let opened = offers[0]
        .2
        .open(&recipient, Utc::now().timestamp_millis())
        .unwrap();
    assert_eq!(opened.sender(), &sender.public_key());
    assert_eq!(
        opened.reference().provider_endpoint_id,
        provider.to_string()
    );
    assert_eq!(opened.reference().payload_bytes, 0);
    assert!(opened.reference().payload_hash.as_str().is_empty());
    assert!(
        matches!(&opened.reference().scope, ReceiveOfferScopeV1::DirectMessageFrame { dm_id, message_id, frame_hash }
        if dm_id == &row.dm_id && message_id == &row.message_id && frame_hash == &row.frame_blob_hash)
    );
    assert_eq!(blobs.writes.load(Ordering::SeqCst), 0);
    assert!(
        store
            .get_direct_message_outbox(&row.dm_id, &row.message_id)
            .await
            .unwrap()
            .is_some()
    );
    drop(offers);
    let _busy = services
        .account_dm_offer_permits
        .acquire_many(4)
        .await
        .unwrap();
    timeout(
        Duration::from_secs(1),
        AppService::flush_due_direct_message_outbox(&services, 3_000),
    )
    .await
    .expect("busy account offer permits must defer without queuing")
    .unwrap();
    assert_eq!(hints.offers.lock().await.len(), 1);
    assert!(
        store
            .get_direct_message_outbox(&row.dm_id, &row.message_id)
            .await
            .unwrap()
            .is_some()
    );
    drop(_busy);
    hints.fail_offer_publish.store(true, Ordering::SeqCst);
    AppService::flush_due_direct_message_outbox(&services, 5_000)
        .await
        .unwrap();
    assert!(hints.resolved_destination.lock().await.is_none());
    assert_eq!(hints.offers.lock().await.len(), 1);
    assert!(
        store
            .get_direct_message_outbox(&row.dm_id, &row.message_id)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn revoked_mutual_after_destination_lookup_sends_no_account_dm_offer() {
    use kukuri_core::BlobHash;
    use kukuri_store::{DirectMessageOutboxRow, DirectMessageStore};
    use kukuri_transport::EndpointAddr;

    let store = Arc::new(MemoryStore::default());
    let sender = generate_keys();
    let recipient = generate_keys();
    let local = sender.public_key_hex();
    let peer = recipient.public_key_hex();
    SocialProjectionStore::rebuild_author_relationships(
        store.as_ref(),
        &local,
        vec![AuthorRelationshipProjectionRow {
            local_author_pubkey: local.clone(),
            author_pubkey: peer.clone(),
            following: true,
            followed_by: true,
            mutual: true,
            friend_of_friend: false,
            friend_of_friend_via_pubkeys: Vec::new(),
            derived_at: 1,
        }],
    )
    .await
    .unwrap();
    let row = DirectMessageOutboxRow {
        dm_id: direct_message_id_for_participants(&sender.public_key(), &recipient.public_key()),
        message_id: "revoked-while-resolving-destination".into(),
        peer_pubkey: peer.clone(),
        frame_blob_hash: BlobHash::new("bb".repeat(32)),
        created_at: 42,
        last_attempt_at: None,
    };
    store.put_direct_message_outbox(row.clone()).await.unwrap();
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let mut hint_double = TrackingHintTransport::default();
    hint_double.resolve_barrier = Some(barrier.clone());
    let hints = Arc::new(hint_double);
    *hints.resolved_destination.lock().await = Some(EndpointAddr::new(
        iroh::SecretKey::from_bytes(&[26; 32]).public(),
    ));
    let blob = Arc::new(OfferBlobService::new(
        Arc::new(MemoryBlobService::default()),
    ));
    let services = ServiceHandles::new(
        store.clone(),
        store.clone(),
        Arc::new(
            StaticTransport::new(PeerSnapshot::default()).with_local_endpoint_id(
                iroh::SecretKey::from_bytes(&[27; 32]).public().to_string(),
            ),
        ),
        hints.clone(),
        Arc::new(MemoryDocsSync::default()),
        blob.clone(),
        sender,
    );
    let local_for_revoke = local.clone();
    let flush =
        tokio::spawn(
            async move { AppService::flush_due_direct_message_outbox(&services, 1_000).await },
        );
    timeout(Duration::from_secs(5), barrier.wait())
        .await
        .expect("destination lookup must reach the pause");
    SocialProjectionStore::rebuild_author_relationships(store.as_ref(), &local_for_revoke, vec![])
        .await
        .unwrap();
    timeout(Duration::from_secs(5), barrier.wait())
        .await
        .expect("destination lookup must resume");
    flush.await.unwrap().unwrap();
    assert!(hints.offers.lock().await.is_empty());
    assert_eq!(blob.writes.load(Ordering::SeqCst), 0);
    assert!(
        store
            .get_direct_message_outbox(&row.dm_id, &row.message_id)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn account_dm_retry_owner_is_single_and_shutdown_cancels_active_lookup() {
    use kukuri_core::BlobHash;
    use kukuri_store::{DirectMessageOutboxRow, DirectMessageStore};

    let store = Arc::new(MemoryStore::default());
    let sender = generate_keys();
    let recipient = generate_keys();
    let local = sender.public_key_hex();
    let peer = recipient.public_key_hex();
    SocialProjectionStore::rebuild_author_relationships(
        store.as_ref(),
        &local,
        vec![AuthorRelationshipProjectionRow {
            local_author_pubkey: local.clone(),
            author_pubkey: peer.clone(),
            following: true,
            followed_by: true,
            mutual: true,
            friend_of_friend: false,
            friend_of_friend_via_pubkeys: Vec::new(),
            derived_at: 1,
        }],
    )
    .await
    .unwrap();
    let row = DirectMessageOutboxRow {
        dm_id: direct_message_id_for_participants(&sender.public_key(), &recipient.public_key()),
        message_id: "cancel-owner-lookup".into(),
        peer_pubkey: peer,
        frame_blob_hash: BlobHash::new("cc".repeat(32)),
        created_at: 42,
        last_attempt_at: None,
    };
    store.put_direct_message_outbox(row.clone()).await.unwrap();
    for index in 0..100 {
        store
            .put_direct_message_outbox(DirectMessageOutboxRow {
                dm_id: format!("unrelated-dm-{index}"),
                message_id: format!("unrelated-message-{index}"),
                peer_pubkey: format!("unrelated-peer-{index}"),
                frame_blob_hash: BlobHash::new("dd".repeat(32)),
                created_at: 43,
                last_attempt_at: None,
            })
            .await
            .unwrap();
    }
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let mut hint_double = TrackingHintTransport::default();
    hint_double.resolve_barrier = Some(barrier.clone());
    let hints = Arc::new(hint_double);
    let app = app_service_from_dependencies(
        store.clone(),
        store.clone(),
        Arc::new(StaticTransport::new(PeerSnapshot::default())),
        hints.clone(),
        Arc::new(MemoryDocsSync::default()),
        Arc::new(MemoryBlobService::default()),
        sender,
    );
    app.start_direct_message_outbox_retry().await.unwrap();
    app.start_direct_message_outbox_retry().await.unwrap();
    assert_eq!(
        app.subscription_registry
            .dm_outbox_retry_starts
            .load(Ordering::SeqCst),
        1
    );
    timeout(Duration::from_secs(5), barrier.wait())
        .await
        .expect("owner must enter destination lookup");
    timeout(Duration::from_secs(2), app.shutdown())
        .await
        .expect("shutdown must cancel the active lookup");
    assert!(app.start_direct_message_outbox_retry().await.is_err());
    assert!(
        app.subscription_registry
            .dm_outbox_retry_task
            .lock()
            .await
            .is_none()
    );
    assert!(hints.offers.lock().await.is_empty());
    assert!(
        store
            .get_direct_message_outbox(&row.dm_id, &row.message_id)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn blocked_pairwise_publish_cannot_stop_other_peer_or_account_offer() {
    use kukuri_core::BlobHash;
    use kukuri_store::{DirectMessageOutboxRow, DirectMessageStore};
    use kukuri_transport::EndpointAddr;

    let store = Arc::new(MemoryStore::default());
    let sender = generate_keys();
    let peers = [generate_keys(), generate_keys()];
    let local = sender.public_key_hex();
    SocialProjectionStore::rebuild_author_relationships(
        store.as_ref(),
        &local,
        peers
            .iter()
            .map(|peer| AuthorRelationshipProjectionRow {
                local_author_pubkey: local.clone(),
                author_pubkey: peer.public_key_hex(),
                following: true,
                followed_by: true,
                mutual: true,
                friend_of_friend: false,
                friend_of_friend_via_pubkeys: Vec::new(),
                derived_at: 1,
            })
            .collect(),
    )
    .await
    .unwrap();
    for (index, peer) in peers.iter().enumerate() {
        store
            .put_direct_message_outbox(DirectMessageOutboxRow {
                dm_id: direct_message_id_for_participants(&sender.public_key(), &peer.public_key()),
                message_id: format!("blocked-pairwise-{index}"),
                peer_pubkey: peer.public_key_hex(),
                frame_blob_hash: BlobHash::new("aa".repeat(32)),
                created_at: index as i64,
                last_attempt_at: None,
            })
            .await
            .unwrap();
    }
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let mut hint_double = TrackingHintTransport::default();
    hint_double.publish_hint_barrier = Some(barrier.clone());
    let hints = Arc::new(hint_double);
    *hints.resolved_destination.lock().await = Some(EndpointAddr::new(
        iroh::SecretKey::from_bytes(&[30; 32]).public(),
    ));
    let services = ServiceHandles::new(
        store.clone(),
        store.clone(),
        Arc::new(
            StaticTransport::new(PeerSnapshot::default()).with_local_endpoint_id(
                iroh::SecretKey::from_bytes(&[31; 32]).public().to_string(),
            ),
        ),
        hints.clone(),
        Arc::new(MemoryDocsSync::default()),
        Arc::new(MemoryBlobService::default()),
        sender,
    );
    let flush =
        tokio::spawn(
            async move { AppService::flush_due_direct_message_outbox(&services, 1_000).await },
        );
    timeout(Duration::from_secs(2), barrier.wait())
        .await
        .expect("first peer must enter the blocked pairwise publisher");
    assert_eq!(
        timeout(Duration::from_secs(7), flush)
            .await
            .expect("a blocked peer must not stall account retry owner")
            .unwrap()
            .unwrap(),
        2
    );
    assert_eq!(hints.offers.lock().await.len(), 2);
    assert_eq!(store.list_direct_message_outbox().await.unwrap().len(), 2);
}

#[tokio::test]
async fn dm_due_owner_processes_bounded_new_and_retry_lanes() {
    use kukuri_core::BlobHash;
    use kukuri_store::DirectMessageOutboxRow;

    let store = Arc::new(MemoryStore::default());
    let local_keys = generate_keys();
    let local = local_keys.public_key_hex();
    let peer = generate_keys().public_key_hex();
    let unrelated = generate_keys().public_key_hex();
    SocialProjectionStore::rebuild_author_relationships(
        store.as_ref(),
        &local,
        vec![AuthorRelationshipProjectionRow {
            local_author_pubkey: local.clone(),
            author_pubkey: peer.clone(),
            following: true,
            followed_by: true,
            mutual: true,
            friend_of_friend: false,
            friend_of_friend_via_pubkeys: Vec::new(),
            derived_at: 1,
        }],
    )
    .await
    .unwrap();
    for index in 0..1_000 {
        DirectMessageStore::put_direct_message_outbox(
            store.as_ref(),
            DirectMessageOutboxRow {
                dm_id: "dm-unrelated".into(),
                message_id: format!("unrelated-{index:04}"),
                peer_pubkey: unrelated.clone(),
                frame_blob_hash: BlobHash::new("unrelated-hash"),
                created_at: 42,
                last_attempt_at: None,
            },
        )
        .await
        .unwrap();
    }
    for index in 0..130 {
        DirectMessageStore::put_direct_message_outbox(
            store.as_ref(),
            DirectMessageOutboxRow {
                dm_id: "dm-target".into(),
                message_id: format!("target-{index:04}"),
                peer_pubkey: peer.clone(),
                frame_blob_hash: BlobHash::new("target-hash"),
                created_at: 42,
                last_attempt_at: Some(0),
            },
        )
        .await
        .unwrap();
    }
    let hint_transport = Arc::new(TrackingHintTransport::default());
    let topic = derive_direct_message_topic(&local_keys, &Pubkey::from(peer.as_str())).unwrap();
    let projection_store = store.clone();
    let services = ServiceHandles::new(
        store.clone(),
        store,
        Arc::new(StaticTransport::new(PeerSnapshot::default())),
        hint_transport.clone(),
        Arc::new(MemoryDocsSync::default()),
        Arc::new(MemoryBlobService::default()),
        local_keys,
    );
    for tick in 0..3 {
        let processed = AppService::flush_due_direct_message_outbox(&services, 3_000)
            .await
            .unwrap();
        assert_eq!(processed, 4);
        assert_eq!(
            hint_transport.published_count.load(Ordering::SeqCst),
            tick + 1,
            "only the mutual retry lane publishes while unrelated new rows rotate"
        );
        assert_eq!(
            hint_transport.resolved_count.load(Ordering::SeqCst),
            tick + 1,
            "only one mutual recipient is resolved in this bounded tick"
        );
        for index in 0..3 {
            DirectMessageStore::put_direct_message_outbox(
                projection_store.as_ref(),
                DirectMessageOutboxRow {
                    dm_id: "dm-unrelated".into(),
                    message_id: format!("later-{tick}-{index}"),
                    peer_pubkey: unrelated.clone(),
                    frame_blob_hash: BlobHash::new("unrelated-hash"),
                    created_at: 43,
                    last_attempt_at: None,
                },
            )
            .await
            .unwrap();
        }
    }
    let mut fresh_hints = hint_transport.subscribe_hints(&topic).await.unwrap();
    let app = AppService::from_handles(services);
    let fresh = app
        .send_direct_message_internal(peer.as_str(), Some("fresh"), None, Vec::new())
        .await
        .unwrap();
    assert_eq!(hint_transport.published_count.load(Ordering::SeqCst), 4);
    let received = timeout(Duration::from_secs(1), fresh_hints.next())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        received.hint,
        GossipHint::DirectMessageFrame { message_id, .. } if message_id == fresh
    ));
}

#[tokio::test]
async fn dm_first_message_appears_in_recipient_conversation_list_without_opening_dm() {
    let transport = Arc::new(StaticTransport::new(PeerSnapshot::default()));
    let hint_transport = Arc::new(TrackingHintTransport::default());
    let docs_sync = Arc::new(MemoryDocsSync::default());
    let blob_service = Arc::new(MemoryBlobService::default());
    let store_a = Arc::new(MemoryStore::default());
    let store_b = Arc::new(MemoryStore::default());
    let keys_a = generate_keys();
    let keys_b = generate_keys();
    let a_pubkey = keys_a.public_key_hex();
    let b_pubkey = keys_b.public_key_hex();
    let follow_a_to_b = parse_follow_edge(
        &build_follow_edge_envelope(
            &keys_a,
            &Pubkey::from(b_pubkey.as_str()),
            FollowEdgeStatus::Active,
        )
        .expect("build follow edge a->b"),
    )
    .expect("parse follow edge a->b")
    .expect("follow edge a->b");
    let follow_b_to_a = parse_follow_edge(
        &build_follow_edge_envelope(
            &keys_b,
            &Pubkey::from(a_pubkey.as_str()),
            FollowEdgeStatus::Active,
        )
        .expect("build follow edge b->a"),
    )
    .expect("parse follow edge b->a")
    .expect("follow edge b->a");

    store_a
        .upsert_follow_edge(follow_a_to_b.clone())
        .await
        .expect("seed follow edge a->b in store a");
    store_a
        .upsert_follow_edge(follow_b_to_a.clone())
        .await
        .expect("seed follow edge b->a in store a");
    store_b
        .upsert_follow_edge(follow_a_to_b)
        .await
        .expect("seed follow edge a->b in store b");
    store_b
        .upsert_follow_edge(follow_b_to_a)
        .await
        .expect("seed follow edge b->a in store b");

    let app_a = app_service_from_dependencies(
        store_a.clone(),
        store_a,
        transport.clone(),
        hint_transport.clone(),
        docs_sync.clone(),
        blob_service.clone(),
        keys_a.clone(),
    );
    let app_b = app_service_from_dependencies(
        store_b.clone(),
        store_b,
        transport.clone(),
        hint_transport.clone(),
        docs_sync,
        blob_service,
        keys_b.clone(),
    );

    app_a
        .rebuild_author_relationships()
        .await
        .expect("rebuild relationships for app a");
    app_b
        .rebuild_author_relationships()
        .await
        .expect("rebuild relationships for app b");
    assert!(
        app_b
            .subscription_registry
            .direct_message_subscriptions
            .lock()
            .await
            .contains_key(a_pubkey.as_str()),
        "recipient should subscribe to mutual dm topics before opening the dm",
    );

    let message_id = app_a
        .send_direct_message(b_pubkey.as_str(), Some("hello from a"), None, Vec::new())
        .await
        .expect("send direct message");

    let conversation = timeout(Duration::from_secs(10), async {
        loop {
            let conversations = app_b
                .list_direct_messages()
                .await
                .expect("list recipient direct messages");
            if let Some(conversation) = conversations.into_iter().find(|item| {
                item.peer_pubkey == a_pubkey
                    && item.last_message_id.as_deref() == Some(message_id.as_str())
            }) {
                break conversation;
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("wait for recipient conversation list update");
    assert_eq!(
        conversation.last_message_preview.as_deref(),
        Some("hello from a")
    );

    let delivered = app_b
        .list_direct_message_messages(a_pubkey.as_str(), None, 20)
        .await
        .expect("list recipient direct message timeline");
    assert!(
        delivered
            .items
            .iter()
            .any(|message| message.message_id == message_id),
        "recipient should see the delivered message after the conversation appears",
    );
}

#[tokio::test]
async fn dm_outbox_retry_stops_when_mutual_is_lost_and_resumes_when_it_returns() {
    let transport = Arc::new(StaticTransport::new(PeerSnapshot::default()));
    let hint_transport = Arc::new(TrackingHintTransport::default());
    let docs_sync = Arc::new(MemoryDocsSync::default());
    let blob_service = Arc::new(MemoryBlobService::default());
    let store = Arc::new(MemoryStore::default());
    let keys_local = generate_keys();
    let keys_peer = generate_keys();
    let local_pubkey = keys_local.public_key_hex();
    let peer_pubkey = keys_peer.public_key_hex();
    let follow_local_to_peer = parse_follow_edge(
        &build_follow_edge_envelope(
            &keys_local,
            &Pubkey::from(peer_pubkey.as_str()),
            FollowEdgeStatus::Active,
        )
        .expect("build follow edge local->peer"),
    )
    .expect("parse follow edge local->peer")
    .expect("follow edge local->peer");
    let follow_peer_to_local_active = parse_follow_edge(
        &build_follow_edge_envelope(
            &keys_peer,
            &Pubkey::from(local_pubkey.as_str()),
            FollowEdgeStatus::Active,
        )
        .expect("build follow edge peer->local active"),
    )
    .expect("parse follow edge peer->local active")
    .expect("follow edge peer->local active");
    let follow_peer_to_local_inactive = parse_follow_edge(
        &build_follow_edge_envelope(
            &keys_peer,
            &Pubkey::from(local_pubkey.as_str()),
            FollowEdgeStatus::Revoked,
        )
        .expect("build follow edge peer->local inactive"),
    )
    .expect("parse follow edge peer->local inactive")
    .expect("follow edge peer->local inactive");

    store
        .upsert_follow_edge(follow_local_to_peer)
        .await
        .expect("seed follow edge local->peer");
    store
        .upsert_follow_edge(follow_peer_to_local_active.clone())
        .await
        .expect("seed follow edge peer->local");

    let app = app_service_from_dependencies(
        store.clone(),
        store.clone(),
        transport.clone(),
        hint_transport.clone(),
        docs_sync,
        blob_service,
        keys_local.clone(),
    );

    app.rebuild_author_relationships()
        .await
        .expect("seed relationship projection");
    app.start_direct_message_outbox_retry().await.unwrap();
    assert!(
        app.subscription_registry
            .direct_message_subscriptions
            .lock()
            .await
            .contains_key(peer_pubkey.as_str()),
        "mutual peer should start with an active dm subscription",
    );

    let topic = derive_direct_message_topic(&keys_local, &Pubkey::from(peer_pubkey.as_str()))
        .expect("derive dm topic");
    let message_id = app
        .send_direct_message(
            peer_pubkey.as_str(),
            Some("queued while disconnected"),
            None,
            Vec::new(),
        )
        .await
        .expect("queue direct message while disconnected");
    let queued_outbox = store
        .list_direct_message_outbox()
        .await
        .expect("list queued outbox");
    assert_eq!(queued_outbox.len(), 1);
    assert_eq!(queued_outbox[0].message_id, message_id);
    assert_eq!(queued_outbox[0].last_attempt_at, None);
    let published_before_loss = hint_transport.published_count.load(Ordering::SeqCst);

    store
        .upsert_follow_edge(follow_peer_to_local_inactive)
        .await
        .expect("drop peer->local follow edge");
    app.rebuild_author_relationships()
        .await
        .expect("rebuild relationships after mutual loss");
    assert!(
        !app.subscription_registry
            .direct_message_subscriptions
            .lock()
            .await
            .contains_key(peer_pubkey.as_str()),
        "subscription should stop when mutual relationship is lost",
    );
    let disabled_status = app
        .get_direct_message_status(peer_pubkey.as_str())
        .await
        .expect("status after mutual loss");
    assert!(!disabled_status.send_enabled);
    assert_eq!(disabled_status.pending_outbox_count, 1);

    {
        let mut snapshot = transport.peers.lock().await;
        snapshot.connected = true;
        snapshot.peer_count = 1;
        snapshot.connected_peers = vec!["peer-b".into()];
        snapshot.topic_diagnostics = vec![TopicPeerSnapshot {
            topic: format!("hint/{}", topic.as_str()),
            joined: true,
            peer_count: 1,
            connected_peers: vec!["peer-b".into()],
            configured_peer_ids: vec!["peer-b".into()],
            missing_peer_ids: Vec::new(),
            active_path: Default::default(),
            rendezvous_peer_ids: Vec::new(),
            fallback_peer_ids: Vec::new(),
            last_received_at: None,
            status_detail: "connected".into(),
            last_error: None,
        }];
    }
    sleep(Duration::from_millis(
        DIRECT_MESSAGE_RETRY_INTERVAL_MS + 250,
    ))
    .await;
    let stopped_outbox = store
        .list_direct_message_outbox()
        .await
        .expect("list outbox while retry is stopped");
    assert_eq!(stopped_outbox.len(), 1);
    assert_eq!(
        hint_transport.published_count.load(Ordering::SeqCst),
        published_before_loss,
        "mutual loss must suppress network retry even while the owner rotates the row"
    );

    let follow_peer_to_local_restored = parse_follow_edge(
        &build_follow_edge_envelope(
            &keys_peer,
            &Pubkey::from(local_pubkey.as_str()),
            FollowEdgeStatus::Active,
        )
        .expect("build follow edge peer->local restored"),
    )
    .expect("parse follow edge peer->local restored")
    .expect("follow edge peer->local restored");
    store
        .upsert_follow_edge(follow_peer_to_local_restored)
        .await
        .expect("restore peer->local follow edge");
    app.rebuild_author_relationships()
        .await
        .expect("rebuild relationships after mutual restore");
    assert!(
        app.subscription_registry
            .direct_message_subscriptions
            .lock()
            .await
            .contains_key(peer_pubkey.as_str()),
        "subscription should resume when mutual relationship returns",
    );
    let restored_status = app
        .get_direct_message_status(peer_pubkey.as_str())
        .await
        .expect("status after mutual restore");
    assert!(restored_status.send_enabled);

    timeout(Duration::from_secs(10), async {
        loop {
            let outbox = store
                .list_direct_message_outbox()
                .await
                .expect("list outbox after mutual restore");
            if outbox.iter().any(|row| row.message_id == message_id)
                && hint_transport.published_count.load(Ordering::SeqCst) > published_before_loss
            {
                break;
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("wait for queued retry to resume after mutual restore");
}
