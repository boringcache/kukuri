use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Result;
use async_trait::async_trait;
use futures_util::Stream;
pub use iroh::EndpointAddr;
use kukuri_core::{GossipHint, Pubkey, SealedReceiveOfferV1, TopicId};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::config::{ConnectionPath, DiscoveryMode, DiscoverySnapshot, SeedPeer};

pub type HintStream = Pin<Box<dyn Stream<Item = HintEnvelope> + Send>>;
pub type ReceiveOfferStream = Pin<Box<dyn Stream<Item = ReceiveOfferEnvelope> + Send>>;
pub type ReceiveOfferSubscription = (
    ReceiveOfferLease,
    ReceiveOfferStream,
    watch::Receiver<ReceiveOfferStop>,
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiveOfferStop {
    Active,
    Superseded,
    Closed,
    TransportClosed,
}

/// Process-unique lease so an old account owner cannot close a later receiver,
/// including after an endpoint/transport reload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReceiveOfferLease {
    id: u64,
    transport_instance: u64,
}

static NEXT_RECEIVE_OFFER_LEASE: AtomicU64 = AtomicU64::new(1);

impl ReceiveOfferLease {
    pub fn fresh() -> Self {
        Self::for_instance(0)
    }

    pub(crate) fn for_instance(transport_instance: u64) -> Self {
        Self {
            id: NEXT_RECEIVE_OFFER_LEASE.fetch_add(1, Ordering::Relaxed),
            transport_instance,
        }
    }

    pub fn transport_instance(self) -> u64 {
        self.transport_instance
    }
}

pub(crate) fn next_receive_offer_lease() -> ReceiveOfferLease {
    ReceiveOfferLease::fresh()
}

#[derive(Clone, Debug)]
pub struct ReceiveOfferEnvelope {
    pub offer: SealedReceiveOfferV1,
    pub received_at: i64,
    pub source_peer: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HintEnvelope {
    pub hint: GossipHint,
    pub received_at: i64,
    pub source_peer: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerSnapshot {
    pub connected: bool,
    pub peer_count: usize,
    pub connected_peers: Vec<String>,
    pub configured_peers: Vec<String>,
    pub subscribed_topics: Vec<String>,
    pub active_path: ConnectionPath,
    pub fallback_peer_ids: Vec<String>,
    pub pending_events: usize,
    pub status_detail: String,
    pub last_error: Option<String>,
    pub topic_diagnostics: Vec<TopicPeerSnapshot>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopicPeerSnapshot {
    pub topic: String,
    pub joined: bool,
    pub peer_count: usize,
    pub connected_peers: Vec<String>,
    pub configured_peer_ids: Vec<String>,
    pub missing_peer_ids: Vec<String>,
    pub active_path: ConnectionPath,
    pub rendezvous_peer_ids: Vec<String>,
    pub fallback_peer_ids: Vec<String>,
    pub last_received_at: Option<i64>,
    pub status_detail: String,
    pub last_error: Option<String>,
}

#[async_trait]
pub trait Transport: Send + Sync {
    async fn peers(&self) -> Result<PeerSnapshot>;
    async fn export_ticket(&self) -> Result<Option<String>>;
    async fn import_ticket(&self, ticket: &str) -> Result<()>;
    async fn configure_discovery(
        &self,
        _mode: DiscoveryMode,
        _env_locked: bool,
        _configured_seed_peers: Vec<SeedPeer>,
        _bootstrap_seed_peers: Vec<SeedPeer>,
    ) -> Result<()> {
        Ok(())
    }
    async fn discovery(&self) -> Result<DiscoverySnapshot> {
        Ok(DiscoverySnapshot::default())
    }
}

#[async_trait]
pub trait HintTransport: Send + Sync {
    async fn subscribe_hints(&self, topic: &TopicId) -> Result<HintStream>;
    async fn unsubscribe_hints(&self, topic: &TopicId) -> Result<()>;
    async fn publish_hint(&self, topic: &TopicId, hint: GossipHint) -> Result<()>;

    /// A new lease supersedes the previous consumer, including for the same
    /// account. The underlying route may be reused, but the old stream ends.
    async fn subscribe_receive_offers(
        &self,
        _recipient: &Pubkey,
    ) -> Result<ReceiveOfferSubscription> {
        anyhow::bail!("account receive offers are not supported by this transport")
    }

    /// Atomically restart only while `expected` still owns the route. A
    /// superseded owner receives None and must stop instead of reclaiming it.
    async fn resubscribe_receive_offers_if_current(
        &self,
        _recipient: &Pubkey,
        _expected: ReceiveOfferLease,
    ) -> Result<Option<ReceiveOfferSubscription>> {
        Ok(None)
    }

    /// Claim an empty route after the transport instance has changed. A newer
    /// owner already present on the instance wins; this call returns None.
    async fn subscribe_receive_offers_if_vacant(
        &self,
        _recipient: &Pubkey,
    ) -> Result<Option<ReceiveOfferSubscription>> {
        Ok(None)
    }

    async fn receive_offer_transport_instance(&self) -> Result<u64> {
        Ok(0)
    }

    /// Idempotent. Only the matching lease may close the current route.
    async fn unsubscribe_receive_offers(
        &self,
        _recipient: &Pubkey,
        _lease: ReceiveOfferLease,
    ) -> Result<()> {
        anyhow::bail!("account receive offers are not supported by this transport")
    }

    async fn publish_receive_offer(
        &self,
        _recipient: &Pubkey,
        _destination: EndpointAddr,
        _offer: SealedReceiveOfferV1,
    ) -> Result<()> {
        anyhow::bail!("account receive offers are not supported by this transport")
    }
}
