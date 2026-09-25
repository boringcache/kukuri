use super::super::*;
use super::receive_offer::{offer_app, offer_for};
use super::receive_offer_doubles::OfferBlobService;
use crate::service::public_notification_offer_support::encode_public_notification_manifest;
use kukuri_core::ReceiveOfferScopeV1;
use kukuri_transport::ReceiveOfferEnvelope;

async fn deliver_public_source(
    app: &AppService,
    sender: &KukuriKeys,
    recipient: &KukuriKeys,
    memory_blob: &MemoryBlobService,
    source: PublicNotificationSource,
) -> Result<bool> {
    let payload = encode_public_notification_manifest(source).unwrap();
    let stored = memory_blob
        .put_blob(payload, "application/vnd.kukuri.public-notification+json")
        .await
        .unwrap();
    let (_, offer) = offer_for(
        sender,
        recipient,
        ReceiveOfferScopeV1::PublicSource,
        stored.hash,
        stored.bytes as u32,
    );
    AppService::ingest_account_receive_offer(
        &app.services,
        ReceiveOfferEnvelope {
            offer,
            received_at: Utc::now().timestamp_millis(),
            source_peer: "offscreen".into(),
        },
    )
    .await
}

#[tokio::test]
async fn public_account_offer_creates_one_offscreen_mention_notification() {
    let sender = generate_keys();
    let recipient = generate_keys();
    let store = Arc::new(MemoryStore::default());
    let memory_blob = Arc::new(MemoryBlobService::default());
    let blob = Arc::new(OfferBlobService::new(memory_blob.clone()));
    let transport = Arc::new(FakeTransport::new("recipient", FakeNetwork::default()));
    let app = offer_app(recipient.clone(), store, transport, blob.clone());
    let topic = TopicId::new("offscreen-public-notification");
    let content = format!("hello @{}", recipient.public_key_hex());
    let docs = MemoryDocsSync::default();
    let envelope = persist_test_post(
        &docs,
        None,
        &sender,
        &topic,
        PayloadRef::InlineText {
            text: content.clone(),
        },
        Vec::new(),
        None,
    )
    .await;
    let source = PublicNotificationSource::Post {
        replica: topic_replica_id(topic.as_str()),
        envelope,
        content,
        reply_target: None,
    };
    for expected in [true, false] {
        assert_eq!(
            deliver_public_source(
                &app,
                &sender,
                &recipient,
                memory_blob.as_ref(),
                source.clone()
            )
            .await
            .unwrap(),
            expected
        );
    }
    let notifications = app.list_notifications().await.unwrap();
    assert_eq!(notifications.len(), 1);
    assert_eq!(notifications[0].kind, NotificationKind::Mention);
    assert_eq!(blob.fetches.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn public_account_offer_rejects_unsigned_content_before_notification_storage() {
    let sender = generate_keys();
    let recipient = generate_keys();
    let store = Arc::new(MemoryStore::default());
    let memory_blob = Arc::new(MemoryBlobService::default());
    let blob = Arc::new(OfferBlobService::new(memory_blob.clone()));
    let transport = Arc::new(FakeTransport::new("recipient", FakeNetwork::default()));
    let app = offer_app(recipient.clone(), store, transport, blob);
    let topic = TopicId::new("offscreen-forged-content");
    let docs = MemoryDocsSync::default();
    let envelope = persist_test_post(
        &docs,
        None,
        &sender,
        &topic,
        PayloadRef::InlineText {
            text: format!("hello @{}", recipient.public_key_hex()),
        },
        Vec::new(),
        None,
    )
    .await;
    assert!(
        deliver_public_source(
            &app,
            &sender,
            &recipient,
            memory_blob.as_ref(),
            PublicNotificationSource::Post {
                replica: topic_replica_id(topic.as_str()),
                envelope,
                content: "forged preview".into(),
                reply_target: None,
            },
        )
        .await
        .is_err()
    );
    assert!(app.list_notifications().await.unwrap().is_empty());
}
