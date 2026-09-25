//! Read a hash-addressed cached blob in bounded chunks without importing it
//! into the legacy iroh-blobs store.

use std::str::FromStr;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use iroh::EndpointAddr;
use iroh::endpoint::{Connection, Endpoint};
use iroh::protocol::{AcceptError, ProtocolHandler};
use iroh_blobs::Hash;
use kukuri_store::SqliteStore;
use tokio::sync::Semaphore;
use tokio::time::timeout;

use crate::remote_fetch::{BlobTooLarge, RemoteCacheDeferred};

pub(crate) const REMOTE_BLOB_ALPN: &[u8] = b"/kukuri/remote-blob/1";
const CHUNK_BYTES: usize = 1024 * 1024;
const DEADLINE: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub(crate) struct RemoteBlobProtocol {
    cache: Arc<OnceLock<Arc<SqliteStore>>>,
    permits: Arc<Semaphore>,
}

impl std::fmt::Debug for RemoteBlobProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteBlobProtocol").finish_non_exhaustive()
    }
}

impl RemoteBlobProtocol {
    pub(crate) fn new(cache: Arc<OnceLock<Arc<SqliteStore>>>) -> Self {
        Self {
            cache,
            permits: Arc::new(Semaphore::new(8)),
        }
    }

    async fn serve(&self, connection: &Connection) -> Result<()> {
        let (mut send, mut recv) = connection.accept_bi().await?;
        let request = recv.read_to_end(128).await?;
        let hash = Hash::from_str(std::str::from_utf8(&request)?)?;
        let Some(cache) = self.cache.get() else {
            send.write_all(&[0]).await?;
            send.finish()?;
            send.stopped().await?;
            return Ok(());
        };
        let Some(len) = cache.remote_content_len("blob", &hash.to_string()).await? else {
            send.write_all(&[0]).await?;
            send.finish()?;
            send.stopped().await?;
            return Ok(());
        };
        send.write_all(&[1]).await?;
        send.write_all(&len.to_be_bytes()).await?;
        let mut offset = 0;
        while offset < len {
            let chunk = cache
                .remote_content_chunk(
                    "blob",
                    &hash.to_string(),
                    offset,
                    CHUNK_BYTES.min(usize::try_from(len - offset)?),
                )
                .await?
                .context("cached blob disappeared during transfer")?;
            ensure!(
                !chunk.is_empty(),
                "cached blob ended before its declared size"
            );
            offset += u64::try_from(chunk.len())?;
            send.write_all(&chunk).await?;
        }
        send.finish()?;
        send.stopped().await?;
        Ok(())
    }
}

impl ProtocolHandler for RemoteBlobProtocol {
    async fn accept(&self, connection: Connection) -> std::result::Result<(), AcceptError> {
        let Ok(_permit) = self.permits.try_acquire() else {
            return Ok(());
        };
        timeout(DEADLINE, self.serve(&connection))
            .await
            .context("cached blob transfer timed out")
            .and_then(|result| result)
            .map_err(|error| AcceptError::from_boxed(error.into_boxed_dyn_error()))
    }
}

pub(crate) async fn fetch(
    endpoint: &Endpoint,
    peer: EndpointAddr,
    hash: Hash,
    max_bytes: Option<u64>,
    local_cache: Option<&SqliteStore>,
) -> Result<Option<Vec<u8>>> {
    let connection = endpoint.connect(peer, REMOTE_BLOB_ALPN).await?;
    let (mut send, mut recv) = connection.open_bi().await?;
    send.write_all(hash.to_string().as_bytes()).await?;
    send.finish()?;
    let mut present = [0];
    recv.read_exact(&mut present).await?;
    if present[0] == 0 {
        connection.close(0u32.into(), b"cached blob missing");
        return Ok(None);
    }
    ensure!(present[0] == 1, "invalid cached blob response");
    let mut length = [0; 8];
    recv.read_exact(&mut length).await?;
    let length = u64::from_be_bytes(length);
    if let Some(limit) = max_bytes
        && length > limit
    {
        return Err(BlobTooLarge { limit }.into());
    }
    let mut reservation = local_cache.map(SqliteStore::empty_remote_cache_reservation);
    if length <= kukuri_store::REMOTE_CACHE_CAPACITY_BYTES as u64
        && let (Some(cache), Some(reservation)) = (local_cache, reservation.as_mut())
        && !cache
            .reserve_remote_cache_bytes(reservation, length)
            .await?
    {
        return Err(RemoteCacheDeferred.into());
    }
    let mut bytes = Vec::new();
    while (bytes.len() as u64) < length {
        let mut chunk = vec![0; CHUNK_BYTES.min(usize::try_from(length - bytes.len() as u64)?)];
        recv.read_exact(&mut chunk).await?;
        bytes.extend_from_slice(&chunk);
    }
    ensure!(Hash::new(&bytes) == hash, "cached blob hash mismatch");
    connection.close(0u32.into(), b"cached blob complete");
    Ok(Some(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IrohDocsNode;

    #[tokio::test]
    async fn cached_blob_is_reprovided_without_legacy_store_import() -> Result<()> {
        let provider = IrohDocsNode::memory().await?;
        let requester = IrohDocsNode::memory().await?;
        let cache = Arc::new(SqliteStore::connect_memory().await?);
        provider.install_remote_cache(cache.clone())?;
        let bytes = b"cached remote bytes";
        let hash = Hash::new(bytes);
        ensure!(
            cache
                .put_remote_content("blob", &hash.to_string(), "blob", bytes)
                .await?,
            "fixture must fit"
        );
        assert!(!provider.blobs().blobs().has(hash).await?);
        assert_eq!(
            fetch(
                requester.endpoint(),
                provider.endpoint().addr(),
                hash,
                None,
                None
            )
            .await?,
            Some(bytes.to_vec())
        );
        provider.shutdown().await?;
        requester.shutdown().await?;
        Ok(())
    }
}
