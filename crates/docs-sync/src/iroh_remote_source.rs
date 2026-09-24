use super::*;
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
