use super::super::*;
use kukuri_transport::{EndpointAddr, ReceiveOfferEnvelope, ReceiveOfferLease};

#[derive(Default)]
pub(super) struct ProbeOfferTransport {
    pub(super) unsubscribes: AtomicUsize,
    pub(super) subscribe_barrier: Option<Arc<tokio::sync::Barrier>>,
    pub(super) unsubscribe_barrier: Option<Arc<tokio::sync::Barrier>>,
    pub(super) stream_drops: Arc<AtomicUsize>,
    pub(super) stop_senders:
        std::sync::Mutex<Vec<tokio::sync::watch::Sender<kukuri_transport::ReceiveOfferStop>>>,
}

struct CountedPendingOfferStream(Arc<AtomicUsize>);

impl futures_util::Stream for CountedPendingOfferStream {
    type Item = ReceiveOfferEnvelope;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::task::Poll::Pending
    }
}

impl Drop for CountedPendingOfferStream {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[async_trait]
impl HintTransport for ProbeOfferTransport {
    async fn subscribe_hints(&self, _topic: &TopicId) -> Result<HintStream> {
        Ok(Box::pin(futures_util::stream::empty()))
    }

    async fn unsubscribe_hints(&self, _topic: &TopicId) -> Result<()> {
        Ok(())
    }

    async fn publish_hint(&self, _topic: &TopicId, _hint: GossipHint) -> Result<()> {
        Ok(())
    }

    async fn subscribe_receive_offers(
        &self,
        _recipient: &Pubkey,
    ) -> Result<kukuri_transport::ReceiveOfferSubscription> {
        if let Some(barrier) = &self.subscribe_barrier {
            barrier.wait().await;
            barrier.wait().await;
        }
        let (stop, stopped) =
            tokio::sync::watch::channel(kukuri_transport::ReceiveOfferStop::Active);
        self.stop_senders.lock().unwrap().push(stop);
        Ok((
            ReceiveOfferLease::fresh(),
            Box::pin(CountedPendingOfferStream(Arc::clone(&self.stream_drops))),
            stopped,
        ))
    }

    async fn unsubscribe_receive_offers(
        &self,
        _recipient: &Pubkey,
        _lease: ReceiveOfferLease,
    ) -> Result<()> {
        let attempt = self.unsubscribes.fetch_add(1, Ordering::SeqCst);
        if attempt == 0
            && let Some(barrier) = &self.unsubscribe_barrier
        {
            barrier.wait().await;
            barrier.wait().await;
        }
        Ok(())
    }
}

#[derive(Clone)]
pub(super) struct OfferBlobService {
    pub(super) inner: Arc<MemoryBlobService>,
    pub(super) fetches: Arc<AtomicUsize>,
    pub(super) regular_fetches: Arc<AtomicUsize>,
    pub(super) barrier: Option<Arc<tokio::sync::Barrier>>,
    pub(super) pause_blob_hash: Option<kukuri_core::BlobHash>,
    pub(super) attachment_barrier: Option<Arc<tokio::sync::Barrier>>,
    pub(super) writes: Arc<AtomicUsize>,
    pub(super) in_flight: Arc<AtomicUsize>,
}

struct CountedFetch(Arc<AtomicUsize>);

impl Drop for CountedFetch {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl OfferBlobService {
    pub(super) fn new(inner: Arc<MemoryBlobService>) -> Self {
        Self {
            inner,
            fetches: Arc::new(AtomicUsize::new(0)),
            regular_fetches: Arc::new(AtomicUsize::new(0)),
            barrier: None,
            pause_blob_hash: None,
            attachment_barrier: None,
            writes: Arc::new(AtomicUsize::new(0)),
            in_flight: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[async_trait]
impl BlobService for OfferBlobService {
    async fn put_blob(&self, data: Vec<u8>, mime: &str) -> Result<StoredBlob> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.inner.put_blob(data, mime).await
    }

    async fn fetch_blob(&self, hash: &kukuri_core::BlobHash) -> Result<Option<Vec<u8>>> {
        self.regular_fetches.fetch_add(1, Ordering::SeqCst);
        if self.pause_blob_hash.as_ref() == Some(hash)
            && let Some(barrier) = &self.attachment_barrier
        {
            barrier.wait().await;
            barrier.wait().await;
        }
        self.inner.fetch_blob(hash).await
    }

    async fn fetch_local_blob(&self, hash: &kukuri_core::BlobHash) -> Result<Option<Vec<u8>>> {
        self.inner.fetch_local_blob(hash).await
    }

    async fn fetch_verified_receive_offer_payload(
        &self,
        offer: &kukuri_core::VerifiedReceiveOffer,
        provider: EndpointAddr,
    ) -> Result<Vec<u8>> {
        self.fetches.fetch_add(1, Ordering::SeqCst);
        self.in_flight.fetch_add(1, Ordering::SeqCst);
        let _in_flight = CountedFetch(Arc::clone(&self.in_flight));
        if let Some(barrier) = &self.barrier {
            barrier.wait().await;
            barrier.wait().await;
        }
        anyhow::ensure!(
            provider.id.to_string() == offer.reference().provider_endpoint_id,
            "provider mismatch"
        );
        let bytes = self
            .inner
            .fetch_local_blob(&offer.reference().payload_hash)
            .await?
            .ok_or_else(|| anyhow::anyhow!("manifest missing"))?;
        anyhow::ensure!(
            bytes.len() == offer.reference().payload_bytes as usize,
            "manifest length mismatch"
        );
        Ok(bytes)
    }

    async fn pin_blob(&self, hash: &kukuri_core::BlobHash) -> Result<()> {
        self.inner.pin_blob(hash).await
    }

    async fn blob_status(&self, hash: &kukuri_core::BlobHash) -> Result<BlobStatus> {
        self.inner.blob_status(hash).await
    }

    async fn local_blob_status(&self, hash: &kukuri_core::BlobHash) -> Result<BlobStatus> {
        self.inner.local_blob_status(hash).await
    }

    async fn import_peer_ticket(&self, ticket: &str) -> Result<()> {
        self.inner.import_peer_ticket(ticket).await
    }
}
