use super::*;

pub(crate) fn initial_topic_join_timeout() -> Duration {
    if cfg!(target_os = "windows") || std::env::var_os("GITHUB_ACTIONS").is_some() {
        Duration::from_secs(180)
    } else {
        Duration::from_secs(15)
    }
}

fn topic_warmup_retry_delay(attempt: usize, relay_backed: bool) -> Duration {
    if relay_backed {
        return relay_topic_warmup_retry_delay(attempt);
    }
    direct_topic_warmup_retry_delay(attempt)
}

fn direct_topic_warmup_retry_delay(attempt: usize) -> Duration {
    match attempt {
        0 => Duration::from_millis(250),
        1 => Duration::from_millis(500),
        2 => Duration::from_secs(1),
        3 => Duration::from_secs(2),
        _ => Duration::from_secs(5),
    }
}

fn relay_topic_warmup_retry_delay(attempt: usize) -> Duration {
    match attempt {
        0 => Duration::from_secs(1),
        1 => Duration::from_secs(2),
        2 => Duration::from_secs(4),
        3 => Duration::from_secs(8),
        _ => Duration::from_secs(10),
    }
}

fn peers_use_relay(peers: &[EndpointAddr]) -> bool {
    peers.iter().any(|peer| peer.relay_urls().next().is_some())
}

fn direct_warmup_addr(endpoint_addr: &EndpointAddr) -> EndpointAddr {
    endpoint_addr.clone()
}
pub(crate) fn topic_to_gossip_id(topic: &TopicId) -> GossipTopicId {
    let hash = blake3::hash(topic.as_str().as_bytes());
    GossipTopicId::from_bytes(*hash.as_bytes())
}

impl TopicWarmupCoordinator {
    async fn warmup_peers_once(
        &self,
        endpoint: &Endpoint,
        gossip: &Gossip,
        peers: &[EndpointAddr],
    ) {
        let selected = self.warmup_window(peers);
        futures_util::stream::iter(selected)
            .for_each_concurrent(2, |peer| {
                let endpoint = endpoint.clone();
                let gossip = gossip.clone();
                async move {
                    self.warmup_peer(endpoint, gossip, peer).await;
                }
            })
            .await;
    }

    fn warmup_window(&self, peers: &[EndpointAddr]) -> Vec<EndpointAddr> {
        if peers.is_empty() {
            return Vec::new();
        }
        let start = self.warmup_cursor.fetch_add(4, Ordering::Relaxed) as usize % peers.len();
        (0..peers.len().min(4))
            .map(|offset| peers[(start + offset) % peers.len()].clone())
            .collect()
    }

    async fn warmup_peer(&self, endpoint: Endpoint, gossip: Gossip, peer: EndpointAddr) {
        let Ok(_permit) = self.permits.try_acquire() else {
            return;
        };
        let peer_key = peer.id.to_string();
        let Some(_in_flight_guard) = self.try_mark_peer_in_flight(peer_key) else {
            return;
        };
        // Active endpoint paths can belong to docs/blob connections. They do not
        // prove that gossip has a connection, especially after a peer restart.
        // Keep gossip dialing behind the shared concurrency and retry limits.
        let warmup_addr = direct_warmup_addr(&peer);
        if let Ok(connection) = endpoint.connect(warmup_addr, GOSSIP_ALPN).await {
            let _ = gossip.handle_connection(connection).await;
        }
    }

    fn try_mark_peer_in_flight(&self, peer_key: String) -> Option<TopicWarmupInFlightGuard> {
        let mut in_flight_peers = self
            .in_flight_peers
            .write()
            .expect("topic warmup in-flight lock poisoned");
        if !in_flight_peers.insert(peer_key.clone()) {
            return None;
        }
        Some(TopicWarmupInFlightGuard {
            peer_key,
            in_flight_peers: Arc::clone(&self.in_flight_peers),
        })
    }

    #[cfg(test)]
    fn try_mark_peer_in_flight_for_test(&self, peer_key: &str) -> bool {
        self.in_flight_peers
            .write()
            .expect("topic warmup in-flight lock poisoned")
            .insert(peer_key.to_string())
    }

    #[cfg(test)]
    fn clear_in_flight_for_test(&self, peer_key: &str) {
        self.in_flight_peers
            .write()
            .expect("topic warmup in-flight lock poisoned")
            .remove(peer_key);
    }
}

struct TopicWarmupInFlightGuard {
    peer_key: String,
    in_flight_peers: Arc<StdRwLock<BTreeSet<String>>>,
}

struct AbortWarmupOnDrop(JoinHandle<()>);

impl Drop for AbortWarmupOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn topic_closed(closed: &AtomicBool, notify: &Notify) {
    loop {
        let stopped = notify.notified();
        tokio::pin!(stopped);
        stopped.as_mut().enable();
        if closed.load(Ordering::Acquire) {
            return;
        }
        stopped.await;
    }
}

#[cfg(test)]
struct CountedWarmupTask(Arc<AtomicUsize>);

#[cfg(test)]
impl CountedWarmupTask {
    fn new(count: Arc<AtomicUsize>) -> Self {
        count.fetch_add(1, Ordering::SeqCst);
        Self(count)
    }
}

#[cfg(test)]
impl Drop for CountedWarmupTask {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Drop for TopicWarmupInFlightGuard {
    fn drop(&mut self) {
        if let Ok(mut in_flight_peers) = self.in_flight_peers.write() {
            in_flight_peers.remove(&self.peer_key);
        }
    }
}

impl IrohGossipTransport {
    async fn remove_topic_state(&self, topic: &str) {
        let mut topics = self.topic_states.lock().await;
        if let Some(mut state) = topics.remove(topic) {
            state.closed.store(true, Ordering::Release);
            state.closed_notify.notify_waiters();
            state._receiver_task.abort();
            let update = state.update_warmup_task.take();
            if let Some(update) = &update {
                update.abort();
            }
            self.subscribed_topics.lock().await.remove(topic);
            let _ = state._receiver_task.await;
            if let Some(update) = update {
                let _ = update.await;
            }
        } else {
            self.subscribed_topics.lock().await.remove(topic);
        }
    }

    pub(crate) async fn extend_active_topic_peers(
        &self,
        endpoint_addrs: Vec<EndpointAddr>,
        reason: &str,
    ) {
        if endpoint_addrs.is_empty() || self.hint_closed.load(Ordering::Acquire) {
            return;
        }
        let mut updates = Vec::new();
        {
            let mut topic_states = self.topic_states.lock().await;
            if self.hint_closed.load(Ordering::Acquire) {
                return;
            }
            for (topic, state) in topic_states.iter_mut() {
                let mut join_peer_ids = Vec::new();
                let mut added_peer_ids = Vec::new();
                let mut join_endpoint_addrs = Vec::new();
                for endpoint_addr in &endpoint_addrs {
                    let peer_id = endpoint_addr.id.to_string();
                    if state.bootstrap_peer_ids.insert(peer_id.clone()) {
                        join_peer_ids.push(endpoint_addr.id);
                        added_peer_ids.push(peer_id);
                        join_endpoint_addrs.push(endpoint_addr.clone());
                    }
                }
                if !join_peer_ids.is_empty() {
                    updates.push((
                        topic.clone(),
                        state.sender.clone(),
                        Arc::clone(&state.neighbors),
                        Arc::clone(&state.closed),
                        Arc::clone(&state.closed_notify),
                        added_peer_ids,
                        join_peer_ids,
                        join_endpoint_addrs,
                    ));
                }
            }
        }

        for (
            topic,
            sender,
            neighbors,
            closed,
            closed_notify,
            added_peer_ids,
            join_peer_ids,
            join_endpoint_addrs,
        ) in updates
        {
            info!(
                topic = %topic,
                reason,
                added_peer_ids = ?added_peer_ids,
                "updating active gossip topic peers"
            );
            let join = async { sender.lock().await.join_peers(join_peer_ids).await };
            let result = tokio::select! {
                biased;
                _ = topic_closed(&closed, &closed_notify) => continue,
                result = join => result,
            };
            if let Err(error) = result {
                warn!(
                    topic = %topic,
                    reason,
                    added_peer_ids = ?added_peer_ids,
                    error = %error,
                    "failed to join updated peers on active gossip topic"
                );
            }

            let endpoint = self.endpoint.clone();
            let gossip = self.gossip.clone();
            let warmups = Arc::clone(&self.topic_warmups);
            let mut topics = self.topic_states.lock().await;
            let Some(state) = topics.get_mut(&topic) else {
                continue;
            };
            if !Arc::ptr_eq(&state.closed, &closed) || closed.load(Ordering::Acquire) {
                continue;
            }
            if let Some(previous) = state.update_warmup_task.take() {
                previous.abort();
                let _ = previous.await;
            }
            let task = tokio::spawn(async move {
                let join_deadline = tokio::time::Instant::now() + initial_topic_join_timeout();
                let relay_backed = peers_use_relay(&join_endpoint_addrs);
                let mut attempt = 0usize;
                loop {
                    let already_connected = {
                        let guard = neighbors.read().await;
                        join_endpoint_addrs
                            .iter()
                            .any(|peer| guard.contains(&peer.id.to_string()))
                    };
                    if already_connected || tokio::time::Instant::now() >= join_deadline {
                        return;
                    }
                    warmups
                        .warmup_peers_once(&endpoint, &gossip, &join_endpoint_addrs)
                        .await;
                    if tokio::time::Instant::now() >= join_deadline {
                        return;
                    }
                    let retry_delay = topic_warmup_retry_delay(attempt, relay_backed);
                    attempt = attempt.saturating_add(1);
                    sleep(retry_delay).await;
                }
            });
            state.update_warmup_task = Some(task);
        }
    }

    async fn ensure_hint_topic(&self, topic: &TopicId) -> Result<broadcast::Sender<HintEnvelope>> {
        anyhow::ensure!(
            !self.hint_closed.load(Ordering::Acquire),
            "hint transport is closed"
        );
        let bootstrap_peers = self.bootstrap_peers().await;
        let bootstrap_peer_ids = bootstrap_peers
            .iter()
            .map(|peer| peer.id.to_string())
            .collect::<BTreeSet<_>>();

        let existing = {
            let topics = self.topic_states.lock().await;
            topics.get(topic.as_str()).map(|state| {
                (
                    state.broadcaster.clone(),
                    state.bootstrap_peer_ids.clone(),
                    Arc::clone(&state.neighbors),
                    Arc::clone(&state.last_error),
                )
            })
        };

        if let Some((_broadcaster, existing_bootstrap_peer_ids, neighbors, last_error)) = existing {
            let has_neighbors = !neighbors.read().await.is_empty();
            let timed_out_join = last_error
                .lock()
                .await
                .as_deref()
                .is_some_and(|message| message.contains("initial topic join"));
            if existing_bootstrap_peer_ids == bootstrap_peer_ids
                && (!timed_out_join || has_neighbors)
            {
                let topics = self.topic_states.lock().await;
                let mut subscribed = self.subscribed_topics.lock().await;
                anyhow::ensure!(
                    !self.hint_closed.load(Ordering::Acquire),
                    "hint transport is closed"
                );
                if let Some(current) = topics.get(topic.as_str()) {
                    subscribed.insert(topic.0.clone());
                    return Ok(current.broadcaster.clone());
                }
            }
            self.remove_topic_state(topic.as_str()).await;
        }

        let bootstrap = bootstrap_peers
            .iter()
            .map(|peer| peer.id)
            .collect::<Vec<_>>();

        for peer in &bootstrap_peers {
            self.discovery.add_endpoint_info(peer.clone());
        }

        let topic_handle = match self
            .gossip
            .subscribe(topic_to_gossip_id(topic), bootstrap)
            .await
        {
            Ok(topic_handle) => topic_handle,
            Err(error) => {
                let message = format!("failed to subscribe gossip topic: {error}");
                *self.last_error.lock().await = Some(message.clone());
                return Err(anyhow!(message));
            }
        };
        let (sender, mut receiver) = topic_handle.split();
        let (broadcaster, _) = broadcast::channel(256);
        let outbound = broadcaster.clone();
        let topic_name = topic.as_str().to_string();
        let joined = Arc::new(AtomicBool::new(bootstrap_peers.is_empty()));
        let joined_notify = Arc::new(Notify::new());
        let joined_task_state = Arc::clone(&joined);
        let joined_task_notify = Arc::clone(&joined_notify);
        let neighbors = Arc::new(RwLock::new(BTreeSet::new()));
        let neighbors_task = Arc::clone(&neighbors);
        let last_received_at = Arc::new(Mutex::new(None));
        let last_received_at_task = Arc::clone(&last_received_at);
        let last_error = Arc::new(Mutex::new(None));
        let last_error_task = Arc::clone(&last_error);
        let invalid_hint_count = Arc::new(AtomicU64::new(0));
        let invalid_hint_count_task = Arc::clone(&invalid_hint_count);
        let closed = Arc::new(AtomicBool::new(false));
        let closed_notify = Arc::new(Notify::new());
        let transport_last_error = Arc::clone(&self.last_error);
        let imported_count = bootstrap_peers.len();
        let warm_endpoint = self.endpoint.clone();
        let warm_bootstrap_peers = bootstrap_peers.clone();
        let warm_gossip = self.gossip.clone();
        let warmups = Arc::clone(&self.topic_warmups);

        // The receiver must enter the registry in the same non-await section
        // in which it is spawned. Shutdown and concurrent subscribe use these
        // locks in the same order.
        let mut topics = self.topic_states.lock().await;
        let mut subscribed = self.subscribed_topics.lock().await;
        anyhow::ensure!(
            !self.hint_closed.load(Ordering::Acquire),
            "hint transport is closed"
        );
        if let Some(current) = topics.get(topic.as_str()) {
            subscribed.insert(topic.0.clone());
            return Ok(current.broadcaster.clone());
        }

        let task = tokio::spawn(async move {
            if imported_count > 0 {
                let join_timeout = initial_topic_join_timeout();
                #[cfg(test)]
                let task_guard = CountedWarmupTask::new(Arc::clone(&warmups.initial_warmup_tasks));
                let warmup_task = AbortWarmupOnDrop(tokio::spawn(async move {
                    #[cfg(test)]
                    let _task_guard = task_guard;
                    let join_deadline = tokio::time::Instant::now() + join_timeout;
                    let relay_backed = peers_use_relay(&warm_bootstrap_peers);
                    let mut attempt = 0usize;
                    loop {
                        warmups
                            .warmup_peers_once(&warm_endpoint, &warm_gossip, &warm_bootstrap_peers)
                            .await;
                        if tokio::time::Instant::now() >= join_deadline {
                            return;
                        }
                        let retry_delay = topic_warmup_retry_delay(attempt, relay_backed);
                        attempt = attempt.saturating_add(1);
                        sleep(retry_delay).await;
                    }
                }));
                let joined = timeout(join_timeout, receiver.joined())
                    .await
                    .is_ok_and(|result| result.is_ok());
                warmup_task.0.abort();
                if joined {
                    joined_task_state.store(true, Ordering::SeqCst);
                    joined_task_notify.notify_waiters();
                    *last_error_task.lock().await = None;
                    *transport_last_error.lock().await = None;
                    let current_neighbors = receiver
                        .neighbors()
                        .map(|peer| peer.to_string())
                        .collect::<BTreeSet<_>>();
                    *neighbors_task.write().await = current_neighbors;
                } else {
                    let message = "timed out waiting for initial topic join".to_string();
                    *last_error_task.lock().await = Some(message.clone());
                    *transport_last_error.lock().await =
                        Some(format!("topic join pending: {message}"));
                }
            }
            while let Some(event) = receiver.next().await {
                match event {
                    Ok(GossipEvent::Received(message)) => {
                        joined_task_state.store(true, Ordering::SeqCst);
                        joined_task_notify.notify_waiters();
                        let current_neighbors = receiver
                            .neighbors()
                            .map(|peer| peer.to_string())
                            .collect::<BTreeSet<_>>();
                        *neighbors_task.write().await = current_neighbors;
                        *last_received_at_task.lock().await = Some(Utc::now().timestamp_millis());
                        match serde_json::from_slice::<GossipHint>(&message.content) {
                            Ok(parsed) => {
                                *last_error_task.lock().await = None;
                                *transport_last_error.lock().await = None;
                                let _ = outbound.send(HintEnvelope {
                                    hint: parsed,
                                    received_at: Utc::now().timestamp_millis(),
                                    source_peer: message.delivered_from.to_string(),
                                });
                            }
                            Err(error) => {
                                // wire 非互換・不正 payload の観測点(WP-C4)。payload 本体は
                                // ログに出さない。悪性 peer によるログ洪水を避けるため warn は
                                // 初回と 100 回毎に絞り、それ以外は debug に落とす。
                                let count =
                                    invalid_hint_count_task.fetch_add(1, Ordering::SeqCst) + 1;
                                if count == 1 || count.is_multiple_of(100) {
                                    warn!(
                                        topic = %topic_name,
                                        source_peer = %message.delivered_from,
                                        payload_len = message.content.len(),
                                        invalid_hint_count = count,
                                        error = %error,
                                        "failed to decode gossip hint payload"
                                    );
                                } else {
                                    debug!(
                                        topic = %topic_name,
                                        source_peer = %message.delivered_from,
                                        payload_len = message.content.len(),
                                        invalid_hint_count = count,
                                        error = %error,
                                        "failed to decode gossip hint payload"
                                    );
                                }
                                *last_error_task.lock().await =
                                    Some("failed to decode hint payload".to_string());
                            }
                        }
                    }
                    Ok(GossipEvent::NeighborUp(peer_id)) => {
                        joined_task_state.store(true, Ordering::SeqCst);
                        joined_task_notify.notify_waiters();
                        let mut guard = neighbors_task.write().await;
                        let first_direct_peer = guard.is_empty();
                        guard.insert(peer_id.to_string());
                        if first_direct_peer {
                            info!(
                                topic = %topic_name,
                                peer_id = %peer_id,
                                "gossip topic established direct peer"
                            );
                        }
                        *last_error_task.lock().await = None;
                        *transport_last_error.lock().await = None;
                    }
                    Ok(GossipEvent::NeighborDown(peer_id)) => {
                        let mut guard = neighbors_task.write().await;
                        guard.remove(peer_id.to_string().as_str());
                    }
                    Ok(GossipEvent::Lagged) => {}
                    Err(error) => {
                        let message = format!("gossip receiver closed: {error}");
                        *last_error_task.lock().await = Some(message.clone());
                        *transport_last_error.lock().await = Some(message);
                        break;
                    }
                }
            }
        });

        subscribed.insert(topic.0.clone());
        topics.insert(
            topic.0.clone(),
            HintTopicState {
                sender: Arc::new(Mutex::new(sender)),
                broadcaster: broadcaster.clone(),
                bootstrap_peer_ids,
                neighbors,
                last_received_at,
                last_error,
                invalid_hint_count,
                closed,
                closed_notify,
                update_warmup_task: None,
                _receiver_task: task,
            },
        );

        Ok(broadcaster)
    }

    fn stream_from_sender(sender: &broadcast::Sender<HintEnvelope>) -> HintStream {
        let stream =
            BroadcastStream::new(sender.subscribe()).filter_map(|event| async move { event.ok() });
        Box::pin(stream)
    }

    async fn shutdown_hint_topics(&self) {
        let mut topics = self.topic_states.lock().await;
        let mut states = topics.drain().map(|(_, state)| state).collect::<Vec<_>>();
        for state in &mut states {
            state.closed.store(true, Ordering::Release);
            state.closed_notify.notify_waiters();
            state._receiver_task.abort();
            if let Some(update) = &state.update_warmup_task {
                update.abort();
            }
        }
        drop(topics);
        self.subscribed_topics.lock().await.clear();
        for mut state in states {
            let _ = state._receiver_task.await;
            if let Some(update) = state.update_warmup_task.take() {
                let _ = update.await;
            }
        }
    }

    pub async fn shutdown(&self) {
        self.hint_closed.store(true, Ordering::Release);
        self.offer_closed.store(true, Ordering::Release);
        self.offer_shutdown_notify.notify_waiters();
        self.shutdown_hint_topics().await;
        self.shutdown_receive_offers().await;
    }

    pub(crate) async fn hint_subscribe_hints_impl(&self, topic: &TopicId) -> Result<HintStream> {
        let hint_topic = kukuri_core::wire::hint_topic_id(topic);
        let sender = self.ensure_hint_topic(&hint_topic).await?;
        Ok(Self::stream_from_sender(&sender))
    }

    /// 対象 topic の hint parse 失敗の累計(WP-C4 の観測用)。未購読なら 0。
    #[cfg(test)]
    pub(crate) async fn invalid_hint_count(&self, topic: &TopicId) -> u64 {
        let hint_topic = kukuri_core::wire::hint_topic_id(topic);
        self.topic_states
            .lock()
            .await
            .get(hint_topic.as_str())
            .map(|state| state.invalid_hint_count.load(Ordering::SeqCst))
            .unwrap_or(0)
    }

    pub(crate) async fn hint_unsubscribe_hints_impl(&self, topic: &TopicId) -> Result<()> {
        let hint_topic = kukuri_core::wire::hint_topic_id(topic);
        self.remove_topic_state(hint_topic.as_str()).await;
        Ok(())
    }

    pub(crate) async fn hint_publish_hint_impl(
        &self,
        topic: &TopicId,
        hint: GossipHint,
    ) -> Result<()> {
        let hint_topic = kukuri_core::wire::hint_topic_id(topic);
        let _ = self.ensure_hint_topic(&hint_topic).await?;
        let states = self.topic_states.lock().await;
        let state = states
            .get(hint_topic.as_str())
            .ok_or_else(|| anyhow!("missing hint topic sender"))?;
        let sender = state.sender.lock().await;
        let payload = serde_json::to_vec(&hint)?;
        if let Err(error) = sender.broadcast(payload.into()).await {
            let message = format!("failed to broadcast gossip hint: {error}");
            *state.last_error.lock().await = Some(message.clone());
            *self.last_error.lock().await = Some(message.clone());
            return Err(anyhow!(message));
        }
        *state.last_error.lock().await = None;
        *self.last_error.lock().await = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NotifyOnDrop(Arc<Notify>);

    impl Drop for NotifyOnDrop {
        fn drop(&mut self) {
            self.0.notify_one();
        }
    }

    #[tokio::test]
    async fn unsubscribing_during_initial_join_stops_its_warmup_task() {
        let mut transport = IrohGossipTransport::bind_local().await.unwrap();
        let peer = iroh::SecretKey::from_bytes(&[47; 32]).public();
        transport
            .insert_imported_peer_addr(EndpointAddr::new(peer))
            .await;
        let topic = TopicId::new("kukuri:topic:owned-initial-warmup");
        let _stream = transport.subscribe_hints(&topic).await.unwrap();
        timeout(Duration::from_secs(2), async {
            while transport
                .topic_warmups
                .initial_warmup_tasks
                .load(Ordering::SeqCst)
                == 0
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("initial warmup task must start");
        transport.unsubscribe_hints(&topic).await.unwrap();
        timeout(Duration::from_millis(300), async {
            while transport
                .topic_warmups
                .initial_warmup_tasks
                .load(Ordering::SeqCst)
                != 0
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("unsubscription must stop the initial warmup task");
        transport.shutdown().await;
        transport._router.take().unwrap().shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn unsubscribing_stops_a_registered_peer_update_warmup() {
        let mut transport = IrohGossipTransport::bind_local().await.unwrap();
        let topic = TopicId::new("kukuri:topic:owned-update-warmup");
        let _stream = transport.subscribe_hints(&topic).await.unwrap();
        let dropped = Arc::new(Notify::new());
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let update = tokio::spawn({
            let dropped = Arc::clone(&dropped);
            async move {
                let _guard = NotifyOnDrop(dropped);
                let _ = ready_tx.send(());
                std::future::pending::<()>().await;
            }
        });
        ready_rx.await.unwrap();
        let hint_topic = kukuri_core::wire::hint_topic_id(&topic);
        transport
            .topic_states
            .lock()
            .await
            .get_mut(hint_topic.as_str())
            .unwrap()
            .update_warmup_task = Some(update);
        transport.unsubscribe_hints(&topic).await.unwrap();
        timeout(Duration::from_secs(2), dropped.notified())
            .await
            .expect("removing the topic must abort its update warmup");
        transport.shutdown().await;
        transport._router.take().unwrap().shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn hint_subscribe_waiting_for_registration_cannot_revive_after_shutdown() {
        let transport = Arc::new(IrohGossipTransport::bind_local().await.unwrap());
        let topic = TopicId::new("kukuri:topic:hint-shutdown-registration");
        let registration_guard = transport.subscribed_topics.lock().await;
        let subscribe = tokio::spawn({
            let transport = Arc::clone(&transport);
            async move { transport.subscribe_hints(&topic).await }
        });
        timeout(Duration::from_secs(2), async {
            while transport.topic_states.try_lock().is_ok() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let shutdown = tokio::spawn({
            let transport = Arc::clone(&transport);
            async move { transport.shutdown().await }
        });
        timeout(Duration::from_secs(2), async {
            while !transport.hint_closed.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        drop(registration_guard);
        assert!(subscribe.await.unwrap().is_err());
        shutdown.await.unwrap();
        assert!(transport.topic_states.lock().await.is_empty());
        let mut transport = Arc::try_unwrap(transport).ok().unwrap();
        transport._router.take().unwrap().shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancelled_hint_shutdown_aborts_all_topic_tasks_before_waiting() {
        let transport = Arc::new(IrohGossipTransport::bind_local().await.unwrap());
        let first_topic = TopicId::new("kukuri:topic:a-blocked-hint-shutdown");
        let second_topic = TopicId::new("kukuri:topic:b-pending-hint-shutdown");
        let _first_stream = transport.subscribe_hints(&first_topic).await.unwrap();
        let _second_stream = transport.subscribe_hints(&second_topic).await.unwrap();
        let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
        let blocked = tokio::task::spawn_blocking(move || {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let dropped = Arc::new(Notify::new());
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let pending = tokio::spawn({
            let dropped = Arc::clone(&dropped);
            async move {
                let _guard = NotifyOnDrop(dropped);
                let _ = ready_tx.send(());
                std::future::pending::<()>().await;
            }
        });
        ready_rx.await.unwrap();
        let first_key = kukuri_core::wire::hint_topic_id(&first_topic);
        let second_key = kukuri_core::wire::hint_topic_id(&second_topic);
        {
            let mut states = transport.topic_states.lock().await;
            let first = states.get_mut(first_key.as_str()).unwrap();
            let old = std::mem::replace(&mut first._receiver_task, blocked);
            old.abort();
            let second = states.get_mut(second_key.as_str()).unwrap();
            let old = std::mem::replace(&mut second._receiver_task, pending);
            old.abort();
        }
        let shutdown = tokio::spawn({
            let transport = Arc::clone(&transport);
            async move { transport.shutdown().await }
        });
        timeout(Duration::from_secs(2), async {
            while !transport.topic_states.lock().await.is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        shutdown.abort();
        let _ = shutdown.await;
        release_tx.send(()).unwrap();
        timeout(Duration::from_secs(2), dropped.notified())
            .await
            .expect("all topic tasks must be aborted before shutdown awaits one");
        transport.shutdown().await;
        let mut transport = Arc::try_unwrap(transport).ok().unwrap();
        transport._router.take().unwrap().shutdown().await.unwrap();
    }

    #[test]
    fn warmup_samples_a_moving_four_peer_window_from_large_history() {
        for history in [100_usize, 1_000] {
            let peers = (0..history)
                .map(|index| {
                    let mut secret = [0_u8; 32];
                    secret[..8].copy_from_slice(&(index as u64 + 1).to_be_bytes());
                    EndpointAddr::new(iroh::SecretKey::from_bytes(&secret).public())
                })
                .collect::<Vec<_>>();
            let coordinator = TopicWarmupCoordinator::default();
            let first = coordinator.warmup_window(&peers);
            let second = coordinator.warmup_window(&peers);
            assert_eq!(first.len(), 4, "history={history}");
            assert_eq!(second.len(), 4, "history={history}");
            assert_eq!(first[0].id, peers[0].id);
            assert_eq!(second[0].id, peers[4].id);
            assert_eq!(second[3].id, peers[7].id);
        }
    }

    #[tokio::test]
    async fn warmup_does_not_queue_peer_state_when_shared_dial_slots_are_full() {
        let endpoint = Endpoint::bind(presets::Minimal).await.unwrap();
        let gossip = Gossip::builder().spawn(endpoint.clone());
        let coordinator = TopicWarmupCoordinator::default();
        let first = coordinator.permits.try_acquire().unwrap();
        let second = coordinator.permits.try_acquire().unwrap();
        let peer = iroh::SecretKey::from_bytes(&[3; 32]).public();
        timeout(
            Duration::from_millis(100),
            coordinator.warmup_peer(endpoint.clone(), gossip.clone(), EndpointAddr::new(peer)),
        )
        .await
        .expect("full dial capacity must defer without a waiting future");
        assert!(coordinator.in_flight_peers.read().unwrap().is_empty());
        drop(first);
        drop(second);
        let _ = gossip.shutdown().await;
        endpoint.close().await;
    }

    #[tokio::test]
    async fn active_other_protocol_does_not_suppress_gossip_warmup() {
        const OTHER_ALPN: &[u8] = b"kukuri-test-other-protocol";
        let local = Endpoint::bind(presets::Minimal)
            .await
            .expect("local endpoint");
        let remote = Endpoint::builder(presets::Minimal)
            .alpns(vec![OTHER_ALPN.to_vec(), GOSSIP_ALPN.to_vec()])
            .bind()
            .await
            .expect("remote endpoint");
        let (outbound, inbound) = timeout(Duration::from_secs(5), async {
            tokio::join!(local.connect(remote.addr(), OTHER_ALPN), async {
                remote.accept().await.expect("other incoming").await
            })
        })
        .await
        .expect("other protocol connection deadline");
        let outbound = outbound.expect("other outbound");
        let inbound = inbound.expect("other inbound");
        assert!(local.remote_info(remote.id()).await.is_some_and(|info| {
            info.addrs()
                .any(|addr| matches!(addr.usage(), TransportAddrUsage::Active))
        }));
        assert_eq!(outbound.alpn(), OTHER_ALPN);
        assert_eq!(inbound.alpn(), OTHER_ALPN);

        let gossip = Gossip::builder().spawn(local.clone());
        let warmup = tokio::spawn({
            let local = local.clone();
            let gossip = gossip.clone();
            let peer = remote.addr();
            async move {
                TopicWarmupCoordinator::default()
                    .warmup_peer(local, gossip, peer)
                    .await;
            }
        });
        let incoming = timeout(Duration::from_secs(3), async {
            remote.accept().await.expect("gossip incoming").await
        })
        .await;
        warmup.abort();
        let _ = warmup.await;
        gossip.shutdown().await.expect("gossip shutdown");
        local.close().await;
        remote.close().await;
        let connection = incoming
            .expect("an active other protocol must not suppress gossip dialing")
            .expect("gossip connection");
        assert_eq!(connection.alpn(), GOSSIP_ALPN);
    }

    // gossip topic id 派生の golden(WP-S3 T4)。blake3(topic 文字列)が
    // on-wire の gossip 識別子そのもの。変更はネットワーク分断になる。
    #[test]
    fn topic_to_gossip_id_matches_golden() {
        let id = topic_to_gossip_id(&kukuri_core::TopicId::new("kukuri:topic:golden"));
        let expected = blake3::Hash::from_hex(
            "65994b46e778ead0264f20707efc571bfc4bdf510f97add9ebd81c8ad507dc80",
        )
        .expect("expected hex parses");
        assert_eq!(id.as_bytes(), expected.as_bytes());
    }

    #[test]
    fn topic_warmup_retry_delay_backs_off_and_caps() {
        assert_eq!(
            topic_warmup_retry_delay(0, false),
            Duration::from_millis(250)
        );
        assert_eq!(
            topic_warmup_retry_delay(1, false),
            Duration::from_millis(500)
        );
        assert_eq!(topic_warmup_retry_delay(2, false), Duration::from_secs(1));
        assert_eq!(topic_warmup_retry_delay(3, false), Duration::from_secs(2));
        assert_eq!(topic_warmup_retry_delay(4, false), Duration::from_secs(5));
        assert_eq!(topic_warmup_retry_delay(12, false), Duration::from_secs(5));
    }

    #[test]
    fn relay_topic_warmup_retry_delay_backs_off_more_slowly() {
        assert_eq!(topic_warmup_retry_delay(0, true), Duration::from_secs(1));
        assert_eq!(topic_warmup_retry_delay(1, true), Duration::from_secs(2));
        assert_eq!(topic_warmup_retry_delay(2, true), Duration::from_secs(4));
        assert_eq!(topic_warmup_retry_delay(3, true), Duration::from_secs(8));
        assert_eq!(topic_warmup_retry_delay(4, true), Duration::from_secs(10));
        assert_eq!(topic_warmup_retry_delay(12, true), Duration::from_secs(10));
    }

    #[test]
    fn warmup_coordinator_coalesces_same_peer_dials() {
        let coordinator = TopicWarmupCoordinator::default();

        assert!(coordinator.try_mark_peer_in_flight_for_test("peer"));
        assert!(!coordinator.try_mark_peer_in_flight_for_test("peer"));

        coordinator.clear_in_flight_for_test("peer");
        assert!(coordinator.try_mark_peer_in_flight_for_test("peer"));
    }

    #[test]
    fn warmup_in_flight_guard_clears_on_drop() {
        let coordinator = TopicWarmupCoordinator::default();
        let guard = coordinator
            .try_mark_peer_in_flight("peer".to_string())
            .expect("first warmup marks peer");

        assert!(!coordinator.try_mark_peer_in_flight_for_test("peer"));
        drop(guard);
        assert!(coordinator.try_mark_peer_in_flight_for_test("peer"));
    }
}
