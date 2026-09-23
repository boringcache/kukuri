//! One account receive route per transport. The wire contains only a sealed,
//! bounded offer; scope and provider authorization remain with the recipient.

use super::*;
use kukuri_core::receive_route_for_account;

const OFFER_BOOTSTRAP_PER_SOURCE: usize = 4;
const MAX_OUTBOUND_OFFER_HOLDS: usize = 32;
const OUTBOUND_OFFER_HOLD: Duration = Duration::from_secs(30);

impl IrohGossipTransport {
    pub(crate) async fn offer_bootstrap_window(&self) -> Vec<EndpointAddr> {
        let mut selected = Vec::with_capacity(3 * OFFER_BOOTSTRAP_PER_SOURCE);
        let mut seen = BTreeSet::new();
        for source in [
            &self.configured_seed_peers,
            &self.bootstrap_seed_peers,
            &self.imported_peers,
        ] {
            let guard = source.lock().await;
            for peer in guard.values().take(OFFER_BOOTSTRAP_PER_SOURCE) {
                if seen.insert(peer.id) {
                    selected.push(peer.clone());
                }
            }
        }
        selected
    }

    pub(super) async fn subscribe_receive_offers_impl(
        &self,
        recipient: &Pubkey,
    ) -> Result<ReceiveOfferStream> {
        let route = receive_route_for_account(recipient)?;
        let mut current = self.receive_offer_topic.lock().await;
        if let Some(state) = current.as_ref()
            && state.route == route.as_str()
        {
            return Ok(stream_from_offer_sender(&state.broadcaster));
        }
        if let Some(old) = current.take() {
            old.receiver_task.abort();
            let _ = old.receiver_task.await;
            self.subscribed_topics.lock().await.remove(&old.route);
        }

        let peers = self.offer_bootstrap_window().await;
        for peer in &peers {
            if !peer.is_empty() {
                self.discovery.add_endpoint_info(peer.clone());
            }
        }
        let topic = self
            .gossip
            .subscribe(
                topics::topic_to_gossip_id(&route),
                peers.into_iter().map(|peer| peer.id).collect(),
            )
            .await?;
        let (sender, mut receiver) = topic.split();
        let (broadcaster, _) = broadcast::channel(64);
        let forward = broadcaster.clone();
        let task = tokio::spawn(async move {
            while let Some(event) = receiver.next().await {
                let Ok(GossipEvent::Received(message)) = event else {
                    continue;
                };
                if let Ok(offer) = SealedReceiveOfferV1::decode(&message.content) {
                    let _ = forward.send(ReceiveOfferEnvelope {
                        offer,
                        received_at: Utc::now().timestamp_millis(),
                        source_peer: message.delivered_from.to_string(),
                    });
                }
            }
        });
        self.subscribed_topics
            .lock()
            .await
            .insert(route.as_str().to_string());
        *current = Some(ReceiveOfferTopicState {
            route: route.as_str().to_string(),
            broadcaster: broadcaster.clone(),
            _sender: sender,
            receiver_task: task,
        });
        Ok(stream_from_offer_sender(&broadcaster))
    }

    pub(super) async fn unsubscribe_receive_offers_impl(&self, recipient: &Pubkey) -> Result<()> {
        let route = receive_route_for_account(recipient)?;
        let mut current = self.receive_offer_topic.lock().await;
        if current
            .as_ref()
            .is_some_and(|state| state.route == route.as_str())
        {
            let old = current.take().expect("matching offer route");
            old.receiver_task.abort();
            let _ = old.receiver_task.await;
            self.subscribed_topics.lock().await.remove(&old.route);
        }
        Ok(())
    }

    pub(super) async fn publish_receive_offer_impl(
        &self,
        recipient: &Pubkey,
        destination: EndpointAddr,
        offer: SealedReceiveOfferV1,
    ) -> Result<()> {
        let payload = offer.encode()?;
        let route = receive_route_for_account(recipient)?;
        let mut peer_ids = vec![destination.id];
        if !destination.is_empty() {
            self.discovery.add_endpoint_info(destination);
        }
        for peer in self.offer_bootstrap_window().await {
            if !peer_ids.contains(&peer.id) {
                if !peer.is_empty() {
                    self.discovery.add_endpoint_info(peer.clone());
                }
                peer_ids.push(peer.id);
            }
        }
        let mut topic = self
            .gossip
            .subscribe(topics::topic_to_gossip_id(&route), peer_ids)
            .await?;
        timeout(Duration::from_secs(10), topic.joined())
            .await
            .context("account receive route join timed out")??;
        topic.broadcast(payload.into()).await?;

        // Keep the sending subscription alive briefly after the gossip actor
        // accepts the message. Eviction and transport shutdown abort every hold.
        let now = tokio::time::Instant::now();
        let task = tokio::spawn(async move {
            sleep(OUTBOUND_OFFER_HOLD).await;
            drop(topic);
        });
        let mut holds = self.outbound_offer_holds.lock().await;
        while holds.front().is_some_and(|hold| hold.expires_at <= now) {
            holds.pop_front();
        }
        holds.push_back(OutboundOfferHold {
            expires_at: now + OUTBOUND_OFFER_HOLD,
            task,
        });
        while holds.len() > MAX_OUTBOUND_OFFER_HOLDS {
            if let Some(old) = holds.pop_front() {
                old.task.abort();
            }
        }
        Ok(())
    }

    pub(super) async fn shutdown_receive_offers(&self) {
        if let Some(state) = self.receive_offer_topic.lock().await.take() {
            state.receiver_task.abort();
            let _ = state.receiver_task.await;
        }
        let holds = self
            .outbound_offer_holds
            .lock()
            .await
            .drain(..)
            .collect::<Vec<_>>();
        for hold in holds {
            hold.task.abort();
            let _ = hold.task.await;
        }
    }
}

fn stream_from_offer_sender(
    sender: &broadcast::Sender<ReceiveOfferEnvelope>,
) -> ReceiveOfferStream {
    let stream =
        BroadcastStream::new(sender.subscribe()).filter_map(|event| async move { event.ok() });
    Box::pin(stream)
}
