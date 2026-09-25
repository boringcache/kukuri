use super::*;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

pub const REMOTE_CACHE_CAPACITY_BYTES: i64 = 1024 * 1024 * 1024;
pub const REMOTE_CACHE_UNUSED_MS: i64 = 7 * 24 * 60 * 60 * 1000;
pub const REMOTE_CACHE_RECLAIM_STEP: usize = 128;

pub struct RemoteCacheReservation {
    counter: Arc<AtomicU64>,
    bytes: u64,
}

impl RemoteCacheReservation {
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
}

impl Drop for RemoteCacheReservation {
    fn drop(&mut self) {
        self.counter.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

fn now_ms() -> Result<i64> {
    Ok(i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis(),
    )?)
}

async fn delete_cache_item(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    kind: &str,
    key: &str,
) -> Result<()> {
    if kind == "projection" {
        sqlx::query("DELETE FROM object_thread_cache WHERE object_id = ?1")
            .bind(key)
            .execute(&mut **tx)
            .await?;
        sqlx::query("DELETE FROM object_index_cache WHERE object_id = ?1")
            .bind(key)
            .execute(&mut **tx)
            .await?;
    }
    sqlx::query("DELETE FROM remote_content_cache WHERE kind = ?1 AND cache_key = ?2")
        .bind(kind)
        .bind(key)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

impl SqliteStore {
    pub fn empty_remote_cache_reservation(&self) -> RemoteCacheReservation {
        RemoteCacheReservation {
            counter: self.remote_cache_reserved.clone(),
            bytes: 0,
        }
    }

    /// Reserve transfer bytes before appending the next bounded chunk in memory.
    /// The caller keeps the token until the fetch completes or is cancelled.
    pub async fn reserve_remote_cache_bytes(
        &self,
        reservation: &mut RemoteCacheReservation,
        bytes: u64,
    ) -> Result<bool> {
        anyhow::ensure!(
            Arc::ptr_eq(&reservation.counter, &self.remote_cache_reserved),
            "reservation belongs to another cache"
        );
        let _gate = self.remote_cache_gate.lock().await;
        let reserved = self.remote_cache_reserved.load(Ordering::Acquire);
        let target = reserved.saturating_add(bytes);
        if target > REMOTE_CACHE_CAPACITY_BYTES as u64 {
            return Ok(false);
        }
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE remote_content_cache_usage SET used_bytes = used_bytes WHERE id = 1")
            .execute(&mut *tx)
            .await?;
        let mut used = sqlx::query_scalar::<_, i64>(
            "SELECT used_bytes FROM remote_content_cache_usage WHERE id = 1",
        )
        .fetch_one(&mut *tx)
        .await?;
        let mut reclaimed = 0;
        while (used as u64).saturating_add(target) > REMOTE_CACHE_CAPACITY_BYTES as u64
            && reclaimed < REMOTE_CACHE_RECLAIM_STEP
        {
            let row = sqlx::query(
                "SELECT kind, cache_key, charged_bytes FROM remote_content_cache \
                 WHERE is_protected = 0 ORDER BY last_used_at, kind, cache_key LIMIT 1",
            )
            .fetch_optional(&mut *tx)
            .await?;
            let Some(row) = row else { break };
            delete_cache_item(
                &mut tx,
                &row.get::<String, _>("kind"),
                &row.get::<String, _>("cache_key"),
            )
            .await?;
            used -= row.get::<i64, _>("charged_bytes");
            reclaimed += 1;
        }
        sqlx::query("UPDATE remote_content_cache_usage SET used_bytes = ?1 WHERE id = 1")
            .bind(used)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        if (used as u64).saturating_add(target) > REMOTE_CACHE_CAPACITY_BYTES as u64 {
            return Ok(false);
        }
        self.remote_cache_reserved
            .fetch_add(bytes, Ordering::AcqRel);
        reservation.bytes += bytes;
        Ok(true)
    }

    /// One background pass only; callers reschedule while a full page remains.
    pub async fn reclaim_remote_cache_step(&self) -> Result<usize> {
        let _gate = self.remote_cache_gate.lock().await;
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE remote_content_cache_usage SET used_bytes = used_bytes WHERE id = 1")
            .execute(&mut *tx)
            .await?;
        let rows = sqlx::query(
            "SELECT kind, cache_key, charged_bytes FROM remote_content_cache \
             WHERE is_protected = 0 AND last_used_at <= ?1 \
             ORDER BY last_used_at, kind, cache_key LIMIT ?2",
        )
        .bind(now_ms()? - REMOTE_CACHE_UNUSED_MS)
        .bind(i64::try_from(REMOTE_CACHE_RECLAIM_STEP)?)
        .fetch_all(&mut *tx)
        .await?;
        let count = rows.len();
        let mut reclaimed_bytes = 0;
        for row in rows {
            delete_cache_item(
                &mut tx,
                &row.get::<String, _>("kind"),
                &row.get::<String, _>("cache_key"),
            )
            .await?;
            reclaimed_bytes += row.get::<i64, _>("charged_bytes");
        }
        sqlx::query(
            "UPDATE remote_content_cache_usage SET used_bytes = used_bytes - ?1 WHERE id = 1",
        )
        .bind(reclaimed_bytes)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(count)
    }

    pub(super) async fn charge_remote_projection(
        tx: &mut sqlx::Transaction<'_, Sqlite>,
        row: &ObjectProjectionRow,
        budget: i64,
    ) -> Result<bool> {
        let charge = i64::try_from(serde_json::to_vec(row)?.len())? + 512;
        Self::put_remote_content_in_tx(
            tx,
            "projection",
            row.object_id.as_str(),
            row.source_replica_id.as_str(),
            &[],
            None,
            None,
            budget,
            now_ms()?,
            Some(charge),
        )
        .await
    }

    /// Store one remote record or blob. A false result means the bounded reclaim
    /// step could not admit it without exceeding the cache budget.
    pub async fn put_remote_content(
        &self,
        kind: &str,
        key: &str,
        scope: &str,
        payload: &[u8],
    ) -> Result<bool> {
        self.put_remote_content_with_budget(
            kind,
            key,
            scope,
            payload,
            None,
            None,
            REMOTE_CACHE_CAPACITY_BYTES,
            now_ms()?,
        )
        .await
    }

    pub async fn put_remote_record(
        &self,
        replica: &str,
        key: &str,
        author: &str,
        payload: &[u8],
    ) -> Result<bool> {
        let cache_key = format!("{replica}\0{key}\0{author}");
        self.put_remote_content_with_budget(
            "record",
            &cache_key,
            replica,
            payload,
            Some(key),
            Some(author),
            REMOTE_CACHE_CAPACITY_BYTES,
            now_ms()?,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn put_remote_content_with_budget(
        &self,
        kind: &str,
        key: &str,
        scope: &str,
        payload: &[u8],
        record_key: Option<&str>,
        record_author: Option<&str>,
        budget: i64,
        now: i64,
    ) -> Result<bool> {
        let _gate = self.remote_cache_gate.lock().await;
        let reserved = i64::try_from(self.remote_cache_reserved.load(Ordering::Acquire))?;
        let mut tx = self.pool.begin().await?;
        let admitted = Self::put_remote_content_in_tx(
            &mut tx,
            kind,
            key,
            scope,
            payload,
            record_key,
            record_author,
            budget.saturating_sub(reserved),
            now,
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(admitted)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn put_remote_content_in_tx(
        tx: &mut sqlx::Transaction<'_, Sqlite>,
        kind: &str,
        key: &str,
        scope: &str,
        payload: &[u8],
        record_key: Option<&str>,
        record_author: Option<&str>,
        budget: i64,
        now: i64,
        charged_bytes: Option<i64>,
    ) -> Result<bool> {
        anyhow::ensure!(
            matches!(kind, "blob" | "record" | "projection"),
            "unknown remote cache kind"
        );
        let charge = charged_bytes
            .unwrap_or(i64::try_from(payload.len() + kind.len() + key.len() + scope.len())? + 64);
        // The ledger row is the write lock shared by all cache writers.
        sqlx::query("UPDATE remote_content_cache_usage SET used_bytes = used_bytes WHERE id = 1")
            .execute(&mut **tx)
            .await?;
        let protected = sqlx::query_scalar::<_, i64>(
            "SELECT EXISTS(SELECT 1 FROM remote_content_cache_protected_ref WHERE kind = ?1 AND cache_key = ?2)",
        )
        .bind(kind)
        .bind(key)
        .fetch_one(&mut **tx)
        .await?
            != 0;
        if !protected && charge > budget {
            return Ok(false);
        }
        let old = sqlx::query(
            "SELECT charged_bytes, is_protected FROM remote_content_cache WHERE kind = ?1 AND cache_key = ?2",
        )
        .bind(kind)
        .bind(key)
        .fetch_optional(&mut **tx)
        .await?;
        let old_unprotected = old
            .as_ref()
            .filter(|row| row.get::<i64, _>("is_protected") == 0)
            .map_or(0, |row| row.get::<i64, _>("charged_bytes"));
        let used = sqlx::query_scalar::<_, i64>(
            "SELECT used_bytes FROM remote_content_cache_usage WHERE id = 1",
        )
        .fetch_one(&mut **tx)
        .await?;
        let mut next_used = used - old_unprotected + if protected { 0 } else { charge };
        let mut reclaimed = 0;
        while reclaimed < REMOTE_CACHE_RECLAIM_STEP {
            let expired = sqlx::query(
                "SELECT kind, cache_key, charged_bytes FROM remote_content_cache \
                 WHERE is_protected = 0 AND last_used_at <= ?1 \
                 AND NOT (kind = ?2 AND cache_key = ?3) \
                 ORDER BY last_used_at, kind, cache_key LIMIT 1",
            )
            .bind(now - REMOTE_CACHE_UNUSED_MS)
            .bind(kind)
            .bind(key)
            .fetch_optional(&mut **tx)
            .await?;
            let victim = if let Some(row) = expired {
                Some(row)
            } else if next_used > budget {
                sqlx::query(
                    "SELECT kind, cache_key, charged_bytes FROM remote_content_cache \
                     WHERE is_protected = 0 AND NOT (kind = ?1 AND cache_key = ?2) \
                     ORDER BY last_used_at, kind, cache_key LIMIT 1",
                )
                .bind(kind)
                .bind(key)
                .fetch_optional(&mut **tx)
                .await?
            } else {
                None
            };
            let Some(victim) = victim else { break };
            let victim_kind: String = victim.get("kind");
            let victim_key: String = victim.get("cache_key");
            delete_cache_item(tx, &victim_kind, &victim_key).await?;
            next_used -= victim.get::<i64, _>("charged_bytes");
            reclaimed += 1;
        }
        if next_used > budget {
            sqlx::query("UPDATE remote_content_cache_usage SET used_bytes = ?1 WHERE id = 1")
                .bind(next_used - if protected { 0 } else { charge } + old_unprotected)
                .execute(&mut **tx)
                .await?;
            return Ok(false);
        }
        sqlx::query(
            "INSERT INTO remote_content_cache \
             (kind, cache_key, scope_key, record_key, record_author, payload, charged_bytes, is_protected, last_used_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
             ON CONFLICT(kind, cache_key) DO UPDATE SET \
             scope_key = excluded.scope_key, record_key = excluded.record_key, \
             record_author = excluded.record_author, payload = excluded.payload, \
             charged_bytes = excluded.charged_bytes, is_protected = excluded.is_protected, \
             last_used_at = excluded.last_used_at",
        )
        .bind(kind)
        .bind(key)
        .bind(scope)
        .bind(record_key)
        .bind(record_author)
        .bind(payload)
        .bind(charge)
        .bind(if protected { 1 } else { 0 })
        .bind(now)
        .execute(&mut **tx)
        .await?;
        sqlx::query("UPDATE remote_content_cache_usage SET used_bytes = ?1 WHERE id = 1")
            .bind(next_used)
            .execute(&mut **tx)
            .await?;
        Ok(true)
    }

    pub async fn get_remote_content(&self, kind: &str, key: &str) -> Result<Option<Vec<u8>>> {
        let now = now_ms()?;
        let row = sqlx::query(
            "SELECT payload FROM remote_content_cache \
             WHERE kind = ?1 AND cache_key = ?2 \
             AND (is_protected = 1 OR last_used_at > ?3)",
        )
        .bind(kind)
        .bind(key)
        .bind(now - REMOTE_CACHE_UNUSED_MS)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else { return Ok(None) };
        sqlx::query(
            "UPDATE remote_content_cache SET last_used_at = ?1 WHERE kind = ?2 AND cache_key = ?3",
        )
        .bind(now)
        .bind(kind)
        .bind(key)
        .execute(&self.pool)
        .await?;
        Ok(Some(row.get("payload")))
    }

    pub async fn get_remote_records(
        &self,
        replica: &str,
        key: &str,
        author: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Vec<u8>>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        anyhow::ensure!(limit <= 8, "remote record cache limit exceeded");
        let now = now_ms()?;
        let rows = if let Some(author) = author {
            sqlx::query(
                "SELECT cache_key, payload, is_protected, last_used_at FROM remote_content_cache \
                 WHERE kind = 'record' AND scope_key = ?1 AND record_key = ?2 \
                   AND record_author = ?3 LIMIT 1",
            )
            .bind(replica)
            .bind(key)
            .bind(author)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                "SELECT cache_key, payload, is_protected, last_used_at FROM remote_content_cache \
                 WHERE kind = 'record' AND scope_key = ?1 AND record_key = ?2 \
                 ORDER BY record_author LIMIT ?3",
            )
            .bind(replica)
            .bind(key)
            .bind(i64::try_from(limit)?)
            .fetch_all(&self.pool)
            .await?
        };
        let mut result = Vec::with_capacity(rows.len());
        for row in rows {
            if row.get::<i64, _>("is_protected") == 0
                && row.get::<i64, _>("last_used_at") <= now - REMOTE_CACHE_UNUSED_MS
            {
                continue;
            }
            sqlx::query(
                "UPDATE remote_content_cache SET last_used_at = ?1 WHERE kind = 'record' AND cache_key = ?2",
            )
            .bind(now)
            .bind(row.get::<String, _>("cache_key"))
            .execute(&self.pool)
            .await?;
            result.push(row.get("payload"));
        }
        Ok(result)
    }

    pub async fn has_remote_content(&self, kind: &str, key: &str) -> Result<bool> {
        let now = now_ms()?;
        let row = sqlx::query(
            "SELECT last_used_at, is_protected FROM remote_content_cache \
             WHERE kind = ?1 AND cache_key = ?2",
        )
        .bind(kind)
        .bind(key)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.is_some_and(|row| {
            row.get::<i64, _>("is_protected") != 0
                || row.get::<i64, _>("last_used_at") > now - REMOTE_CACHE_UNUSED_MS
        }))
    }

    pub async fn remote_content_len(&self, kind: &str, key: &str) -> Result<Option<u64>> {
        let now = now_ms()?;
        let row = sqlx::query(
            "SELECT length(payload) AS bytes FROM remote_content_cache \
             WHERE kind = ?1 AND cache_key = ?2 \
             AND (is_protected = 1 OR last_used_at > ?3)",
        )
        .bind(kind)
        .bind(key)
        .bind(now - REMOTE_CACHE_UNUSED_MS)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(row) = row {
            sqlx::query(
                "UPDATE remote_content_cache SET last_used_at = ?1 WHERE kind = ?2 AND cache_key = ?3",
            )
            .bind(now)
            .bind(kind)
            .bind(key)
            .execute(&self.pool)
            .await?;
            Ok(Some(u64::try_from(row.get::<i64, _>("bytes"))?))
        } else {
            Ok(None)
        }
    }

    pub async fn remote_content_chunk(
        &self,
        kind: &str,
        key: &str,
        offset: u64,
        limit: usize,
    ) -> Result<Option<Vec<u8>>> {
        anyhow::ensure!(limit <= 1024 * 1024, "remote cache chunk limit exceeded");
        let row = sqlx::query(
            "SELECT substr(payload, ?3, ?4) AS chunk FROM remote_content_cache \
             WHERE kind = ?1 AND cache_key = ?2",
        )
        .bind(kind)
        .bind(key)
        .bind(i64::try_from(offset)? + 1)
        .bind(i64::try_from(limit)?)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|row| row.get("chunk")))
    }

    pub async fn protect_remote_content(
        &self,
        kind: &str,
        key: &str,
        reference: &str,
    ) -> Result<()> {
        let _gate = self.remote_cache_gate.lock().await;
        let mut tx = self.pool.begin().await?;
        Self::add_remote_protected_ref(&mut tx, kind, key, reference).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn add_remote_protected_ref(
        tx: &mut sqlx::Transaction<'_, Sqlite>,
        kind: &str,
        key: &str,
        reference: &str,
    ) -> Result<()> {
        sqlx::query("UPDATE remote_content_cache_usage SET used_bytes = used_bytes WHERE id = 1")
            .execute(&mut **tx)
            .await?;
        sqlx::query(
            "INSERT OR IGNORE INTO remote_content_cache_protected_ref (kind, cache_key, ref_id) \
             VALUES (?1, ?2, ?3)",
        )
        .bind(kind)
        .bind(key)
        .bind(reference)
        .execute(&mut **tx)
        .await?;
        sqlx::query(
            "UPDATE remote_content_cache_usage SET used_bytes = used_bytes - \
             COALESCE((SELECT charged_bytes FROM remote_content_cache \
             WHERE kind = ?1 AND cache_key = ?2 AND is_protected = 0), 0) WHERE id = 1",
        )
        .bind(kind)
        .bind(key)
        .execute(&mut **tx)
        .await?;
        sqlx::query(
            "UPDATE remote_content_cache SET is_protected = 1 WHERE kind = ?1 AND cache_key = ?2",
        )
        .bind(kind)
        .bind(key)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    pub(super) async fn replace_remote_protected_refs(
        tx: &mut sqlx::Transaction<'_, Sqlite>,
        reference: &str,
        desired: &[(String, String)],
        budget: i64,
    ) -> Result<()> {
        let old = sqlx::query(
            "SELECT kind, cache_key FROM remote_content_cache_protected_ref WHERE ref_id = ?1",
        )
        .bind(reference)
        .fetch_all(&mut **tx)
        .await?
        .into_iter()
        .map(|row| {
            (
                row.get::<String, _>("kind"),
                row.get::<String, _>("cache_key"),
            )
        })
        .collect::<HashSet<_>>();
        let desired = desired.iter().cloned().collect::<HashSet<_>>();
        for (kind, key) in old.difference(&desired) {
            sqlx::query(
                "DELETE FROM remote_content_cache_protected_ref \
                 WHERE kind = ?1 AND cache_key = ?2 AND ref_id = ?3",
            )
            .bind(kind)
            .bind(key)
            .bind(reference)
            .execute(&mut **tx)
            .await?;
            let remains = sqlx::query_scalar::<_, i64>(
                "SELECT EXISTS(SELECT 1 FROM remote_content_cache_protected_ref \
                 WHERE kind = ?1 AND cache_key = ?2)",
            )
            .bind(kind)
            .bind(key)
            .fetch_one(&mut **tx)
            .await?;
            if remains != 0 {
                continue;
            }
            let row = sqlx::query(
                "SELECT charged_bytes FROM remote_content_cache \
                 WHERE kind = ?1 AND cache_key = ?2 AND is_protected = 1",
            )
            .bind(kind)
            .bind(key)
            .fetch_optional(&mut **tx)
            .await?;
            let Some(row) = row else { continue };
            let charge = row.get::<i64, _>("charged_bytes");
            let used = sqlx::query_scalar::<_, i64>(
                "SELECT used_bytes FROM remote_content_cache_usage WHERE id = 1",
            )
            .fetch_one(&mut **tx)
            .await?;
            if used.saturating_add(charge) <= budget {
                sqlx::query(
                    "UPDATE remote_content_cache SET is_protected = 0 WHERE kind = ?1 AND cache_key = ?2",
                )
                .bind(kind)
                .bind(key)
                .execute(&mut **tx)
                .await?;
                sqlx::query(
                    "UPDATE remote_content_cache_usage SET used_bytes = used_bytes + ?1 WHERE id = 1",
                )
                .bind(charge)
                .execute(&mut **tx)
                .await?;
            } else {
                delete_cache_item(tx, kind, key).await?;
            }
        }
        for (kind, key) in desired.difference(&old) {
            Self::add_remote_protected_ref(tx, kind, key, reference).await?;
        }
        Ok(())
    }

    pub async fn remove_remote_content(&self, kind: &str, key: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE remote_content_cache_usage SET used_bytes = used_bytes WHERE id = 1")
            .execute(&mut *tx)
            .await?;
        let row = sqlx::query(
            "SELECT charged_bytes, is_protected FROM remote_content_cache WHERE kind = ?1 AND cache_key = ?2",
        )
        .bind(kind)
        .bind(key)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(row) = row {
            if row.get::<i64, _>("is_protected") == 0 {
                sqlx::query(
                    "UPDATE remote_content_cache_usage SET used_bytes = used_bytes - ?1 WHERE id = 1",
                )
                .bind(row.get::<i64, _>("charged_bytes"))
                .execute(&mut *tx)
                .await?;
            }
            delete_cache_item(&mut tx, kind, key).await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kukuri_core::PayloadRef;

    fn remote_post(object_id: &str) -> ObjectProjectionRow {
        let hash = BlobHash::new("a".repeat(64));
        ObjectProjectionRow {
            object_id: EnvelopeId::from(object_id),
            topic_id: "topic".into(),
            channel_id: "public".into(),
            author_pubkey: "b".repeat(64),
            created_at: 1,
            object_kind: "post".into(),
            root_object_id: None,
            reply_to_object_id: None,
            payload_ref: PayloadRef::BlobText {
                hash: hash.clone(),
                mime: "text/plain".into(),
                bytes: 1,
            },
            content: Some("body".into()),
            attachments: Vec::new(),
            repost_of: None,
            content_labels: Vec::new(),
            source_replica_id: ReplicaId::new("bucket::v1::topic::746f706963::1"),
            source_key: format!("objects/{object_id}/envelope"),
            source_envelope_id: EnvelopeId::from(object_id),
            source_blob_hash: Some(hash),
            source_docs_author: None,
            derived_at: 1,
            projection_version: 3,
        }
    }

    #[tokio::test]
    async fn remote_projection_and_its_page_index_are_reclaimed_together() {
        let store = SqliteStore::connect_memory().await.unwrap();
        let row = remote_post("remote-1");
        store
            .put_remote_object_projection(row.clone())
            .await
            .unwrap();
        let charged = sqlx::query_scalar::<_, i64>(
            "SELECT used_bytes FROM remote_content_cache_usage WHERE id = 1",
        )
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert!(charged > 0);
        assert!(
            store
                .put_remote_content_with_budget(
                    "blob",
                    "x",
                    "s",
                    b"v",
                    None,
                    None,
                    100,
                    now_ms().unwrap()
                )
                .await
                .unwrap()
        );
        assert!(
            store
                .get_object_projection(&row.object_id)
                .await
                .unwrap()
                .is_none()
        );
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM object_thread_cache WHERE object_id = ?1",
        )
        .bind(row.object_id.as_str())
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn bookmark_protects_shared_remote_hash_until_its_reference_is_removed() {
        let store = SqliteStore::connect_memory().await.unwrap();
        let row = remote_post("remote-bookmark");
        let hash = match &row.payload_ref {
            PayloadRef::BlobText { hash, .. } => hash.clone(),
            _ => unreachable!(),
        };
        store
            .put_remote_object_projection(row.clone())
            .await
            .unwrap();
        assert!(
            store
                .put_remote_content("blob", hash.as_str(), "blob", b"body")
                .await
                .unwrap()
        );
        let bookmark = BookmarkedPostRow {
            source_object_id: row.object_id.clone(),
            source_envelope_id: row.source_envelope_id.clone(),
            source_replica_id: row.source_replica_id.clone(),
            topic_id: row.topic_id.clone(),
            channel_id: row.channel_id.clone(),
            author_pubkey: row.author_pubkey.clone(),
            created_at: row.created_at,
            object_kind: row.object_kind.clone(),
            payload_ref: row.payload_ref.clone(),
            content: row.content.clone(),
            attachments: row.attachments.clone(),
            reply_to_object_id: row.reply_to_object_id.clone(),
            root_object_id: row.root_object_id.clone(),
            repost_of: row.repost_of.clone(),
            bookmarked_at: 2,
        };
        store.put_bookmarked_post(bookmark.clone()).await.unwrap();
        let mut second = bookmark;
        second.source_object_id = EnvelopeId::from("second-bookmark");
        store.put_bookmarked_post(second.clone()).await.unwrap();
        let protected = sqlx::query_scalar::<_, i64>(
            "SELECT is_protected FROM remote_content_cache WHERE kind = 'blob' AND cache_key = ?1",
        )
        .bind(hash.as_str())
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(protected, 1);
        store.remove_bookmarked_post(&row.object_id).await.unwrap();
        let protected = sqlx::query_scalar::<_, i64>(
            "SELECT is_protected FROM remote_content_cache WHERE kind = 'blob' AND cache_key = ?1",
        )
        .bind(hash.as_str())
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(protected, 1);
        store
            .remove_bookmarked_post(&second.source_object_id)
            .await
            .unwrap();
        let protected = sqlx::query_scalar::<_, i64>(
            "SELECT is_protected FROM remote_content_cache WHERE kind = 'blob' AND cache_key = ?1",
        )
        .bind(hash.as_str())
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(protected, 0);
    }

    #[tokio::test]
    async fn remote_cache_reclaims_lru_without_deleting_protected_content() {
        let store = SqliteStore::connect_memory().await.unwrap();
        let now = now_ms().unwrap();
        let payload = [7u8; 100];
        let charge = 100 + "blob".len() + "a".len() + "scope".len() + 64;
        let budget = (charge * 2) as i64;
        for (key, at) in [("a", now - 3), ("b", now - 2)] {
            assert!(
                store
                    .put_remote_content_with_budget(
                        "blob", key, "scope", &payload, None, None, budget, at
                    )
                    .await
                    .unwrap()
            );
        }
        store
            .protect_remote_content("blob", "a", "bookmark:1")
            .await
            .unwrap();
        for (key, at) in [("c", now - 1), ("d", now)] {
            assert!(
                store
                    .put_remote_content_with_budget(
                        "blob", key, "scope", &payload, None, None, budget, at
                    )
                    .await
                    .unwrap()
            );
        }
        assert!(
            store
                .get_remote_content("blob", "a")
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            store
                .get_remote_content("blob", "b")
                .await
                .unwrap()
                .is_none()
        );
        let used = sqlx::query_scalar::<_, i64>(
            "SELECT used_bytes FROM remote_content_cache_usage WHERE id = 1",
        )
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(used, budget);
    }

    #[tokio::test]
    async fn in_flight_reservation_counts_against_cache_writes_and_releases_on_drop() {
        let store = SqliteStore::connect_memory().await.unwrap();
        let mut reservation = store.empty_remote_cache_reservation();
        assert!(
            store
                .reserve_remote_cache_bytes(
                    &mut reservation,
                    REMOTE_CACHE_CAPACITY_BYTES as u64 - 80,
                )
                .await
                .unwrap()
        );
        assert!(
            !store
                .put_remote_content("blob", "reserved", "scope", &[1; 100])
                .await
                .unwrap()
        );
        drop(reservation);
        assert!(
            store
                .put_remote_content("blob", "reserved", "scope", &[1; 100])
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn remote_cache_reclaims_at_most_128_expired_items_per_write() {
        let store = SqliteStore::connect_memory().await.unwrap();
        let now = now_ms().unwrap();
        let old = now - REMOTE_CACHE_UNUSED_MS - 1;
        for i in 0..129 {
            store
                .put_remote_content_with_budget(
                    "record",
                    &format!("item-{i}"),
                    "scope",
                    b"v",
                    None,
                    None,
                    REMOTE_CACHE_CAPACITY_BYTES,
                    old,
                )
                .await
                .unwrap();
        }
        store
            .put_remote_content_with_budget(
                "record",
                "fresh",
                "scope",
                b"v",
                None,
                None,
                REMOTE_CACHE_CAPACITY_BYTES,
                now,
            )
            .await
            .unwrap();
        let expired = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM remote_content_cache WHERE last_used_at < ?1",
        )
        .bind(now - REMOTE_CACHE_UNUSED_MS)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(expired, 1);
        assert_eq!(store.reclaim_remote_cache_step().await.unwrap(), 1);
        assert_eq!(store.reclaim_remote_cache_step().await.unwrap(), 0);
    }
}
