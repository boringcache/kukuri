use std::sync::Arc;
use std::time::Duration;

use kukuri_core::{
    BlobHash, KukuriKeys, ReceiveOfferReferenceV1, ReceiveOfferScopeV1, seal_receive_offer,
};
use tokio::time::timeout;

use crate::{IrohDocsNode, remote_fetch};

fn sealed_offer(
    sender: &KukuriKeys,
    recipient: &KukuriKeys,
    provider_endpoint_id: String,
    payload_hash: String,
    payload_bytes: u32,
) -> kukuri_core::VerifiedReceiveOffer {
    let now: i64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .try_into()
        .unwrap();
    seal_receive_offer(
        sender,
        &recipient.public_key(),
        ReceiveOfferReferenceV1 {
            provider_endpoint_id,
            payload_hash: BlobHash(payload_hash),
            payload_bytes,
            scope: ReceiveOfferScopeV1::PublicSource,
        },
        now,
        now + 60_000,
    )
    .unwrap()
    .open(recipient, now)
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn signed_provider_offer_fetches_only_its_bounded_manifest_without_storing_it() {
    let provider = IrohDocsNode::memory().await.unwrap();
    let receiver = IrohDocsNode::memory().await.unwrap();
    let sender_keys = Arc::new(KukuriKeys::generate());
    let recipient_keys = KukuriKeys::generate();
    provider
        .install_receive_binding(Arc::clone(&sender_keys))
        .await
        .unwrap();
    let payload = b"bounded public source manifest".to_vec();
    let tag = provider
        .blobs()
        .blobs()
        .add_bytes(payload.clone())
        .await
        .unwrap();
    let hash = tag.hash;
    let offer = sealed_offer(
        &sender_keys,
        &recipient_keys,
        provider.endpoint().id().to_string(),
        hash.to_string(),
        payload.len() as u32,
    );
    let bytes = timeout(
        Duration::from_secs(5),
        remote_fetch::fetch_verified_receive_offer_payload(
            &receiver,
            &offer,
            provider.endpoint().addr(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(bytes, payload);
    assert!(receiver.blobs().blobs().get_bytes(hash).await.is_err());
    drop(tag);
    provider.shutdown().await.unwrap();
    receiver.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offer_fetch_rejects_wrong_endpoint_missing_binding_and_declared_length() {
    let provider = IrohDocsNode::memory().await.unwrap();
    let receiver = IrohDocsNode::memory().await.unwrap();
    let other = IrohDocsNode::memory().await.unwrap();
    let sender_keys = Arc::new(KukuriKeys::generate());
    let recipient_keys = KukuriKeys::generate();
    let payload = b"signed manifest".to_vec();
    let tag = provider
        .blobs()
        .blobs()
        .add_bytes(payload.clone())
        .await
        .unwrap();
    let hash = tag.hash;
    let offer = sealed_offer(
        &sender_keys,
        &recipient_keys,
        provider.endpoint().id().to_string(),
        hash.to_string(),
        payload.len() as u32,
    );
    assert!(
        remote_fetch::fetch_verified_receive_offer_payload(
            &receiver,
            &offer,
            other.endpoint().addr()
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("provider endpoint mismatch")
    );
    assert!(
        remote_fetch::fetch_verified_receive_offer_payload(
            &receiver,
            &offer,
            provider.endpoint().addr(),
        )
        .await
        .is_err(),
        "a blob-serving endpoint without the sender account binding is not authorized"
    );
    let other_tag = other
        .blobs()
        .blobs()
        .add_bytes(payload.clone())
        .await
        .unwrap();
    other
        .install_receive_binding(Arc::new(KukuriKeys::generate()))
        .await
        .unwrap();
    let misbound_offer = sealed_offer(
        &sender_keys,
        &recipient_keys,
        other.endpoint().id().to_string(),
        hash.to_string(),
        payload.len() as u32,
    );
    assert!(
        remote_fetch::fetch_verified_receive_offer_payload(
            &receiver,
            &misbound_offer,
            other.endpoint().addr(),
        )
        .await
        .is_err(),
        "a provider with the bytes but another account's binding is not authorized"
    );

    provider
        .install_receive_binding(Arc::clone(&sender_keys))
        .await
        .unwrap();
    let short_offer = sealed_offer(
        &sender_keys,
        &recipient_keys,
        provider.endpoint().id().to_string(),
        hash.to_string(),
        (payload.len() - 1) as u32,
    );
    assert!(
        remote_fetch::fetch_verified_receive_offer_payload(
            &receiver,
            &short_offer,
            provider.endpoint().addr(),
        )
        .await
        .is_err()
    );
    assert!(receiver.blobs().blobs().get_bytes(hash).await.is_err());
    drop(tag);
    drop(other_tag);
    provider.shutdown().await.unwrap();
    receiver.shutdown().await.unwrap();
    other.shutdown().await.unwrap();
}

#[tokio::test]
async fn retained_expired_offer_is_rejected_before_provider_io() {
    let provider = IrohDocsNode::memory().await.unwrap();
    let receiver = IrohDocsNode::memory().await.unwrap();
    let sender = KukuriKeys::generate();
    let recipient = KukuriKeys::generate();
    let now: i64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .try_into()
        .unwrap();
    let issued_at = now - 2_000;
    let sealed = seal_receive_offer(
        &sender,
        &recipient.public_key(),
        ReceiveOfferReferenceV1 {
            provider_endpoint_id: provider.endpoint().id().to_string(),
            payload_hash: BlobHash("11".repeat(32)),
            payload_bytes: 1,
            scope: ReceiveOfferScopeV1::PublicSource,
        },
        issued_at,
        now - 1,
    )
    .unwrap();
    let verified = sealed.open(&recipient, issued_at).unwrap();
    let error = remote_fetch::fetch_verified_receive_offer_payload(
        &receiver,
        &verified,
        provider.endpoint().addr(),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("expired before fetch"));
    provider.shutdown().await.unwrap();
    receiver.shutdown().await.unwrap();
}
