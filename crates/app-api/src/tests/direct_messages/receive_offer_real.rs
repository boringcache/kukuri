use super::super::*;
use kukuri_core::{ReceiveOfferReferenceV1, ReceiveOfferScopeV1, seal_receive_offer};

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
    recipient_stack
        ._node
        .install_receive_binding(Arc::new(recipient.clone()))
        .await
        .unwrap();
    let sender_store = Arc::new(MemoryStore::default());
    let recipient_store = Arc::new(MemoryStore::default());
    let sender_app = app_service_from_dependencies(
        sender_store.clone(),
        sender_store.clone(),
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
    SocialProjectionStore::rebuild_author_relationships(
        sender_store.as_ref(),
        &sender.public_key_hex(),
        vec![AuthorRelationshipProjectionRow {
            local_author_pubkey: sender.public_key_hex(),
            author_pubkey: recipient.public_key_hex(),
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
    sender_app.start_account_receive_offers().await.unwrap();
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

    let second_id = "real-account-outbox-2";
    let second_frame = encrypt_direct_message_frame(
        &sender,
        &recipient.public_key(),
        &dm_id,
        second_id,
        Utc::now().timestamp_millis(),
        &DirectMessagePayloadV1 {
            text: Some("sent by durable outbox".into()),
            reply_to: None,
            attachment_manifest: None,
        },
    )
    .unwrap();
    let second_blob = sender_stack
        .blob_service
        .put_blob(
            serde_json::to_vec(&second_frame).unwrap(),
            DIRECT_MESSAGE_FRAME_MIME,
        )
        .await
        .unwrap();
    sender_store
        .put_direct_message_outbox(kukuri_store::DirectMessageOutboxRow {
            dm_id: dm_id.clone(),
            message_id: second_id.into(),
            peer_pubkey: recipient.public_key_hex(),
            frame_blob_hash: second_blob.hash,
            created_at: Utc::now().timestamp_millis(),
            last_attempt_at: None,
        })
        .await
        .unwrap();
    AppService::flush_direct_message_outbox_page_for_peer(
        &sender_app.services,
        &sender.public_key_hex(),
        &recipient.public_key_hex(),
        None,
        None,
    )
    .await
    .unwrap();
    timeout(Duration::from_secs(20), async {
        loop {
            let received = recipient_store
                .get_direct_message_message(&dm_id, second_id)
                .await
                .unwrap()
                .is_some();
            let acked = sender_store
                .get_direct_message_outbox(&dm_id, second_id)
                .await
                .unwrap()
                .is_none();
            if received && acked {
                break;
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("bounded outbox offer and account-route ACK completed without pairwise subscription");
    recipient_app.shutdown().await;
    sender_app.shutdown().await;
}
