use anyhow::Result;
use kukuri_iroh_node::IrohDocsNode;

use crate::{
    BucketReplica, BucketScope, DocFetchPolicy, DocKeyOrder, DocKeyQuery, DocOp, DocsSync,
    IrohDocsSync, TimeBucket,
};

#[tokio::test]
async fn remote_object_lease_reads_provider_keys_and_keeps_local_snapshot() -> Result<()> {
    let provider = IrohDocsNode::memory().await?;
    let requester = IrohDocsNode::memory().await?;
    let writer = IrohDocsSync::new(provider.clone());
    let reader = IrohDocsSync::new(requester.clone());
    let replica = BucketReplica::new(
        BucketScope::Topic {
            topic_id: "remote-object-lease".into(),
        },
        TimeBucket::from_index(1)?,
    )?
    .replica_id();
    let key = "objects/post/state";
    writer
        .apply_doc_op(
            &replica,
            DocOp::SetBytes {
                key: "indexes/timeline/00000000000000000001-post/post".into(),
                value: b"index".to_vec(),
            },
        )
        .await?;
    writer
        .apply_doc_op(
            &replica,
            DocOp::SetBytes {
                key: key.into(),
                value: b"first".to_vec(),
            },
        )
        .await?;
    let peer = provider.endpoint().addr();
    let lease = reader.remote_source(peer.clone());
    let page = lease
        .query_replica_keys(
            &replica,
            DocKeyQuery {
                prefix: "indexes/timeline/".into(),
                order: DocKeyOrder::Descending,
                limit: 10,
            },
        )
        .await?;
    assert_eq!(page.entries.len(), 1);
    assert!(
        lease
            .query_replica_exact_bounded(&replica, key, 8, DocFetchPolicy::LocalOnly)
            .await?
            .is_empty()
    );
    let fetched = lease
        .query_replica_exact_bounded(&replica, key, 8, DocFetchPolicy::LocalThenRemote)
        .await?;
    assert_eq!(fetched[0].value, b"first");

    writer
        .apply_doc_op(
            &replica,
            DocOp::SetBytes {
                key: key.into(),
                value: b"second".to_vec(),
            },
        )
        .await?;
    let snapshot = lease
        .query_replica_exact_bounded(&replica, key, 8, DocFetchPolicy::LocalOnly)
        .await?;
    assert_eq!(snapshot[0].content_hash, fetched[0].content_hash);
    let next = reader
        .remote_source(peer)
        .query_replica_exact_bounded(&replica, key, 8, DocFetchPolicy::LocalThenRemote)
        .await?;
    assert_eq!(next[0].value, b"second");

    writer.shutdown().await;
    reader.shutdown().await;
    requester.shutdown().await?;
    provider.shutdown().await?;
    Ok(())
}
