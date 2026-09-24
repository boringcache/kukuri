use crate::service::*;

impl AppService {
    /// #858: 成人向け表現の表示設定(既定 OFF)。desktop-runtime が永続値を起動時に
    /// 反映し、設定変更時にも呼ぶ。
    pub fn set_adult_content_display_enabled(&self, enabled: bool) {
        self.adult_content_display_enabled
            .store(enabled, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn adult_content_display_enabled(&self) -> bool {
        self.adult_content_display_enabled
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// #1055: Community Node の content advisory が付いた添付 blob hash を取得ゲートへ登録する
    /// (insert-only)。desktop-runtime が index 応答の advisory を issuer 照合した後に呼ぶ。
    /// self-label 由来の hash と違い永続化しない(ADR 0028 §8.10 の transient 分類)。
    pub async fn register_advisory_media_hashes(&self, hashes: &[String]) {
        if hashes.is_empty() {
            return;
        }
        let mut registered = self.advisory_media_hashes.lock().await;
        for hash in hashes {
            let hash = hash.trim();
            if hash.is_empty() {
                continue;
            }
            registered.insert(hash.to_string());
        }
    }

    /// #1055: 対象 hash が advisory 付き添付として観測済みか。
    pub async fn is_advisory_media_hash(&self, hash: &str) -> bool {
        self.advisory_media_hashes.lock().await.contains(hash)
    }

    pub async fn blob_media_payload(
        &self,
        hash: &str,
        mime: &str,
    ) -> Result<Option<BlobMediaPayload>> {
        self.blob_media_payload_for_post(hash, mime, None).await
    }

    pub async fn blob_media_payload_for_post(
        &self,
        hash: &str,
        mime: &str,
        source_object_id: Option<&str>,
    ) -> Result<Option<BlobMediaPayload>> {
        let hash = hash.trim();
        if hash.is_empty() {
            warn!(mime = %mime, "blob media payload fetch skipped because hash was blank");
            return Ok(None);
        }
        info!(hash = %hash, mime = %mime, "blob media payload fetch requested");
        let blob_hash = kukuri_core::BlobHash::new(hash.to_string());
        let source = if let Some(object_id) = source_object_id {
            let object_id = EnvelopeId::from(object_id);
            let Some(row) = self
                .services
                .projection_store
                .get_object_projection(&object_id)
                .await?
            else {
                return Ok(None);
            };
            if !row
                .attachments
                .iter()
                .any(|attachment| attachment.hash == blob_hash)
                || self
                    .services
                    .projection_store
                    .get_post_withdrawal(&object_id)
                    .await?
                    .is_some()
            {
                return Ok(None);
            }
            let Some(generation) = self
                .services
                .active_content_scope_generation(&row.topic_id, &row.channel_id)
                .await
            else {
                return Ok(None);
            };
            Some((object_id, row.topic_id, row.channel_id, generation))
        } else {
            None
        };
        // #858 fail-closed バックストップ: 成人向けラベル付き投稿の添付として観測済みの
        // hash は、表示設定が OFF の間はネットワーク取得もローカル読み出しも行わない。
        // ON の場合も ephemeral fetch でローカル blob store へ永続化しない(ADR 0046)。
        // #1055: 投稿者の self-label に加えて、設定済み Community Node が発行した content
        // advisory の対象 hash も同じゲートで扱う(ADR 0046 §6.2)。ラベル源は 2 つだが、
        // 取得を止める判定点はここ 1 箇所のままにする。
        let adult_labeled = self
            .services
            .projection_store
            .is_adult_media_hash(&blob_hash)
            .await?
            || self.is_advisory_media_hash(hash).await;
        if adult_labeled && !self.adult_content_display_enabled() {
            info!(
                hash = %hash,
                mime = %mime,
                "blob media payload fetch blocked: adult-labeled media while display is disabled"
            );
            return Ok(None);
        }
        let fetch_result = if let Some((_, topic, channel, generation)) = &source {
            match self
                .services
                .until_content_invalid(
                    topic,
                    channel,
                    *generation,
                    self.services.blob_service.prepare_display_fetch(&blob_hash),
                )
                .await
            {
                Some(Ok(fetch)) => self
                    .services
                    .until_content_invalid(topic, channel, *generation, fetch)
                    .await
                    .unwrap_or(Ok(None)),
                Some(Err(error)) => Err(error),
                None => Ok(None),
            }
        } else {
            self.services
                .until_content_invalid(
                    "",
                    PUBLIC_CHANNEL_ID,
                    0,
                    self.services.blob_service.fetch_blob_ephemeral(&blob_hash),
                )
                .await
                .unwrap_or(Ok(None))
        };
        let bytes = match fetch_result {
            Ok(Some(bytes)) => {
                info!(
                    hash = %hash,
                    mime = %mime,
                    byte_len = bytes.len(),
                    "blob media payload fetch hit"
                );
                bytes
            }
            Ok(None) => {
                warn!(hash = %hash, mime = %mime, "blob media payload fetch miss");
                return Ok(None);
            }
            Err(error) => {
                warn!(
                    hash = %hash,
                    mime = %mime,
                    error = %error,
                    "blob media payload fetch failed"
                );
                return Err(error);
            }
        };
        let _save_access = self.services.content_save_access.lock().await;
        let currently_adult = self
            .services
            .projection_store
            .is_adult_media_hash(&blob_hash)
            .await?
            || self.is_advisory_media_hash(hash).await;
        if *self.services.content_closed.borrow()
            || (currently_adult && !self.adult_content_display_enabled())
        {
            return Ok(None);
        }
        if let Some((object_id, topic, channel, generation)) = source
            && (!self
                .services
                .content_scope_is_current(&topic, &channel, generation)
                .await
                || self
                    .services
                    .projection_store
                    .get_post_withdrawal(&object_id)
                    .await?
                    .is_some()
                || !self
                    .services
                    .projection_store
                    .get_object_projection(&object_id)
                    .await?
                    .is_some_and(|row| {
                        row.topic_id == topic
                            && row.channel_id == channel
                            && row.attachments.iter().any(|a| a.hash == blob_hash)
                    }))
        {
            return Ok(None);
        }
        if !currently_adult
            && self
                .services
                .blob_service
                .local_blob_status(&blob_hash)
                .await?
                == BlobStatus::Missing
        {
            let stored = self
                .services
                .blob_service
                .put_blob(bytes.clone(), mime)
                .await?;
            anyhow::ensure!(
                stored.hash == blob_hash,
                "media hash changed before storage"
            );
        }
        Ok(Some(BlobMediaPayload {
            bytes_base64: BASE64_STANDARD.encode(bytes),
            mime: mime.to_string(),
        }))
    }

    pub async fn blob_preview_data_url(&self, hash: &str, mime: &str) -> Result<Option<String>> {
        let Some(payload) = self.blob_media_payload(hash, mime).await? else {
            return Ok(None);
        };
        Ok(Some(format!(
            "data:{};base64,{}",
            payload.mime, payload.bytes_base64
        )))
    }
}
