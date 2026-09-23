//! Bounded, authenticated destination lookup for account receive offers.
//! A peer address is only a candidate until its live signed binding is checked.

use super::*;
use std::ops::Bound::{Excluded, Unbounded};
use tokio::time::Instant;

use crate::receive_binding::fetch_receive_endpoint_binding;
use kukuri_core::receive_route_for_account;

const MAX_DESTINATION_ACCOUNTS: usize = 1_024;
const CANDIDATES_PER_LOOKUP: usize = 4;
const MAX_SELECTION_STEPS: usize = 12;
const BINDING_PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_CACHED_BINDING_MS: i64 = 10_000;

#[derive(Default)]
pub(super) struct DestinationWindow {
    entries: HashMap<Pubkey, DestinationEntry>,
    tick: u64,
}

struct DestinationEntry {
    last_used: u64,
    revision: u64,
    source: usize,
    cursors: [Option<String>; 3],
    verified: Option<CachedDestination>,
}

struct CachedDestination {
    address: EndpointAddr,
    expires_at_ms: i64,
    expires_at: Instant,
}

impl DestinationWindow {
    fn touch(&mut self, recipient: &Pubkey) -> &mut DestinationEntry {
        self.tick = self.tick.wrapping_add(1);
        if !self.entries.contains_key(recipient)
            && self.entries.len() == MAX_DESTINATION_ACCOUNTS
            && let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(account, _)| account.clone())
        {
            self.entries.remove(&oldest);
        }
        let tick = self.tick;
        let entry = self
            .entries
            .entry(recipient.clone())
            .or_insert_with(|| DestinationEntry {
                last_used: tick,
                revision: tick,
                source: 0,
                cursors: Default::default(),
                verified: None,
            });
        entry.last_used = tick;
        entry
    }

    fn cached(&mut self, recipient: &Pubkey, now_ms: i64) -> Option<EndpointAddr> {
        let entry = self.touch(recipient);
        if let Some(cached) = &entry.verified
            && cached.expires_at_ms > now_ms
            && cached.expires_at > Instant::now()
        {
            return Some(cached.address.clone());
        }
        entry.verified = None;
        None
    }

    fn select(
        &mut self,
        recipient: &Pubkey,
        sources: [&BTreeMap<String, EndpointAddr>; 3],
    ) -> (Vec<EndpointAddr>, u64) {
        let entry = self.touch(recipient);
        let mut selected = Vec::with_capacity(CANDIDATES_PER_LOOKUP);
        let mut seen = BTreeSet::new();
        for _ in 0..MAX_SELECTION_STEPS {
            if selected.len() == CANDIDATES_PER_LOOKUP {
                break;
            }
            let source = entry.source;
            entry.source = (entry.source + 1) % sources.len();
            if let Some(candidate) = next_peer(sources[source], &mut entry.cursors[source])
                && seen.insert(candidate.id)
            {
                selected.push(candidate);
            }
        }
        (selected, entry.revision)
    }

    fn store_verified(
        &mut self,
        recipient: &Pubkey,
        revision: u64,
        address: EndpointAddr,
        expires_at_ms: i64,
        expires_at: Instant,
    ) -> bool {
        let Some(entry) = self.entries.get_mut(recipient) else {
            return false;
        };
        if entry.revision != revision {
            return false;
        }
        entry.verified = Some(CachedDestination {
            address,
            expires_at_ms,
            expires_at,
        });
        true
    }

    fn invalidate(&mut self, recipient: &Pubkey, endpoint_id: &str) {
        let tick = self.tick.wrapping_add(1);
        let Some(entry) = self.entries.get_mut(recipient) else {
            return;
        };
        if entry
            .verified
            .as_ref()
            .is_some_and(|cached| cached.address.id.to_string() != endpoint_id)
        {
            return;
        }
        self.tick = tick;
        entry.revision = tick;
        entry.verified = None;
    }
}

fn next_peer(
    peers: &BTreeMap<String, EndpointAddr>,
    cursor: &mut Option<String>,
) -> Option<EndpointAddr> {
    let next = cursor
        .as_ref()
        .and_then(|after| peers.range((Excluded(after.clone()), Unbounded)).next())
        .or_else(|| peers.iter().next());
    let (key, address) = next?;
    *cursor = Some(key.clone());
    Some(address.clone())
}

impl IrohGossipTransport {
    pub(super) async fn resolve_receive_destination_impl(
        &self,
        recipient: &Pubkey,
    ) -> Result<Option<EndpointAddr>> {
        receive_route_for_account(recipient)?;
        anyhow::ensure!(
            !self.offer_closed.load(Ordering::Acquire),
            "account receive offer transport is closed"
        );
        let now_ms = Utc::now().timestamp_millis();
        if let Some(cached) = self
            .receive_destinations
            .lock()
            .await
            .cached(recipient, now_ms)
        {
            return Ok(Some(cached));
        }
        // No queue of recipient lookups grows behind a busy transport.
        let Ok(_permit) = self.receive_destination_probes.try_acquire() else {
            return Ok(None);
        };
        let configured = self.configured_seed_peers.lock().await;
        let bootstrap = self.bootstrap_seed_peers.lock().await;
        let imported = self.imported_peers.lock().await;
        let (candidates, revision) = self
            .receive_destinations
            .lock()
            .await
            .select(recipient, [&configured, &bootstrap, &imported]);
        drop((configured, bootstrap, imported));
        for candidate in candidates {
            let deadline = Instant::now() + BINDING_PROBE_TIMEOUT;
            let Ok(binding) = fetch_receive_endpoint_binding(
                &self.endpoint,
                candidate.clone(),
                recipient,
                deadline,
            )
            .await
            else {
                continue;
            };
            if self.offer_closed.load(Ordering::Acquire) {
                return Ok(None);
            }
            let now_ms = Utc::now().timestamp_millis();
            let expires_at_ms = binding.expires_at_ms().min(now_ms + MAX_CACHED_BINDING_MS);
            if expires_at_ms <= now_ms {
                continue;
            }
            let stored = self.receive_destinations.lock().await.store_verified(
                recipient,
                revision,
                candidate.clone(),
                expires_at_ms,
                Instant::now() + Duration::from_millis((expires_at_ms - now_ms) as u64),
            );
            return Ok(stored.then_some(candidate));
        }
        Ok(None)
    }

    pub(super) async fn invalidate_receive_destination_impl(
        &self,
        recipient: &Pubkey,
        endpoint_id: &str,
    ) {
        self.receive_destinations
            .lock()
            .await
            .invalidate(recipient, endpoint_id);
    }
}

#[cfg(test)]
#[path = "tests/receive_destination.rs"]
mod tests;
