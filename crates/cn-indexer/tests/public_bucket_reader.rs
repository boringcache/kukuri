use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use futures_util::StreamExt;
use kukuri_cn_core::{
    ChannelSecretCipher, IndexScopeKind, MemoryIndexEntryStore, TestDatabase, add_supported_topic,
    connect_postgres, initialize_database, mark_index_demand,
};
use kukuri_cn_indexer::ingest::IngestPipeline;
use kukuri_cn_indexer::participant::IndexerParticipant;
use kukuri_cn_indexer::projection::{IndexProjection, MemoryIndexProjection};
use kukuri_cn_indexer::public_bucket_reader::PublicBucketReader;
use kukuri_cn_indexer::state::IndexerRuntimeState;
use kukuri_cn_indexer::worker::{IndexerWorker, WorkerConfig};
use kukuri_core::{
    KukuriKeys, ObjectVisibility, PayloadRef, TopicId, build_post_envelope_with_payload,
    timeline_sort_key,
};
use kukuri_docs_sync::{
    BucketReplica, BucketScope, DocOp, DocsSync, IrohDocsSync, MemoryDocsSync, TimeBucket,
    stable_key,
};
use kukuri_iroh_node::IrohDocsNode;

#[path = "ingest_support/mod.rs"]
mod ingest_support;

fn loopback_ticket(node: &IrohDocsNode) -> String {
    let socket = node
        .endpoint()
        .bound_sockets()
        .into_iter()
        .next()
        .expect("socket");
    format!("{}@{socket}", node.endpoint().addr().id)
}

async fn publish(
    docs: &IrohDocsSync,
    replica: &kukuri_core::ReplicaId,
    topic: &TopicId,
    body: &str,
) -> Result<String> {
    let envelope = build_post_envelope_with_payload(
        &KukuriKeys::generate(),
        topic,
        PayloadRef::InlineText { text: body.into() },
        Vec::new(),
        Vec::new(),
        None,
        ObjectVisibility::Public,
    )?;
    let post = envelope.to_post_object()?.expect("post");
    let id = post.object_id.as_str().to_string();
    let sort_key = timeline_sort_key(post.created_at, &post.object_id);
    for (key, value) in [
        (
            stable_key("objects", &format!("{id}/state")),
            serde_json::to_value(&post)?,
        ),
        (
            stable_key("objects", &format!("{id}/envelope")),
            serde_json::to_value(&envelope)?,
        ),
        (
            stable_key("indexes/timeline", &format!("{sort_key}/{id}")),
            serde_json::json!({"object_id": id}),
        ),
    ] {
        docs.apply_doc_op(replica, DocOp::SetJson { key, value })
            .await?;
    }
    Ok(id)
}

#[tokio::test(flavor = "multi_thread")]
async fn two_clients_feed_one_cn_through_bounded_bucket_reader() -> Result<()> {
    let Some(admin) = kukuri_test_support::gated_env_url(
        "KUKURI_CN_RUN_INTEGRATION_TESTS",
        "COMMUNITY_NODE_DATABASE_URL",
        "postgres://cn:cn_password@127.0.0.1:15432/cn",
    ) else {
        return Ok(());
    };
    let database = TestDatabase::create(&admin, "cn_remote_bucket_reader").await?;
    let pool = connect_postgres(&database.database_url).await?;
    initialize_database(&pool).await?;
    add_supported_topic(&pool, IndexScopeKind::PublicTopic, "rust").await?;
    mark_index_demand(&pool, IndexScopeKind::PublicTopic, "rust").await?;

    let node_a = IrohDocsNode::memory().await?;
    let node_b = IrohDocsNode::memory().await?;
    let cn_node = IrohDocsNode::memory().await?;
    let docs_a = Arc::new(IrohDocsSync::new(node_a.clone()));
    let docs_b = Arc::new(IrohDocsSync::new(node_b.clone()));
    let cn_docs = Arc::new(IrohDocsSync::new(cn_node.clone()));
    let now = chrono::Utc::now().timestamp();
    let replica = BucketReplica::new(
        BucketScope::Topic {
            topic_id: "rust".into(),
        },
        TimeBucket::from_unix_seconds(now)?,
    )?
    .replica_id();
    let topic = TopicId::new("rust");
    let a = publish(&docs_a, &replica, &topic, "from client a").await?;
    let b = publish(&docs_b, &replica, &topic, "from client b").await?;
    cn_docs
        .import_peer_ticket(&loopback_ticket(&node_a))
        .await?;
    cn_docs
        .import_peer_ticket(&loopback_ticket(&node_b))
        .await?;

    let (safety, artifacts) = ingest_support::allow_service();
    let entries = Arc::new(MemoryIndexEntryStore::new(artifacts));
    let projection = Arc::new(MemoryIndexProjection::default());
    let pipeline = IngestPipeline::new(
        cn_docs.clone(),
        safety.clone(),
        entries.clone(),
        projection.clone(),
    );
    let reader = Arc::new(PublicBucketReader::new(
        pool.clone(),
        cn_docs.clone(),
        entries.clone(),
        pipeline,
    ));
    assert_eq!(reader.poll_once(now).await?.indexed, 2);
    assert!(
        projection
            .contains_object(IndexScopeKind::PublicTopic, "rust", &a)
            .await?
    );
    assert!(
        projection
            .contains_object(IndexScopeKind::PublicTopic, "rust", &b)
            .await?
    );
    let mut local_namespaces = cn_node.docs().list().await?;
    assert!(
        local_namespaces.next().await.is_none(),
        "bounded remote read must not import or sync the bucket namespace"
    );

    let legacy_docs = Arc::new(MemoryDocsSync::default());
    let legacy_pipeline = IngestPipeline::new(
        legacy_docs.clone(),
        safety,
        entries.clone(),
        projection.clone(),
    );
    let participant = IndexerParticipant::new(
        pool.clone(),
        legacy_docs.clone(),
        entries,
        projection.clone(),
        legacy_pipeline,
        ChannelSecretCipher::from_key_material("public-bucket-test-cipher-key-0123456789")?,
    );
    let worker = IndexerWorker::new(
        Arc::new(participant),
        legacy_docs,
        Arc::new(IndexerRuntimeState::default()),
        WorkerConfig {
            poll_interval: Duration::from_millis(250),
            ..WorkerConfig::default()
        },
    )
    .with_public_bucket_reader(reader)
    .spawn();
    let after_start = publish(&docs_a, &replica, &topic, "after worker start").await?;
    let mut indexed = false;
    for _ in 0..50 {
        if projection
            .contains_object(IndexScopeKind::PublicTopic, "rust", &after_start)
            .await?
        {
            indexed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        indexed,
        "owned background reader must index a new bucket post"
    );
    worker.shutdown().await;
    let after_stop = publish(&docs_a, &replica, &topic, "after worker stop").await?;
    tokio::time::sleep(Duration::from_millis(750)).await;
    assert!(
        !projection
            .contains_object(IndexScopeKind::PublicTopic, "rust", &after_stop)
            .await?,
        "shutdown must stop the background reader"
    );

    cn_docs.shutdown().await;
    docs_a.shutdown().await;
    docs_b.shutdown().await;
    cn_node.shutdown().await?;
    node_a.shutdown().await?;
    node_b.shutdown().await?;
    pool.close().await;
    database.cleanup().await?;
    Ok(())
}
