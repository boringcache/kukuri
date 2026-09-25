use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use kukuri_core::{
    RECEIVE_PAYLOAD_MAX_BYTES, ReceiveOfferReferenceV1, ReceiveOfferScopeV1, VerifiedReceiveOffer,
    seal_receive_offer,
};
use kukuri_transport::EndpointAddr;
use serde::Deserialize;

use super::notifications_support::{
    notification_candidate_from_verified_follow, notification_candidate_from_verified_post,
    pubkey_mentions,
};
use super::post_integrity::VerifiedPost;
use super::*;

const PUBLIC_OFFER_TASKS: usize = 4;
const PUBLIC_OFFER_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum PublicNotificationSource {
    Post {
        replica: ReplicaId,
        envelope: KukuriEnvelope,
        content: String,
        reply_target: Option<KukuriEnvelope>,
    },
    Follow {
        envelope: KukuriEnvelope,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublicNotificationManifest {
    version: u8,
    source: PublicNotificationSource,
}

pub(crate) fn encode_public_notification_manifest(
    source: PublicNotificationSource,
) -> Result<Vec<u8>> {
    let payload = serde_json::to_vec(&PublicNotificationManifest { version: 1, source })?;
    anyhow::ensure!(
        payload.len() <= RECEIVE_PAYLOAD_MAX_BYTES,
        "public notification manifest too large"
    );
    Ok(payload)
}

impl AppService {
    pub(crate) async fn queue_public_post_offer(
        &self,
        replica: &ReplicaId,
        envelope: &KukuriEnvelope,
        content: &str,
        parent: Option<&KukuriEnvelope>,
    ) {
        let reply_target = match parent {
            Some(parent) => self
                .resolve_signed_post_envelope(&parent.id)
                .await
                .ok()
                .flatten(),
            None => None,
        };
        let recipients = public_post_notification_recipients(content, reply_target.as_ref(), None);
        self.queue_public_notification_offer(
            PublicNotificationSource::Post {
                replica: replica.clone(),
                envelope: envelope.clone(),
                content: content.to_string(),
                reply_target,
            },
            recipients,
        )
        .await;
    }

    pub(crate) async fn queue_public_repost_offer(
        &self,
        topic_id: &str,
        envelope: &KukuriEnvelope,
        source: Option<&RepostSourceSnapshotV1>,
        commentary: Option<&str>,
    ) {
        let content = commentary.unwrap_or_default();
        let recipients = public_post_notification_recipients(content, None, source);
        self.queue_public_notification_offer(
            PublicNotificationSource::Post {
                replica: topic_replica_id(topic_id),
                envelope: envelope.clone(),
                content: content.to_string(),
                reply_target: None,
            },
            recipients,
        )
        .await;
    }

    pub(crate) async fn queue_public_notification_offer(
        &self,
        source: PublicNotificationSource,
        mut recipients: BTreeSet<String>,
    ) {
        recipients.remove(self.current_author_pubkey().as_str());
        if recipients.is_empty() {
            return;
        }
        let payload = match encode_public_notification_manifest(source) {
            Ok(payload) => payload,
            _ => return,
        };
        let mut tasks = self
            .subscription_registry
            .public_notification_offer_tasks
            .lock()
            .await;
        tasks.retain(|task| !task.is_finished());
        if tasks.len() >= PUBLIC_OFFER_TASKS {
            return;
        }
        let services = self.services.clone();
        let closed = Arc::clone(&self.subscription_registry.account_receive_offer_closed);
        tasks.push(AbortOnDropTask::new(tokio::spawn(async move {
            if let Err(error) =
                publish_public_notification_offers(&services, &closed, payload, recipients).await
            {
                tracing::debug!(%error, "public notification offers deferred");
            }
        })));
    }

    pub(crate) async fn ingest_public_notification_offer(
        services: &ServiceHandles,
        offer: &VerifiedReceiveOffer,
    ) -> Result<bool> {
        let provider = EndpointAddr::new(offer.reference().provider_endpoint_id.parse()?);
        let payload = services
            .blob_service
            .fetch_verified_receive_offer_payload(offer, provider)
            .await?;
        let manifest: PublicNotificationManifest = serde_json::from_slice(&payload)?;
        anyhow::ensure!(
            manifest.version == 1,
            "unsupported public notification manifest"
        );
        let local = services.keys.public_key_hex();
        let candidate = match manifest.source {
            PublicNotificationSource::Post {
                replica,
                envelope,
                content,
                reply_target,
            } => {
                anyhow::ensure!(
                    envelope.pubkey == *offer.sender(),
                    "public offer sender mismatch"
                );
                let post = VerifiedPost::verify_local(envelope, &replica).map_err(|reason| {
                    anyhow::anyhow!("invalid public notification post: {reason:?}")
                })?;
                let header = post.header();
                let content_matches = match &header.payload_ref {
                    PayloadRef::BlobText { hash, bytes, .. } => {
                        content.len() as u64 == *bytes
                            && blake3::hash(content.as_bytes()).to_hex().as_str() == hash.as_str()
                    }
                    PayloadRef::InlineText { text } => content == *text,
                };
                anyhow::ensure!(content_matches, "public notification content mismatch");
                let reply_to_local = if let (Some(parent_id), Some(parent)) =
                    (&header.reply_to, reply_target)
                {
                    parent.verify()?;
                    let parent_post = parent.to_post_object()?.context("invalid reply target")?;
                    anyhow::ensure!(parent.id == *parent_id, "reply target id mismatch");
                    parent.pubkey.as_str() == local
                        && parent_post.topic_id == header.topic_id
                        && parent_post.visibility == ObjectVisibility::Public
                } else {
                    false
                };
                notification_candidate_from_verified_post(
                    &local,
                    &post,
                    Some(content),
                    reply_to_local,
                )
            }
            PublicNotificationSource::Follow { envelope } => {
                envelope.verify()?;
                anyhow::ensure!(
                    envelope.pubkey == *offer.sender(),
                    "follow offer sender mismatch"
                );
                let edge = parse_follow_edge(&envelope)?.context("invalid follow offer")?;
                notification_candidate_from_verified_follow(
                    &local,
                    &author_replica_id(offer.sender().as_str()),
                    &edge,
                )
            }
        };
        let Some(candidate) = candidate else {
            return Ok(false);
        };
        Self::put_notification_candidate(services.projection_store.as_ref(), &local, candidate)
            .await
    }
}

pub(crate) fn public_post_notification_recipients(
    content: &str,
    reply_target: Option<&KukuriEnvelope>,
    repost_of: Option<&RepostSourceSnapshotV1>,
) -> BTreeSet<String> {
    let mut recipients = pubkey_mentions(content)
        .map(str::to_ascii_lowercase)
        .collect::<BTreeSet<_>>();
    if let Some(parent) = reply_target {
        recipients.insert(parent.pubkey.as_str().to_string());
    }
    if let Some(source) = repost_of {
        recipients.insert(source.source_author_pubkey.as_str().to_string());
    }
    recipients
}

async fn publish_public_notification_offers(
    services: &ServiceHandles,
    closed: &AtomicBool,
    payload: Vec<u8>,
    recipients: BTreeSet<String>,
) -> Result<()> {
    if closed.load(Ordering::Acquire) {
        return Ok(());
    }
    let stored = services
        .blob_service
        .put_remote_blob(payload, "application/vnd.kukuri.public-notification+json")
        .await?;
    let provider_endpoint_id = services.transport.discovery().await?.local_endpoint_id;
    for recipient in recipients {
        if closed.load(Ordering::Acquire) {
            break;
        }
        if let Err(error) =
            publish_public_notification_offer(services, &recipient, &provider_endpoint_id, &stored)
                .await
        {
            tracing::debug!(%error, recipient, "public notification offer deferred");
        }
    }
    Ok(())
}

async fn publish_public_notification_offer(
    services: &ServiceHandles,
    recipient: &str,
    provider_endpoint_id: &str,
    stored: &StoredBlob,
) -> Result<()> {
    let recipient = Pubkey::from(recipient);
    let Some(destination) = services
        .hint_transport
        .resolve_receive_destination(&recipient)
        .await?
    else {
        return Ok(());
    };
    let now = Utc::now().timestamp_millis();
    let offer = seal_receive_offer(
        services.keys.as_ref(),
        &recipient,
        ReceiveOfferReferenceV1 {
            provider_endpoint_id: provider_endpoint_id.to_string(),
            payload_hash: stored.hash.clone(),
            payload_bytes: u32::try_from(stored.bytes)?,
            scope: ReceiveOfferScopeV1::PublicSource,
        },
        now,
        now + 60_000,
    )?;
    tokio::time::timeout(
        PUBLIC_OFFER_TIMEOUT,
        services
            .hint_transport
            .publish_receive_offer(&recipient, destination, offer),
    )
    .await
    .context("public notification offer timed out")??;
    Ok(())
}
