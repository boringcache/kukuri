use super::*;

impl SqliteStore {
    pub(crate) async fn remove_remote_blob_files(&self, names: Vec<String>) -> Result<()> {
        for name in names {
            match tokio::fs::remove_file(self.remote_blob_path(&name)?).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    pub(super) fn remote_blob_path(&self, name: &str) -> Result<std::path::PathBuf> {
        anyhow::ensure!(
            name.len() == 64
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "invalid cached blob file name"
        );
        Ok(self
            .remote_cache_files
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("remote blob files are unavailable"))?
            .join(name))
    }
    pub async fn put_remote_blob_file(&self, hash: &str, path: &std::path::Path) -> Result<()> {
        anyhow::ensure!(
            hash.len() == 64
                && hash
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "invalid remote blob hash"
        );
        let root = self
            .remote_cache_files
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("file-backed remote cache is unavailable"))?;
        let length = tokio::fs::metadata(path).await?.len();
        let target = root.join(hash);
        let _gate = self.remote_cache_gate.lock().await;
        let created = match std::fs::hard_link(path, &target) {
            Ok(()) => true,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
            Err(_) => {
                let staging = tempfile::NamedTempFile::new_in(root)?;
                tokio::fs::copy(path, staging.path()).await?;
                match staging.persist_noclobber(&target) {
                    Ok(_) => true,
                    Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => false,
                    Err(error) => return Err(error.error.into()),
                }
            }
        };
        let mut committed = false;
        let result = async {
            let reserved = i64::try_from(self.remote_cache_reserved.load(Ordering::Acquire))?;
            let mut tx = self.pool.begin().await?;
            let mut label_evictions = Vec::new();
            let mut removed_files = Vec::new();
            let admitted = self
                .put_remote_content_in_tx(
                    &mut tx,
                    "blob",
                    hash,
                    "blob",
                    CachePayload::File {
                        name: hash,
                        bytes: length,
                    },
                    None,
                    None,
                    REMOTE_CACHE_CAPACITY_BYTES.saturating_sub(reserved),
                    now_ms()?,
                    None,
                    &mut label_evictions,
                    &mut removed_files,
                )
                .await?;
            anyhow::ensure!(admitted, "remote blob cache capacity exceeded");
            tx.commit().await?;
            committed = true;
            self.publish_adult_label_evictions(label_evictions);
            self.remove_remote_blob_files(removed_files).await?;
            Ok::<(), anyhow::Error>(())
        }
        .await;
        if result.is_err() && created && !committed {
            let _ = tokio::fs::remove_file(&target).await;
        }
        result
    }

    pub async fn copy_remote_content_to_file(
        &self,
        kind: &str,
        key: &str,
        path: &std::path::Path,
    ) -> Result<Option<u64>> {
        use tokio::io::AsyncWriteExt;
        let Some(length) = self.remote_content_len(kind, key).await? else {
            return Ok(None);
        };
        let file_name = sqlx::query_scalar::<_, Option<String>>(
            "SELECT file_name FROM remote_content_cache WHERE kind = ?1 AND cache_key = ?2",
        )
        .bind(kind)
        .bind(key)
        .fetch_optional(&self.pool)
        .await?
        .flatten();
        if let Some(name) = file_name
            && tokio::fs::hard_link(self.remote_blob_path(&name)?, path)
                .await
                .is_ok()
        {
            return Ok(Some(length));
        }
        let mut file = tokio::fs::File::create(path).await?;
        let mut offset = 0u64;
        while offset < length {
            let count = (length - offset).min(1024 * 1024) as usize;
            let chunk = self
                .remote_content_chunk(kind, key, offset, count)
                .await?
                .ok_or_else(|| anyhow::anyhow!("cached content disappeared during display copy"))?;
            anyhow::ensure!(
                !chunk.is_empty(),
                "cached content ended before its declared size"
            );
            file.write_all(&chunk).await?;
            offset += chunk.len() as u64;
        }
        file.flush().await?;
        Ok(Some(length))
    }
}
