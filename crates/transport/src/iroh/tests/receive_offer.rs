use super::*;

use kukuri_core::{
    BlobHash, KukuriKeys, RECEIVE_OFFER_MAX_BYTES, ReceiveOfferReferenceV1, ReceiveOfferScopeV1,
    SealedReceiveOfferV1, receive_epoch_key_id, receive_route_for_account,
    seal_private_receive_payload, seal_receive_offer,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn account_receive_offer_crosses_real_gossip_with_one_recipient_route() {
    let mut left = IrohGossipTransport::bind_local().await.unwrap();
    let mut right = IrohGossipTransport::bind_local().await.unwrap();
    left.discovery.add_endpoint_info(right.endpoint.addr());
    right.discovery.add_endpoint_info(left.endpoint.addr());
    let sender = KukuriKeys::generate();
    let recipient = KukuriKeys::generate();
    let route = receive_route_for_account(&recipient.public_key()).unwrap();
    let topic = topic_to_gossip_id(&route);
    let mut outgoing = left
        .gossip
        .subscribe(topic, vec![right.endpoint.id()])
        .await
        .unwrap();
    let mut incoming = right
        .gossip
        .subscribe(topic, vec![left.endpoint.id()])
        .await
        .unwrap();
    timeout(Duration::from_secs(10), async {
        tokio::try_join!(outgoing.joined(), incoming.joined())
    })
    .await
    .unwrap()
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
        outgoing.broadcast(wire.into()).await.unwrap();
        let received = timeout(Duration::from_secs(5), async {
            for _ in 0..8 {
                if let Some(Ok(GossipEvent::Received(message))) = incoming.next().await {
                    return message.content;
                }
            }
            panic!("receive offer was not delivered within the event budget");
        })
        .await
        .unwrap();
        let sealed = SealedReceiveOfferV1::decode(&received).unwrap();
        let opened = sealed.open(&recipient, now).unwrap();
        assert_eq!(opened.sender(), &sender.public_key());
        assert_eq!(opened.reference(), &reference);
        assert!(sealed.open(&KukuriKeys::generate(), now).is_err());
    }

    drop(outgoing);
    drop(incoming);
    left._router.take().unwrap().shutdown().await.unwrap();
    right._router.take().unwrap().shutdown().await.unwrap();
}
