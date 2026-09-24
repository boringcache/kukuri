use super::*;
use crate::buckets::{BucketReplica, BucketScope};
use crate::remote_source::RemoteDocsSource;
use kukuri_iroh_node::{DocReadQuery, DocReadResponse};

impl IrohDocsSync {
    /// A moving, bounded candidate window; callers keep each object read on one provider.
    pub async fn remote_read_candidates(&self) -> Vec<EndpointAddr> {
        self.peers.ranked_peers().await
    }

    pub fn remote_source(&self, peer: EndpointAddr) -> RemoteDocsSource {
        RemoteDocsSource::new(self.clone(), peer)
    }

    pub(crate) async fn public_bucket_readers_owned(
        &self,
        replica: &ReplicaId,
    ) -> Result<Vec<Arc<dyn DocsSync>>> {
        anyhow::ensure!(
            matches!(
                BucketReplica::parse(replica)?.scope(),
                BucketScope::Topic { .. }
            ),
            "remote bucket reader requires a public topic"
        );
        Ok(self
            .remote_read_candidates()
            .await
            .into_iter()
            .map(|peer| Arc::new(self.remote_source(peer)) as Arc<dyn DocsSync>)
            .collect())
    }

    pub(crate) async fn query_remote_docs(
        &self,
        replica: &ReplicaId,
        peer: EndpointAddr,
        query: DocReadQuery,
    ) -> Result<DocReadResponse> {
        let secret = self.replica_secret(replica).await?;
        self.node
            .query_remote_docs(peer, replica, &secret, query)
            .await
    }
}
