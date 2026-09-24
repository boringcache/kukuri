use anyhow::Result;
use kukuri_cn_core::IndexScopeKind;
use kukuri_core::ReplicaId;
use kukuri_docs_sync::{DocsSync, SharedReplicaKeyFamily, query_time_index_window};

use super::{IngestPipeline, IngestSummary};

const RECENT_INDEX_PAGE: usize = 100;

pub(crate) async fn recent_object_keys(
    docs: &dyn DocsSync,
    replica_id: &ReplicaId,
    limit: usize,
) -> Result<Vec<String>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let page = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        query_time_index_window(
            docs,
            replica_id,
            SharedReplicaKeyFamily::TimelineIndex.prefix(),
            chrono::Utc::now().timestamp().saturating_add(600),
            limit.min(RECENT_INDEX_PAGE),
        ),
    )
    .await??;
    Ok(page
        .entries
        .into_iter()
        .filter(|entry| !entry.object_id.contains('/') && !entry.object_id.is_empty())
        .map(|entry| format!("objects/{}/state", entry.object_id))
        .collect())
}

impl IngestPipeline {
    /// Poll only the current index window. A missing page never de-indexes older entries.
    /// Remote readers can reuse this object-scoped path without importing a namespace.
    pub async fn ingest_recent_scope(
        &self,
        scope_kind: IndexScopeKind,
        scope_id: &str,
        replica_id: &ReplicaId,
    ) -> Result<IngestSummary> {
        crate::replica_plan::validate_scope_replica(scope_kind, scope_id, replica_id)?;
        if !self.retain_supported_scope(scope_kind, scope_id).await? {
            return Ok(IngestSummary::default());
        }
        let keys =
            recent_object_keys(self.docs_sync.as_ref(), replica_id, RECENT_INDEX_PAGE).await?;
        if keys.is_empty() {
            return Ok(IngestSummary::default());
        }
        self.ingest_changed_keys(scope_kind, scope_id, replica_id, &keys)
            .await
    }
}
