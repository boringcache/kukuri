use anyhow::Result;
use iroh_docs::{Capability, NamespaceSecret};
use kukuri_core::ReplicaId;

use crate::{DocReadQuery, DocReadResponse, IrohDocsNode};

#[tokio::test]
async fn real_peer_returns_only_requested_local_keys_and_records() -> Result<()> {
    let provider = IrohDocsNode::memory().await?;
    let requester = IrohDocsNode::memory().await?;
    let replica = ReplicaId::new("bucket::v1::topic::72757374::1");
    let secret = NamespaceSecret::from_bytes(
        blake3::hash(format!("kukuri-docs:{}", replica.as_str()).as_bytes()).as_bytes(),
    );
    let doc = provider
        .docs()
        .import_namespace(Capability::Write(secret.clone()))
        .await?;
    let author = provider.docs().author_default().await?;
    doc.set_bytes(
        author,
        b"indexes/timeline/0001/a".to_vec(),
        b"first".to_vec(),
    )
    .await?;
    doc.set_bytes(
        author,
        b"indexes/timeline/0002/b".to_vec(),
        b"second".to_vec(),
    )
    .await?;
    doc.set_bytes(author, b"other/key".to_vec(), b"unrelated".to_vec())
        .await?;
    let before = doc.status().await?;

    let peer = provider.endpoint().addr();
    let page = requester
        .query_remote_docs(
            peer.clone(),
            &replica,
            &secret,
            DocReadQuery::Keys {
                prefix: "indexes/timeline/".into(),
                descending: true,
                limit: 1,
            },
        )
        .await?;
    let DocReadResponse::Keys {
        entries,
        reached_limit,
    } = page
    else {
        anyhow::bail!("expected index page")
    };
    assert!(reached_limit);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].key, "indexes/timeline/0002/b");

    let record = requester
        .query_remote_docs(
            peer.clone(),
            &replica,
            &secret,
            DocReadQuery::Exact {
                key: entries[0].key.clone(),
                limit: 8,
                author: None,
            },
        )
        .await?;
    let DocReadResponse::Records(records) = record else {
        anyhow::bail!("expected exact records")
    };
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].value, b"second");
    doc.set_bytes(author, b"indexes/timeline/\xff".to_vec(), b"junk".to_vec())
        .await?;
    let malformed = requester
        .query_remote_docs(
            peer.clone(),
            &replica,
            &secret,
            DocReadQuery::Keys {
                prefix: "indexes/timeline/".into(),
                descending: true,
                limit: 1,
            },
        )
        .await?;
    let DocReadResponse::Keys {
        entries: malformed_entries,
        reached_limit,
    } = malformed
    else {
        anyhow::bail!("expected bounded key page")
    };
    assert!(malformed_entries.is_empty());
    assert!(
        reached_limit,
        "skipped keys still count toward the page boundary"
    );
    for index in 0..8u8 {
        let author = provider.docs().author_create().await?;
        doc.set_bytes(author, b"bulk/key".to_vec(), vec![index; 64 * 1024])
            .await?;
    }
    let bulk = requester
        .query_remote_docs(
            peer.clone(),
            &replica,
            &secret,
            DocReadQuery::Exact {
                key: "bulk/key".into(),
                limit: 8,
                author: None,
            },
        )
        .await?;
    let DocReadResponse::Records(bulk) = bulk else {
        anyhow::bail!("expected bounded records")
    };
    assert_eq!(bulk.len(), 8);
    assert!(bulk.iter().all(|record| record.value.len() == 64 * 1024));
    assert!(
        requester
            .query_remote_docs(
                peer.clone(),
                &ReplicaId::new("channel::private"),
                &secret,
                DocReadQuery::Exact {
                    key: "bulk/key".into(),
                    limit: 1,
                    author: None,
                },
            )
            .await
            .is_err(),
        "the page protocol must not serve private replicas without their audience gate"
    );
    assert!(
        requester
            .query_remote_docs(
                peer,
                &replica,
                &NamespaceSecret::from_bytes(&[72; 32]),
                DocReadQuery::Exact {
                    key: entries[0].key.clone(),
                    limit: 8,
                    author: None,
                },
            )
            .await
            .is_err()
    );
    let after = doc.status().await?;
    assert_eq!(
        after.handles, before.handles,
        "remote reads must not open a handle"
    );
    assert_eq!(
        after.sync, before.sync,
        "remote reads must not start docs sync"
    );

    requester.shutdown().await?;
    provider.shutdown().await?;
    Ok(())
}
