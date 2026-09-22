use super::*;

use iroh::RelayMode;
use iroh::endpoint::presets;
use iroh::protocol::Router;
use kukuri_core::KukuriKeys;

async fn endpoint() -> Endpoint {
    Endpoint::builder(presets::Minimal)
        .relay_mode(RelayMode::Disabled)
        .bind_addr("127.0.0.1:0".parse::<std::net::SocketAddr>().unwrap())
        .unwrap()
        .bind()
        .await
        .unwrap()
}

#[tokio::test]
async fn receive_binding_exchange_uses_authenticated_endpoint_without_cn() {
    let sender = endpoint().await;
    let receiver = endpoint().await;
    let keys = KukuriKeys::generate();
    let now = chrono::Utc::now().timestamp_millis();
    let binding =
        ReceiveEndpointBindingV1::sign(&keys, &receiver.id().to_string(), now, now + 60_000)
            .unwrap();
    let handler = ReceiveBindingProtocol::new(receiver.id(), binding.clone()).unwrap();
    let router = Router::builder(receiver.clone())
        .accept(RECEIVE_BINDING_ALPN, handler)
        .spawn();

    let verified = fetch_receive_endpoint_binding(
        &sender,
        receiver.addr(),
        &keys.public_key(),
        Instant::now() + Duration::from_secs(5),
    )
    .await
    .unwrap();
    assert_eq!(verified.account(), &keys.public_key());
    assert_eq!(verified.endpoint_id(), receiver.id().to_string());
    assert_eq!(verified.route(), &binding.route);
    assert!(
        fetch_receive_endpoint_binding(
            &sender,
            receiver.addr(),
            &KukuriKeys::generate().public_key(),
            Instant::now() + Duration::from_secs(5),
        )
        .await
        .is_err()
    );

    router.shutdown().await.unwrap();
    sender.close().await;
}

#[tokio::test]
async fn receive_binding_replacement_cannot_switch_account_or_endpoint() {
    let endpoint = endpoint().await;
    let keys = KukuriKeys::generate();
    let now = chrono::Utc::now().timestamp_millis();
    let binding =
        ReceiveEndpointBindingV1::sign(&keys, &endpoint.id().to_string(), now, now + 60_000)
            .unwrap();
    let handler = ReceiveBindingProtocol::new(endpoint.id(), binding).unwrap();
    let refreshed =
        ReceiveEndpointBindingV1::sign(&keys, &endpoint.id().to_string(), now + 1, now + 60_001)
            .unwrap();
    handler.replace(refreshed.clone()).await.unwrap();
    let stale =
        ReceiveEndpointBindingV1::sign(&keys, &endpoint.id().to_string(), now, now + 60_000)
            .unwrap();
    assert!(handler.replace(stale).await.is_err());
    let other_account = ReceiveEndpointBindingV1::sign(
        &KukuriKeys::generate(),
        &endpoint.id().to_string(),
        now,
        now + 60_000,
    )
    .unwrap();
    assert!(handler.replace(other_account).await.is_err());
    let other_endpoint = ReceiveEndpointBindingV1::sign(
        &keys,
        &iroh::SecretKey::generate().public().to_string(),
        now,
        now + 60_000,
    )
    .unwrap();
    assert!(handler.replace(other_endpoint).await.is_err());
    assert_eq!(*handler.binding.read().await, refreshed);
    endpoint.close().await;
}

#[tokio::test]
async fn receive_binding_full_server_rejects_instead_of_waiting() {
    let sender = endpoint().await;
    let receiver = endpoint().await;
    let keys = KukuriKeys::generate();
    let now = chrono::Utc::now().timestamp_millis();
    let binding =
        ReceiveEndpointBindingV1::sign(&keys, &receiver.id().to_string(), now, now + 60_000)
            .unwrap();
    let handler = ReceiveBindingProtocol::new(receiver.id(), binding).unwrap();
    let held = handler
        .permits
        .clone()
        .acquire_many_owned(RECEIVE_BINDING_CONCURRENT_REQUESTS as u32)
        .await
        .unwrap();
    let router = Router::builder(receiver.clone())
        .accept(RECEIVE_BINDING_ALPN, handler)
        .spawn();
    let error = fetch_receive_endpoint_binding(
        &sender,
        receiver.addr(),
        &keys.public_key(),
        Instant::now() + Duration::from_secs(5),
    )
    .await
    .unwrap_err();
    assert!(
        !error.to_string().contains("timed out"),
        "full admission must reject immediately: {error}"
    );
    drop(held);
    assert!(
        fetch_receive_endpoint_binding(
            &sender,
            receiver.addr(),
            &keys.public_key(),
            Instant::now() + Duration::from_secs(5),
        )
        .await
        .is_ok()
    );
    router.shutdown().await.unwrap();
    sender.close().await;
}

#[derive(Debug)]
struct ReplayBinding(Vec<u8>);

impl ProtocolHandler for ReplayBinding {
    async fn accept(&self, connection: Connection) -> std::result::Result<(), AcceptError> {
        let (mut send, mut recv) = connection.accept_bi().await?;
        recv.read_to_end(1).await.map_err(AcceptError::from_err)?;
        send.write_all(&self.0)
            .await
            .map_err(AcceptError::from_err)?;
        send.finish()?;
        let _ = send.stopped().await;
        Ok(())
    }
}

#[tokio::test]
async fn receive_binding_replay_from_another_endpoint_is_rejected() {
    let client = endpoint().await;
    let attacker = endpoint().await;
    let keys = KukuriKeys::generate();
    let victim = iroh::SecretKey::generate().public();
    let now = chrono::Utc::now().timestamp_millis();
    let binding =
        ReceiveEndpointBindingV1::sign(&keys, &victim.to_string(), now, now + 60_000).unwrap();
    let router = Router::builder(attacker.clone())
        .accept(
            RECEIVE_BINDING_ALPN,
            ReplayBinding(serde_json::to_vec(&binding).unwrap()),
        )
        .spawn();
    let error = fetch_receive_endpoint_binding(
        &client,
        attacker.addr(),
        &keys.public_key(),
        Instant::now() + Duration::from_secs(5),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("endpoint mismatch"), "{error}");
    router.shutdown().await.unwrap();
    client.close().await;
}

#[derive(Debug)]
struct StalledBinding {
    requested: Arc<tokio::sync::Notify>,
    closed: Arc<tokio::sync::Notify>,
}

impl ProtocolHandler for StalledBinding {
    async fn accept(&self, connection: Connection) -> std::result::Result<(), AcceptError> {
        let (_send, mut recv) = connection.accept_bi().await?;
        recv.read_to_end(1).await.map_err(AcceptError::from_err)?;
        self.requested.notify_one();
        connection.closed().await;
        self.closed.notify_one();
        Ok(())
    }
}

#[tokio::test]
async fn receive_binding_cancel_closes_the_connection() {
    let client = endpoint().await;
    let receiver = endpoint().await;
    let requested = Arc::new(tokio::sync::Notify::new());
    let closed = Arc::new(tokio::sync::Notify::new());
    let router = Router::builder(receiver.clone())
        .accept(
            RECEIVE_BINDING_ALPN,
            StalledBinding {
                requested: requested.clone(),
                closed: closed.clone(),
            },
        )
        .spawn();
    let lookup = tokio::spawn({
        let client = client.clone();
        let candidate = receiver.addr();
        async move {
            fetch_receive_endpoint_binding(
                &client,
                candidate,
                &KukuriKeys::generate().public_key(),
                Instant::now() + Duration::from_secs(30),
            )
            .await
        }
    });
    timeout(Duration::from_secs(5), requested.notified())
        .await
        .unwrap();
    lookup.abort();
    assert!(lookup.await.unwrap_err().is_cancelled());
    timeout(Duration::from_secs(2), closed.notified())
        .await
        .expect("caller cancellation must close QUIC without waiting for its deadline");
    router.shutdown().await.unwrap();
    client.close().await;
}
