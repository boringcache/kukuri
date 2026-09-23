use super::super::*;
use super::receive_offer_doubles::{OfferBlobService, ProbeOfferTransport};
use kukuri_core::{ReceiveOfferReferenceV1, ReceiveOfferScopeV1, seal_receive_offer};
use kukuri_transport::{EndpointAddr, ReceiveOfferEnvelope};

fn offer_app(
    keys: KukuriKeys,
    store: Arc<MemoryStore>,
    transport: Arc<FakeTransport>,
    blob: Arc<OfferBlobService>,
) -> AppService {
    AppService::from_handles(ServiceHandles::new(
        store.clone(),
        store,
        transport.clone(),
        transport,
        Arc::new(MemoryDocsSync::default()),
        blob,
        keys,
    ))
}

fn offer_for(
    sender: &KukuriKeys,
    recipient: &KukuriKeys,
    scope: ReceiveOfferScopeV1,
    payload_hash: kukuri_core::BlobHash,
    payload_bytes: u32,
) -> (EndpointAddr, kukuri_core::SealedReceiveOfferV1) {
    let provider = EndpointAddr::new(iroh::SecretKey::from_bytes(&[43; 32]).public());
    let now = Utc::now().timestamp_millis();
    let offer = seal_receive_offer(
        sender,
        &recipient.public_key(),
        ReceiveOfferReferenceV1 {
            provider_endpoint_id: provider.id.to_string(),
            payload_hash,
            payload_bytes,
            scope,
        },
        now,
        now + 60_000,
    )
    .unwrap();
    (provider, offer)
}

#[tokio::test]
async fn account_receive_offer_rejects_unmutual_and_other_scopes_before_provider_io() {
    let sender = generate_keys();
    let recipient = generate_keys();
    let store = Arc::new(MemoryStore::default());
    let blob = Arc::new(OfferBlobService::new(
        Arc::new(MemoryBlobService::default()),
    ));
    let transport = Arc::new(FakeTransport::new("recipient", FakeNetwork::default()));
    let app = offer_app(recipient.clone(), store.clone(), transport, blob.clone());
    let hash = kukuri_core::BlobHash::new("11".repeat(32));

    let (_, unmutual) = offer_for(
        &sender,
        &recipient,
        ReceiveOfferScopeV1::DirectMessage,
        hash.clone(),
        1,
    );
    assert!(
        !AppService::ingest_account_receive_offer(
            &app.services,
            ReceiveOfferEnvelope {
                offer: unmutual,
                received_at: Utc::now().timestamp_millis(),
                source_peer: "untrusted relay".into(),
            },
        )
        .await
        .unwrap()
    );

    SocialProjectionStore::rebuild_author_relationships(
        store.as_ref(),
        &recipient.public_key_hex(),
        vec![AuthorRelationshipProjectionRow {
            local_author_pubkey: recipient.public_key_hex(),
            author_pubkey: sender.public_key_hex(),
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
    let (_, public) = offer_for(
        &sender,
        &recipient,
        ReceiveOfferScopeV1::PublicSource,
        hash,
        1,
    );
    assert!(
        !AppService::ingest_account_receive_offer(
            &app.services,
            ReceiveOfferEnvelope {
                offer: public,
                received_at: Utc::now().timestamp_millis(),
                source_peer: "untrusted relay".into(),
            },
        )
        .await
        .unwrap()
    );
    assert_eq!(blob.fetches.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn account_route_ingests_verified_mutual_dm_and_stops_on_shutdown() {
    let sender = generate_keys();
    let recipient = generate_keys();
    let store = Arc::new(MemoryStore::default());
    let memory_blob = Arc::new(MemoryBlobService::default());
    let blob = Arc::new(OfferBlobService::new(memory_blob.clone()));
    let network = FakeNetwork::default();
    let receiver_transport = Arc::new(FakeTransport::new("recipient", network.clone()));
    let sender_transport = FakeTransport::new("sender", network);
    let app = offer_app(
        recipient.clone(),
        store.clone(),
        receiver_transport,
        blob.clone(),
    );
    SocialProjectionStore::rebuild_author_relationships(
        store.as_ref(),
        &recipient.public_key_hex(),
        vec![AuthorRelationshipProjectionRow {
            local_author_pubkey: recipient.public_key_hex(),
            author_pubkey: sender.public_key_hex(),
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
    let dm_id = direct_message_id_for_participants(&sender.public_key(), &recipient.public_key());
    let message_id = "account-route-dm-1";
    let frame = encrypt_direct_message_frame(
        &sender,
        &recipient.public_key(),
        &dm_id,
        message_id,
        Utc::now().timestamp_millis(),
        &DirectMessagePayloadV1 {
            text: Some("over account route".into()),
            reply_to: None,
            attachment_manifest: None,
        },
    )
    .unwrap();
    let frame_blob = memory_blob
        .put_blob(
            serde_json::to_vec(&frame).unwrap(),
            DIRECT_MESSAGE_FRAME_MIME,
        )
        .await
        .unwrap();
    let topic = derive_direct_message_topic(&recipient, &sender.public_key()).unwrap();
    let manifest = serde_json::to_vec(&GossipHint::DirectMessageFrame {
        topic_id: topic,
        dm_id: dm_id.clone(),
        message_id: message_id.into(),
        frame_hash: frame_blob.hash,
    })
    .unwrap();
    let manifest_blob = memory_blob
        .put_blob(
            manifest,
            "application/vnd.kukuri.direct-message-receive-manifest+json",
        )
        .await
        .unwrap();
    let (provider, offer) = offer_for(
        &sender,
        &recipient,
        ReceiveOfferScopeV1::DirectMessage,
        manifest_blob.hash,
        manifest_blob.bytes as u32,
    );

    app.start_account_receive_offers().await.unwrap();
    app.start_account_receive_offers().await.unwrap();
    let inserted_notify = app.notification_inserted_notify();
    let mut inserted = Box::pin(inserted_notify.notified());
    inserted.as_mut().enable();
    sender_transport
        .publish_receive_offer(&recipient.public_key(), provider.clone(), offer.clone())
        .await
        .unwrap();
    timeout(Duration::from_secs(2), async {
        loop {
            if DirectMessageStore::get_direct_message_message(store.as_ref(), &dm_id, message_id)
                .await
                .unwrap()
                .is_some()
            {
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    timeout(Duration::from_secs(2), inserted)
        .await
        .expect("DM notification event forwarded");
    assert_eq!(blob.fetches.load(Ordering::SeqCst), 1);

    app.shutdown().await;
    assert!(app.start_account_receive_offers().await.is_err());
    sender_transport
        .publish_receive_offer(&recipient.public_key(), provider, offer)
        .await
        .unwrap();
    sleep(Duration::from_millis(30)).await;
    assert_eq!(blob.fetches.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn account_offer_rechecks_mutual_after_provider_io_before_reflection() {
    let sender = generate_keys();
    let recipient = generate_keys();
    let store = Arc::new(MemoryStore::default());
    let memory_blob = Arc::new(MemoryBlobService::default());
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let mut offer_blob = OfferBlobService::new(memory_blob.clone());
    offer_blob.barrier = Some(barrier.clone());
    let blob = Arc::new(offer_blob);
    let transport = Arc::new(FakeTransport::new("recipient", FakeNetwork::default()));
    let app = offer_app(recipient.clone(), store.clone(), transport, blob.clone());
    SocialProjectionStore::rebuild_author_relationships(
        store.as_ref(),
        &recipient.public_key_hex(),
        vec![AuthorRelationshipProjectionRow {
            local_author_pubkey: recipient.public_key_hex(),
            author_pubkey: sender.public_key_hex(),
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
    let dm_id = direct_message_id_for_participants(&sender.public_key(), &recipient.public_key());
    let message_id = "revoked-before-reflection";
    let frame = encrypt_direct_message_frame(
        &sender,
        &recipient.public_key(),
        &dm_id,
        message_id,
        Utc::now().timestamp_millis(),
        &DirectMessagePayloadV1 {
            text: Some("must not appear".into()),
            reply_to: None,
            attachment_manifest: None,
        },
    )
    .unwrap();
    let frame_blob = memory_blob
        .put_blob(
            serde_json::to_vec(&frame).unwrap(),
            DIRECT_MESSAGE_FRAME_MIME,
        )
        .await
        .unwrap();
    let manifest = serde_json::to_vec(&GossipHint::DirectMessageFrame {
        topic_id: derive_direct_message_topic(&recipient, &sender.public_key()).unwrap(),
        dm_id: dm_id.clone(),
        message_id: message_id.into(),
        frame_hash: frame_blob.hash,
    })
    .unwrap();
    let manifest_blob = memory_blob
        .put_blob(
            manifest,
            "application/vnd.kukuri.direct-message-receive-manifest+json",
        )
        .await
        .unwrap();
    let (_, offer) = offer_for(
        &sender,
        &recipient,
        ReceiveOfferScopeV1::DirectMessage,
        manifest_blob.hash,
        manifest_blob.bytes as u32,
    );
    let services = app.services.clone();
    let ingest = tokio::spawn(async move {
        AppService::ingest_account_receive_offer(
            &services,
            ReceiveOfferEnvelope {
                offer,
                received_at: Utc::now().timestamp_millis(),
                source_peer: "untrusted relay".into(),
            },
        )
        .await
    });
    timeout(Duration::from_secs(2), barrier.wait())
        .await
        .expect("provider fetch started");
    SocialProjectionStore::rebuild_author_relationships(
        store.as_ref(),
        &recipient.public_key_hex(),
        Vec::new(),
    )
    .await
    .unwrap();
    barrier.wait().await;
    assert!(!ingest.await.unwrap().unwrap());
    assert_eq!(blob.fetches.load(Ordering::SeqCst), 1);
    assert!(
        DirectMessageStore::get_direct_message_message(store.as_ref(), &dm_id, message_id)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn revoked_mutual_during_attachment_fetch_never_persists_plaintext() {
    let sender = generate_keys();
    let recipient = generate_keys();
    let store = Arc::new(MemoryStore::default());
    let memory_blob = Arc::new(MemoryBlobService::default());
    let message_id = "revoke-during-attachment";
    let encrypted = encrypt_direct_message_attachment(
        &sender,
        &recipient.public_key(),
        message_id,
        "attachment-1",
        b"private attachment",
    )
    .unwrap();
    let encrypted_blob = memory_blob
        .put_blob(
            serde_json::to_vec(&encrypted).unwrap(),
            "application/vnd.kukuri.direct-message-attachment+json",
        )
        .await
        .unwrap();
    let dm_id = direct_message_id_for_participants(&sender.public_key(), &recipient.public_key());
    let frame = encrypt_direct_message_frame(
        &sender,
        &recipient.public_key(),
        &dm_id,
        message_id,
        Utc::now().timestamp_millis(),
        &DirectMessagePayloadV1 {
            text: None,
            reply_to: None,
            attachment_manifest: Some(DirectMessageAttachmentManifestV1 {
                attachment_id: "attachment-1".into(),
                kind: DirectMessageAttachmentKind::Image,
                original: DirectMessageEncryptedBlobRefV1 {
                    blob_id: "attachment-1".into(),
                    hash: encrypted_blob.hash.clone(),
                    mime: "image/png".into(),
                    bytes: 18,
                    nonce_hex: encrypted.nonce_hex,
                },
                poster: None,
            }),
        },
    )
    .unwrap();
    let frame_blob = memory_blob
        .put_blob(
            serde_json::to_vec(&frame).unwrap(),
            DIRECT_MESSAGE_FRAME_MIME,
        )
        .await
        .unwrap();
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let mut wrapped = OfferBlobService::new(memory_blob);
    wrapped.pause_blob_hash = Some(encrypted_blob.hash);
    wrapped.attachment_barrier = Some(barrier.clone());
    let blob = Arc::new(wrapped);
    let app = offer_app(
        recipient.clone(),
        store.clone(),
        Arc::new(FakeTransport::new("recipient", FakeNetwork::default())),
        blob.clone(),
    );
    SocialProjectionStore::rebuild_author_relationships(
        store.as_ref(),
        &recipient.public_key_hex(),
        vec![AuthorRelationshipProjectionRow {
            local_author_pubkey: recipient.public_key_hex(),
            author_pubkey: sender.public_key_hex(),
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
    let topic = derive_direct_message_topic(&recipient, &sender.public_key()).unwrap();
    let recipient_pubkey = recipient.public_key_hex();
    let sender_pubkey = sender.public_key_hex();
    let dm_id_for_task = dm_id.clone();
    let frame_hash = frame_blob.hash;
    let services = app.services.clone();
    let ingest = tokio::spawn(async move {
        AppService::ingest_direct_message_frame(
            &services,
            &recipient_pubkey,
            &sender_pubkey,
            &topic,
            &dm_id_for_task,
            message_id,
            &frame_hash,
        )
        .await
    });
    timeout(Duration::from_secs(2), barrier.wait())
        .await
        .expect("attachment fetch started");
    SocialProjectionStore::rebuild_author_relationships(
        store.as_ref(),
        &recipient.public_key_hex(),
        Vec::new(),
    )
    .await
    .unwrap();
    barrier.wait().await;
    assert!(!ingest.await.unwrap().unwrap());
    assert_eq!(blob.writes.load(Ordering::SeqCst), 0);
    assert!(
        DirectMessageStore::get_direct_message_message(store.as_ref(), &dm_id, message_id)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn dropping_account_owner_aborts_offer_stream() {
    let store = Arc::new(MemoryStore::default());
    let transport = Arc::new(ProbeOfferTransport::default());
    let app = AppService::from_handles(ServiceHandles::new(
        store.clone(),
        store,
        Arc::new(StaticTransport::new(PeerSnapshot::default())),
        transport.clone(),
        Arc::new(MemoryDocsSync::default()),
        Arc::new(MemoryBlobService::default()),
        generate_keys(),
    ));
    app.start_account_receive_offers().await.unwrap();
    drop(app);
    timeout(Duration::from_secs(1), async {
        while transport.stream_drops.load(Ordering::SeqCst) != 1 {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("drop should cancel account offer processing");
}

#[tokio::test]
async fn shutdown_cancels_an_account_offer_subscription_still_registering() {
    let store = Arc::new(MemoryStore::default());
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let transport = Arc::new(ProbeOfferTransport {
        unsubscribes: AtomicUsize::new(0),
        subscribe_barrier: Some(barrier.clone()),
        unsubscribe_barrier: None,
        stream_drops: Arc::new(AtomicUsize::new(0)),
        stop_senders: std::sync::Mutex::new(Vec::new()),
    });
    let app = Arc::new(AppService::from_handles(ServiceHandles::new(
        store.clone(),
        store,
        Arc::new(StaticTransport::new(PeerSnapshot::default())),
        transport,
        Arc::new(MemoryDocsSync::default()),
        Arc::new(MemoryBlobService::default()),
        generate_keys(),
    )));
    let starter = {
        let app = app.clone();
        tokio::spawn(async move { app.start_account_receive_offers().await })
    };
    timeout(Duration::from_secs(1), barrier.wait())
        .await
        .expect("subscribe reached registration wait");
    timeout(Duration::from_secs(1), app.shutdown())
        .await
        .expect("shutdown must cancel pending subscribe");
    assert!(starter.await.unwrap().is_err());
    assert!(
        app.subscription_registry
            .account_receive_offer_task
            .lock()
            .await
            .is_none()
    );
}

#[tokio::test]
async fn cancelled_shutdown_retries_the_account_route_lease_cleanup() {
    let store = Arc::new(MemoryStore::default());
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let transport = Arc::new(ProbeOfferTransport {
        unsubscribes: AtomicUsize::new(0),
        subscribe_barrier: None,
        unsubscribe_barrier: Some(barrier.clone()),
        stream_drops: Arc::new(AtomicUsize::new(0)),
        stop_senders: std::sync::Mutex::new(Vec::new()),
    });
    let app = Arc::new(AppService::from_handles(ServiceHandles::new(
        store.clone(),
        store,
        Arc::new(StaticTransport::new(PeerSnapshot::default())),
        transport.clone(),
        Arc::new(MemoryDocsSync::default()),
        Arc::new(MemoryBlobService::default()),
        generate_keys(),
    )));
    app.start_account_receive_offers().await.unwrap();
    let first_shutdown = {
        let app = app.clone();
        tokio::spawn(async move { app.shutdown().await })
    };
    timeout(Duration::from_secs(1), barrier.wait())
        .await
        .expect("first unsubscribe started");
    first_shutdown.abort();
    let _ = first_shutdown.await;
    assert!(
        app.subscription_registry
            .account_receive_offer_lease
            .lock()
            .unwrap()
            .is_some(),
        "cancelled cleanup must retain its lease"
    );
    timeout(Duration::from_secs(1), app.shutdown())
        .await
        .expect("next shutdown should retry unsubscribe");
    assert_eq!(transport.unsubscribes.load(Ordering::SeqCst), 2);
    assert!(
        app.subscription_registry
            .account_receive_offer_lease
            .lock()
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn shutdown_cancels_an_in_flight_account_offer_provider_fetch() {
    let sender = generate_keys();
    let recipient = generate_keys();
    let store = Arc::new(MemoryStore::default());
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let mut offer_blob = OfferBlobService::new(Arc::new(MemoryBlobService::default()));
    offer_blob.barrier = Some(barrier.clone());
    let blob = Arc::new(offer_blob);
    let network = FakeNetwork::default();
    let receiver = Arc::new(FakeTransport::new("recipient", network.clone()));
    let publisher = FakeTransport::new("sender", network);
    let app = offer_app(recipient.clone(), store.clone(), receiver, blob.clone());
    SocialProjectionStore::rebuild_author_relationships(
        store.as_ref(),
        &recipient.public_key_hex(),
        vec![AuthorRelationshipProjectionRow {
            local_author_pubkey: recipient.public_key_hex(),
            author_pubkey: sender.public_key_hex(),
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
    let (provider, offer) = offer_for(
        &sender,
        &recipient,
        ReceiveOfferScopeV1::DirectMessage,
        kukuri_core::BlobHash::new("11".repeat(32)),
        1,
    );
    app.start_account_receive_offers().await.unwrap();
    publisher
        .publish_receive_offer(&recipient.public_key(), provider, offer)
        .await
        .unwrap();
    timeout(Duration::from_secs(1), barrier.wait())
        .await
        .expect("provider fetch started");
    timeout(Duration::from_secs(1), app.shutdown())
        .await
        .expect("shutdown must cancel in-flight provider fetch");
    assert_eq!(blob.fetches.load(Ordering::SeqCst), 1);
    assert!(
        app.subscription_registry
            .account_receive_offer_task
            .lock()
            .await
            .is_none()
    );
}

#[tokio::test]
async fn old_account_owner_shutdown_cannot_stop_new_same_account_receiver() {
    let sender = generate_keys();
    let recipient = generate_keys();
    let network = FakeNetwork::default();
    let receiver = Arc::new(FakeTransport::new("recipient", network.clone()));
    let publisher = FakeTransport::new("sender", network);
    let old_store = Arc::new(MemoryStore::default());
    let new_store = Arc::new(MemoryStore::default());
    for store in [&old_store, &new_store] {
        SocialProjectionStore::rebuild_author_relationships(
            store.as_ref(),
            &recipient.public_key_hex(),
            vec![AuthorRelationshipProjectionRow {
                local_author_pubkey: recipient.public_key_hex(),
                author_pubkey: sender.public_key_hex(),
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
    }
    let old_app = offer_app(
        recipient.clone(),
        old_store,
        receiver.clone(),
        Arc::new(OfferBlobService::new(
            Arc::new(MemoryBlobService::default()),
        )),
    );
    let new_blob = Arc::new(OfferBlobService::new(
        Arc::new(MemoryBlobService::default()),
    ));
    let new_app = offer_app(recipient.clone(), new_store, receiver, new_blob.clone());
    old_app.start_account_receive_offers().await.unwrap();
    new_app.start_account_receive_offers().await.unwrap();
    // Keep the old owner alive past its retry delay. It must not reclaim the
    // lease after the new owner supersedes its stream.
    sleep(Duration::from_millis(3_200)).await;
    old_app.shutdown().await;
    let (provider, offer) = offer_for(
        &sender,
        &recipient,
        ReceiveOfferScopeV1::DirectMessage,
        kukuri_core::BlobHash::new("11".repeat(32)),
        1,
    );
    publisher
        .publish_receive_offer(&recipient.public_key(), provider, offer)
        .await
        .unwrap();
    timeout(Duration::from_secs(1), async {
        while new_blob.fetches.load(Ordering::SeqCst) == 0 {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("new account route must survive old owner shutdown");
    new_app.shutdown().await;
}

#[tokio::test]
async fn superseding_account_owner_cancels_old_in_flight_provider_fetch() {
    let sender = generate_keys();
    let recipient = generate_keys();
    let store = Arc::new(MemoryStore::default());
    SocialProjectionStore::rebuild_author_relationships(
        store.as_ref(),
        &recipient.public_key_hex(),
        vec![AuthorRelationshipProjectionRow {
            local_author_pubkey: recipient.public_key_hex(),
            author_pubkey: sender.public_key_hex(),
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
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let mut old_blob = OfferBlobService::new(Arc::new(MemoryBlobService::default()));
    old_blob.barrier = Some(barrier.clone());
    let old_blob = Arc::new(old_blob);
    let network = FakeNetwork::default();
    let receiver = Arc::new(FakeTransport::new("recipient", network.clone()));
    let publisher = FakeTransport::new("sender", network);
    let old_app = offer_app(recipient.clone(), store, receiver.clone(), old_blob.clone());
    old_app.start_account_receive_offers().await.unwrap();
    let (provider, offer) = offer_for(
        &sender,
        &recipient,
        ReceiveOfferScopeV1::DirectMessage,
        kukuri_core::BlobHash::new("11".repeat(32)),
        1,
    );
    publisher
        .publish_receive_offer(&recipient.public_key(), provider, offer)
        .await
        .unwrap();
    timeout(Duration::from_secs(1), barrier.wait())
        .await
        .expect("old provider fetch started");
    assert_eq!(old_blob.in_flight.load(Ordering::SeqCst), 1);
    let new_app = offer_app(
        recipient,
        Arc::new(MemoryStore::default()),
        receiver,
        Arc::new(OfferBlobService::new(
            Arc::new(MemoryBlobService::default()),
        )),
    );
    new_app.start_account_receive_offers().await.unwrap();
    timeout(Duration::from_secs(1), async {
        while old_blob.in_flight.load(Ordering::SeqCst) != 0 {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("superseded owner's pending fetch must be canceled");
    assert_eq!(old_blob.writes.load(Ordering::SeqCst), 0);
    old_app.shutdown().await;
    new_app.shutdown().await;
}

#[cfg(feature = "iroh-integration-tests")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_account_route_fetches_bound_provider_manifest_and_reflects_dm() {
    let _guard = iroh_integration_test_lock().lock_owned().await;
    let dir = tempdir().unwrap();
    let sender_stack = TestIrohStack::new(&dir.path().join("offer-sender")).await;
    let recipient_stack = TestIrohStack::new(&dir.path().join("offer-recipient")).await;
    let sender = generate_keys();
    let recipient = generate_keys();
    sender_stack
        ._node
        .install_receive_binding(Arc::new(sender.clone()))
        .await
        .unwrap();
    let sender_store = Arc::new(MemoryStore::default());
    let recipient_store = Arc::new(MemoryStore::default());
    let sender_app = app_service_from_dependencies(
        sender_store.clone(),
        sender_store,
        sender_stack.transport.clone(),
        sender_stack.transport.clone(),
        sender_stack.docs_sync.clone(),
        sender_stack.blob_service.clone(),
        sender.clone(),
    );
    let recipient_app = app_service_from_dependencies(
        recipient_store.clone(),
        recipient_store.clone(),
        recipient_stack.transport.clone(),
        recipient_stack.transport.clone(),
        recipient_stack.docs_sync.clone(),
        recipient_stack.blob_service.clone(),
        recipient.clone(),
    );
    let recipient_ticket = recipient_app.peer_ticket().await.unwrap().unwrap();
    sender_app
        .import_peer_ticket(&recipient_ticket)
        .await
        .unwrap();
    SocialProjectionStore::rebuild_author_relationships(
        recipient_store.as_ref(),
        &recipient.public_key_hex(),
        vec![AuthorRelationshipProjectionRow {
            local_author_pubkey: recipient.public_key_hex(),
            author_pubkey: sender.public_key_hex(),
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
    let dm_id = direct_message_id_for_participants(&sender.public_key(), &recipient.public_key());
    let message_id = "real-account-offer-1";
    let frame = encrypt_direct_message_frame(
        &sender,
        &recipient.public_key(),
        &dm_id,
        message_id,
        Utc::now().timestamp_millis(),
        &DirectMessagePayloadV1 {
            text: Some("bound provider route".into()),
            reply_to: None,
            attachment_manifest: None,
        },
    )
    .unwrap();
    let frame_blob = sender_stack
        .blob_service
        .put_blob(
            serde_json::to_vec(&frame).unwrap(),
            DIRECT_MESSAGE_FRAME_MIME,
        )
        .await
        .unwrap();
    let manifest = serde_json::to_vec(&GossipHint::DirectMessageFrame {
        topic_id: derive_direct_message_topic(&recipient, &sender.public_key()).unwrap(),
        dm_id: dm_id.clone(),
        message_id: message_id.into(),
        frame_hash: frame_blob.hash,
    })
    .unwrap();
    let manifest_blob = sender_stack
        .blob_service
        .put_blob(
            manifest,
            "application/vnd.kukuri.direct-message-receive-manifest+json",
        )
        .await
        .unwrap();
    let now = Utc::now().timestamp_millis();
    let offer = seal_receive_offer(
        &sender,
        &recipient.public_key(),
        ReceiveOfferReferenceV1 {
            provider_endpoint_id: sender_stack._node.endpoint().id().to_string(),
            payload_hash: manifest_blob.hash,
            payload_bytes: manifest_blob.bytes as u32,
            scope: ReceiveOfferScopeV1::DirectMessage,
        },
        now,
        now + 60_000,
    )
    .unwrap();
    recipient_app.start_account_receive_offers().await.unwrap();
    sender_stack
        .transport
        .publish_receive_offer(
            &recipient.public_key(),
            recipient_stack._node.endpoint().addr(),
            offer,
        )
        .await
        .unwrap();
    timeout(Duration::from_secs(20), async {
        loop {
            if DirectMessageStore::get_direct_message_message(
                recipient_store.as_ref(),
                &dm_id,
                message_id,
            )
            .await
            .unwrap()
            .is_some()
            {
                break;
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("verified account offer DM reached recipient");
    recipient_app.shutdown().await;
    sender_app.shutdown().await;
}
