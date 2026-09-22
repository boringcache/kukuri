//! P2: native LiveActorの自動選択を使わず、公開APIで同期を所有できるかのcontract。

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use iroh::endpoint::{Connection, presets};
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use iroh::{Endpoint, RelayMode};
use iroh_docs::actor::{OpenOpts, SyncHandle};
use iroh_docs::net::{AbortReason, AcceptOutcome, connect_and_sync, handle_connection};
use iroh_docs::{Author, Capability, NamespaceId, NamespaceSecret};
use tokio::sync::Notify;
use tokio::time::timeout;

#[derive(Debug)]
struct SelectedNamespace {
    sync: SyncHandle,
    namespace: NamespaceId,
    accepted: Arc<AtomicUsize>,
    rejected: Arc<AtomicUsize>,
}

impl ProtocolHandler for SelectedNamespace {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        handle_connection(
            self.sync.clone(),
            connection,
            |namespace, _peer| {
                let outcome = if namespace == self.namespace {
                    self.accepted.fetch_add(1, Ordering::SeqCst);
                    AcceptOutcome::Allow
                } else {
                    self.rejected.fetch_add(1, Ordering::SeqCst);
                    AcceptOutcome::Reject(AbortReason::NotFound)
                };
                std::future::ready(outcome)
            },
            None,
        )
        .await
        .map(|_| ())
        .map_err(AcceptError::from_err)
    }
}

struct TestDocsNode {
    endpoint: Endpoint,
    sync: SyncHandle,
    router: Router,
    accepted: Arc<AtomicUsize>,
    rejected: Arc<AtomicUsize>,
}

impl TestDocsNode {
    async fn new(secret: &NamespaceSecret) -> Self {
        let endpoint = endpoint().await;
        let sync = SyncHandle::spawn(
            iroh_docs::store::Store::memory(),
            None,
            "explicit-sync-test".into(),
        );
        let namespace = sync
            .import_namespace(Capability::Write(secret.clone()))
            .await
            .unwrap();
        sync.open(namespace, OpenOpts::default().sync())
            .await
            .unwrap();
        let accepted = Arc::new(AtomicUsize::new(0));
        let rejected = Arc::new(AtomicUsize::new(0));
        let router = Router::builder(endpoint.clone())
            .accept(
                iroh_docs::ALPN,
                SelectedNamespace {
                    sync: sync.clone(),
                    namespace,
                    accepted: accepted.clone(),
                    rejected: rejected.clone(),
                },
            )
            .spawn();
        Self {
            endpoint,
            sync,
            router,
            accepted,
            rejected,
        }
    }

    async fn stop(self) {
        self.router.shutdown().await.unwrap();
        self.sync.shutdown().await.unwrap();
    }
}

async fn endpoint() -> Endpoint {
    Endpoint::builder(presets::Minimal)
        .relay_mode(RelayMode::Disabled)
        .bind_addr("127.0.0.1:0".parse::<std::net::SocketAddr>().unwrap())
        .unwrap()
        .bind()
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_docs_sync_ignores_cached_peer_and_rejects_other_namespace() {
    let secret = NamespaceSecret::from_bytes(&[7; 32]);
    let namespace = secret.id();
    let client = TestDocsNode::new(&secret).await;
    let selected = TestDocsNode::new(&secret).await;
    let excluded = TestDocsNode::new(&secret).await;
    let author = Author::from_bytes(&[3; 32]);
    let author_id = selected.sync.import_author(author.clone()).await.unwrap();
    excluded.sync.import_author(author).await.unwrap();
    let hash = iroh_blobs::Hash::new(b"body");
    selected
        .sync
        .insert_local(namespace, author_id, b"selected".to_vec().into(), hash, 4)
        .await
        .unwrap();
    excluded
        .sync
        .insert_local(namespace, author_id, b"excluded".to_vec().into(), hash, 4)
        .await
        .unwrap();
    client
        .sync
        .register_useful_peer(namespace, *excluded.endpoint.id().as_bytes())
        .await
        .unwrap();
    assert!(
        client
            .sync
            .get_sync_peers(namespace)
            .await
            .unwrap()
            .unwrap()
            .contains(excluded.endpoint.id().as_bytes())
    );

    let result = timeout(
        Duration::from_secs(5),
        connect_and_sync(
            &client.endpoint,
            &client.sync,
            namespace,
            selected.endpoint.addr(),
            None,
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(result.outcome.num_recv, 1);
    assert_eq!(selected.accepted.load(Ordering::SeqCst), 1);
    assert_eq!(excluded.accepted.load(Ordering::SeqCst), 0);
    assert!(
        client
            .sync
            .get_exact(namespace, author_id, b"selected".to_vec().into(), false)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        client
            .sync
            .get_exact(namespace, author_id, b"excluded".to_vec().into(), false)
            .await
            .unwrap()
            .is_none()
    );

    let other_secret = NamespaceSecret::from_bytes(&[8; 32]);
    let other = client
        .sync
        .import_namespace(Capability::Write(other_secret.clone()))
        .await
        .unwrap();
    client
        .sync
        .open(other, OpenOpts::default().sync())
        .await
        .unwrap();
    selected
        .sync
        .import_namespace(Capability::Write(other_secret))
        .await
        .unwrap();
    selected
        .sync
        .open(other, OpenOpts::default().sync())
        .await
        .unwrap();
    selected
        .sync
        .insert_local(other, author_id, b"private".to_vec().into(), hash, 4)
        .await
        .unwrap();
    assert!(
        timeout(
            Duration::from_secs(5),
            connect_and_sync(
                &client.endpoint,
                &client.sync,
                other,
                selected.endpoint.addr(),
                None,
            )
        )
        .await
        .unwrap()
        .is_err()
    );
    assert_eq!(selected.rejected.load(Ordering::SeqCst), 1);
    assert!(
        client
            .sync
            .get_exact(other, author_id, b"private".to_vec().into(), false)
            .await
            .unwrap()
            .is_none()
    );

    client.sync.set_sync(namespace, false).await.unwrap();
    client.sync.close(namespace).await.unwrap();
    client
        .sync
        .open(namespace, OpenOpts::default())
        .await
        .unwrap();
    assert!(!client.sync.get_state(namespace).await.unwrap().sync);
    assert!(
        client
            .sync
            .get_exact(namespace, author_id, b"selected".to_vec().into(), false)
            .await
            .unwrap()
            .is_some()
    );
    client.stop().await;
    selected.stop().await;
    excluded.stop().await;
}

#[derive(Debug)]
struct StalledSync {
    started: Arc<Notify>,
    closed: Arc<Notify>,
}

impl ProtocolHandler for StalledSync {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let (_send, mut recv) = connection.accept_bi().await?;
        let mut first = [0; 1];
        recv.read_exact(&mut first)
            .await
            .map_err(AcceptError::from_err)?;
        self.started.notify_one();
        connection.closed().await;
        self.closed.notify_one();
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_docs_sync_cancel_closes_remote_connection() {
    let secret = NamespaceSecret::from_bytes(&[7; 32]);
    let client = TestDocsNode::new(&secret).await;
    let server = endpoint().await;
    let started = Arc::new(Notify::new());
    let closed = Arc::new(Notify::new());
    let router = Router::builder(server.clone())
        .accept(
            iroh_docs::ALPN,
            StalledSync {
                started: started.clone(),
                closed: closed.clone(),
            },
        )
        .spawn();
    let work = tokio::spawn({
        let endpoint = client.endpoint.clone();
        let sync = client.sync.clone();
        let peer = server.addr();
        async move { connect_and_sync(&endpoint, &sync, secret.id(), peer, None).await }
    });
    timeout(Duration::from_secs(5), started.notified())
        .await
        .unwrap();
    work.abort();
    assert!(work.await.unwrap_err().is_cancelled());
    timeout(Duration::from_secs(2), closed.notified())
        .await
        .expect("sync future cancellation must close its QUIC connection");
    router.shutdown().await.unwrap();
    client.stop().await;
}
