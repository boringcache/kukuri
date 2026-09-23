use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use anyhow::{Result, bail};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};

use crate::config::TOPIC_RENDEZVOUS_TTL_SECONDS;
use kukuri_cn_protocol::models::CommunityNodeSeedPeer;

const RENDEZVOUS_BUCKET_SECONDS: u64 = 15;
const RENDEZVOUS_BUCKETS_TO_READ: u64 = 4;
const RENDEZVOUS_CANDIDATE_LIMIT: usize = 8;
const RENDEZVOUS_SAMPLE_PER_BUCKET: usize = 16;
const RENDEZVOUS_REDIS_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug)]
pub struct TopicRendezvousStore {
    client: redis::Client,
    key_prefix: String,
    ttl_seconds: u64,
}

use kukuri_cn_protocol::{
    TopicRendezvousCandidate, TopicRendezvousHeartbeat, TopicRendezvousHeartbeatResponse,
    TopicRendezvousTopicResponse,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredRendezvousPeer {
    endpoint_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    addr_hint: Option<String>,
}

impl TopicRendezvousStore {
    pub fn new(redis_url: &str, key_prefix: impl Into<String>) -> Result<Self> {
        let client = redis::Client::open(redis_url)?;
        let key_prefix = normalize_key_prefix(key_prefix.into().as_str())?;
        Ok(Self {
            client,
            key_prefix,
            ttl_seconds: TOPIC_RENDEZVOUS_TTL_SECONDS,
        })
    }

    pub async fn heartbeat(
        &self,
        heartbeat: TopicRendezvousHeartbeat,
        relay_urls: &[String],
    ) -> Result<TopicRendezvousHeartbeatResponse> {
        self.heartbeat_with_time(heartbeat, relay_urls, None).await
    }

    #[cfg(test)]
    async fn heartbeat_at(
        &self,
        heartbeat: TopicRendezvousHeartbeat,
        relay_urls: &[String],
        now_seconds: u64,
    ) -> Result<TopicRendezvousHeartbeatResponse> {
        self.heartbeat_with_time(heartbeat, relay_urls, Some(now_seconds))
            .await
    }

    async fn heartbeat_with_time(
        &self,
        heartbeat: TopicRendezvousHeartbeat,
        relay_urls: &[String],
        now_override: Option<u64>,
    ) -> Result<TopicRendezvousHeartbeatResponse> {
        let endpoint = CommunityNodeSeedPeer::new(heartbeat.endpoint_id, heartbeat.addr_hint)?;
        let joins = normalize_topic_keys(heartbeat.joins)?;
        let refreshes = normalize_topic_keys(heartbeat.refreshes)?;
        let leaves = normalize_topic_keys(heartbeat.leaves)?;
        let mut active_topics = BTreeSet::new();
        active_topics.extend(joins.iter().cloned());
        active_topics.extend(refreshes.iter().cloned());

        let mut connection = self.connection().await?;
        // A shared Valkey clock keeps bucket selection consistent across CN API replicas.
        let now_seconds = match now_override {
            Some(now) => now,
            None => {
                let (seconds, _micros): (u64, u64) =
                    redis::cmd("TIME").query_async(&mut connection).await?;
                seconds
            }
        };

        let bucket = now_seconds / RENDEZVOUS_BUCKET_SECONDS;
        let expires_at = now_seconds
            .checked_add(self.ttl_seconds)
            .ok_or_else(|| anyhow::anyhow!("rendezvous expiry overflow"))?;
        let stored_peer = serde_json::to_string(&StoredRendezvousPeer {
            endpoint_id: endpoint.endpoint_id.clone(),
            addr_hint: endpoint.addr_hint.clone(),
        })?;

        if !active_topics.is_empty() {
            let _: () = connection
                .set_ex(
                    self.peer_key(endpoint.endpoint_id.as_str()),
                    stored_peer,
                    self.ttl_seconds,
                )
                .await?;
        }

        for topic_key in &active_topics {
            let key = self.topic_bucket_key(topic_key, bucket);
            let _: () = connection
                .set_ex(
                    self.membership_key(topic_key, endpoint.endpoint_id.as_str()),
                    expires_at,
                    self.ttl_seconds,
                )
                .await?;
            let bucket_update = bucket_update(
                key.as_str(),
                endpoint.endpoint_id.as_str(),
                (self.ttl_seconds + RENDEZVOUS_BUCKET_SECONDS) as i64,
            );
            let _: () = bucket_update.query_async(&mut connection).await?;
        }

        for topic_key in &leaves {
            let _: usize = connection
                .del(self.membership_key(topic_key, endpoint.endpoint_id.as_str()))
                .await?;
            for old_bucket in recent_buckets(bucket) {
                let _: usize = connection
                    .srem(
                        self.topic_bucket_key(topic_key, old_bucket),
                        endpoint.endpoint_id.as_str(),
                    )
                    .await?;
            }
        }

        let mut topics = Vec::with_capacity(active_topics.len());
        for topic_key in active_topics {
            let mut seen = BTreeSet::new();
            let mut sampled = Vec::new();
            for old_bucket in recent_buckets(bucket) {
                let ids: Vec<String> = redis::cmd("SRANDMEMBER")
                    .arg(self.topic_bucket_key(topic_key.as_str(), old_bucket))
                    .arg(RENDEZVOUS_SAMPLE_PER_BUCKET)
                    .query_async(&mut connection)
                    .await?;
                for id in ids {
                    if id != endpoint.endpoint_id && seen.insert(id.clone()) {
                        sampled.push(id);
                    }
                }
            }

            let mut peers = Vec::new();
            if !sampled.is_empty() {
                let membership_keys = sampled
                    .iter()
                    .map(|id| self.membership_key(topic_key.as_str(), id))
                    .collect::<Vec<_>>();
                let expiry: Vec<Option<u64>> = redis::cmd("MGET")
                    .arg(&membership_keys)
                    .query_async(&mut connection)
                    .await?;
                let live_ids = sampled
                    .into_iter()
                    .zip(expiry)
                    .filter_map(|(id, expiry)| expiry.filter(|at| *at > now_seconds).map(|_| id))
                    .collect::<Vec<_>>();
                if !live_ids.is_empty() {
                    let peer_keys = live_ids
                        .iter()
                        .map(|id| self.peer_key(id))
                        .collect::<Vec<_>>();
                    let peer_jsons: Vec<Option<String>> = redis::cmd("MGET")
                        .arg(&peer_keys)
                        .query_async(&mut connection)
                        .await?;
                    for (id, peer_json) in live_ids.into_iter().zip(peer_jsons) {
                        let Some(peer_json) = peer_json else { continue };
                        let peer: StoredRendezvousPeer = serde_json::from_str(&peer_json)?;
                        if peer.endpoint_id != id {
                            continue;
                        }
                        peers.push(TopicRendezvousCandidate {
                            endpoint_id: peer.endpoint_id,
                            addr_hint: peer.addr_hint,
                            relay_urls: relay_urls.to_vec(),
                        });
                        if peers.len() == RENDEZVOUS_CANDIDATE_LIMIT {
                            break;
                        }
                    }
                }
            }
            peers.sort_by(|left, right| left.endpoint_id.cmp(&right.endpoint_id));
            topics.push(TopicRendezvousTopicResponse { topic_key, peers });
        }

        Ok(TopicRendezvousHeartbeatResponse {
            expires_in_seconds: self.ttl_seconds,
            topics,
        })
    }

    fn topic_bucket_key(&self, topic_key: &str, bucket: u64) -> String {
        format!("{}:topic-window:{topic_key}:{bucket}", self.key_prefix)
    }

    fn membership_key(&self, topic_key: &str, endpoint_id: &str) -> String {
        format!("{}:topic-peer:{topic_key}:{endpoint_id}", self.key_prefix)
    }

    fn peer_key(&self, endpoint_id: &str) -> String {
        format!("{}:peer:{endpoint_id}", self.key_prefix)
    }

    async fn connection(&self) -> Result<redis::aio::MultiplexedConnection> {
        let config = redis::AsyncConnectionConfig::new()
            .set_connection_timeout(Some(RENDEZVOUS_REDIS_TIMEOUT))
            .set_response_timeout(Some(RENDEZVOUS_REDIS_TIMEOUT));
        Ok(self
            .client
            .get_multiplexed_async_connection_with_config(&config)
            .await?)
    }
}

fn normalize_key_prefix(value: &str) -> Result<String> {
    let trimmed = value.trim().trim_end_matches(':');
    if trimmed.is_empty() {
        bail!("topic rendezvous key prefix must not be empty");
    }
    Ok(trimmed.to_string())
}

fn normalize_topic_keys(values: Vec<String>) -> Result<Vec<String>> {
    let mut deduped = BTreeMap::new();
    for value in values {
        let normalized = normalize_topic_key(value.as_str())?;
        deduped.insert(normalized.clone(), normalized);
    }
    Ok(deduped.into_values().collect())
}

fn normalize_topic_key(value: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.len() != 64 || !trimmed.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("topic rendezvous key must be a 64-character opaque hex value");
    }
    Ok(trimmed.to_ascii_lowercase())
}

fn recent_buckets(current: u64) -> impl Iterator<Item = u64> {
    (0..RENDEZVOUS_BUCKETS_TO_READ).filter_map(move |offset| current.checked_sub(offset))
}

fn bucket_update(key: &str, endpoint_id: &str, ttl_seconds: i64) -> redis::Pipeline {
    let mut update = redis::pipe();
    update
        .atomic()
        .cmd("SADD")
        .arg(key)
        .arg(endpoint_id)
        .ignore()
        .cmd("EXPIRE")
        .arg(key)
        .arg(ttl_seconds)
        .ignore();
    update
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    static REDIS_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn request(endpoint_id: &str, topic: &str) -> TopicRendezvousHeartbeat {
        TopicRendezvousHeartbeat {
            endpoint_id: endpoint_id.to_string(),
            addr_hint: None,
            joins: vec![topic.to_string()],
            refreshes: Vec::new(),
            leaves: Vec::new(),
        }
    }

    fn test_store(nonce: u128) -> Result<TopicRendezvousStore> {
        let prefix = format!("cn:test:1221:rendezvous:{}:{nonce}", std::process::id());
        let redis_url = std::env::var("COMMUNITY_NODE_RENDEZVOUS_REDIS_URL")
            .unwrap_or_else(|_| "redis://127.0.0.1:16379/".to_string());
        TopicRendezvousStore::new(redis_url.as_str(), prefix)
    }

    #[test]
    fn bucket_insert_and_ttl_are_one_valkey_transaction() {
        let update = bucket_update("bucket", "peer-a", 60);
        assert!(update.is_transaction());
        let packed = String::from_utf8(update.get_packed_pipeline()).unwrap();
        let multi = packed.find("MULTI").unwrap();
        let add = packed.find("SADD").unwrap();
        let expiry = packed.find("EXPIRE").unwrap();
        let exec = packed.find("EXEC").unwrap();
        assert!(multi < add && add < expiry && expiry < exec);
    }

    #[tokio::test]
    async fn invalid_topic_is_rejected_before_valkey_io() -> Result<()> {
        let store = TopicRendezvousStore::new(
            "redis://127.0.0.1:1/",
            "cn:test:1221:no-network-on-invalid-topic",
        )?;
        let error = store
            .heartbeat(request("peer-a", "private/raw-topic-id"), &[])
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("64-character opaque hex"),
            "{error:#}"
        );
        Ok(())
    }

    #[tokio::test]
    async fn rendezvous_candidates_do_not_grow_with_topic_membership() -> Result<()> {
        let _redis = REDIS_TEST_LOCK.lock().await;
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let store = test_store(nonce)?;
        let topic = "a".repeat(64);
        for index in 0..65 {
            store
                .heartbeat(request(format!("peer-{index}").as_str(), &topic), &[])
                .await?;
        }
        let response = store.heartbeat(request("requester", &topic), &[]).await?;
        let peers = &response.topics[0].peers;
        assert!(
            peers.len() <= 8,
            "candidate count followed topic size: {}",
            peers.len()
        );
        assert_eq!(
            peers.len(),
            8,
            "healthy participants should fill the candidate window"
        );
        assert!(peers.iter().all(|peer| peer.endpoint_id != "requester"));
        Ok(())
    }

    #[tokio::test]
    async fn another_topic_cannot_extend_an_expired_membership() -> Result<()> {
        let _redis = REDIS_TEST_LOCK.lock().await;
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let store = test_store(nonce)?;
        let older_topic = "b".repeat(64);
        let current_topic = "c".repeat(64);
        store
            .heartbeat_at(request("peer-a", &older_topic), &[], 1_000)
            .await?;
        store
            .heartbeat_at(request("peer-a", &current_topic), &[], 1_030)
            .await?;

        let expired = store
            .heartbeat_at(request("requester-old", &older_topic), &[], 1_046)
            .await?;
        assert!(expired.topics[0].peers.is_empty());
        let current = store
            .heartbeat_at(request("requester-current", &current_topic), &[], 1_046)
            .await?;
        assert_eq!(current.topics[0].peers.len(), 1);
        assert_eq!(current.topics[0].peers[0].endpoint_id, "peer-a");

        store
            .heartbeat_at(
                TopicRendezvousHeartbeat {
                    endpoint_id: "peer-a".into(),
                    addr_hint: None,
                    joins: Vec::new(),
                    refreshes: Vec::new(),
                    leaves: vec![current_topic.clone()],
                },
                &[],
                1_047,
            )
            .await?;
        let left = store
            .heartbeat_at(request("requester-after-leave", &current_topic), &[], 1_048)
            .await?;
        assert!(
            left.topics[0]
                .peers
                .iter()
                .all(|peer| peer.endpoint_id != "peer-a")
        );
        Ok(())
    }

    #[tokio::test]
    async fn rendezvous_bucket_and_membership_have_finite_ttls() -> Result<()> {
        let _redis = REDIS_TEST_LOCK.lock().await;
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let store = test_store(nonce)?;
        let topic = "d".repeat(64);
        store
            .heartbeat_at(request("peer-a", &topic), &[], 1_000)
            .await?;
        let mut connection = store.connection().await?;
        let bucket_ttl: i64 = connection
            .ttl(store.topic_bucket_key(&topic, 1_000 / RENDEZVOUS_BUCKET_SECONDS))
            .await?;
        let membership_ttl: i64 = connection
            .ttl(store.membership_key(&topic, "peer-a"))
            .await?;
        assert!((1..=60).contains(&bucket_ttl));
        assert!((1..=45).contains(&membership_ttl));
        Ok(())
    }
}
