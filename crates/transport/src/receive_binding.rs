//! 公開のaccount/endpoint対応を、実際に接続したendpointに照合する。
//! このprotocolだけでは投稿のscope権限や通知の配送完了を認定しない。

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler};
use iroh::{Endpoint, EndpointAddr, EndpointId};
use kukuri_core::{
    Pubkey, RECEIVE_ENDPOINT_BINDING_MAX_BYTES, ReceiveEndpointBindingV1,
    VerifiedReceiveEndpointBinding, receive_route_for_account,
};
use tokio::sync::{RwLock, Semaphore};
use tokio::time::{Instant, timeout, timeout_at};

pub const RECEIVE_BINDING_ALPN: &[u8] = b"/kukuri/receive-binding/1";
const RECEIVE_BINDING_CONCURRENT_REQUESTS: usize = 2;
const RECEIVE_BINDING_SERVE_TIMEOUT: Duration = Duration::from_secs(2);

/// account runtimeに組み込む際はownerがbinding更新とRouter終了を所有する。
/// 新たな待機taskを作らず、満杯時は接続を閉じる。
#[derive(Debug, Clone)]
pub struct ReceiveBindingProtocol {
    local_endpoint: EndpointId,
    account: Pubkey,
    binding: Arc<RwLock<ReceiveEndpointBindingV1>>,
    permits: Arc<Semaphore>,
}

impl ReceiveBindingProtocol {
    pub fn new(local_endpoint: EndpointId, binding: ReceiveEndpointBindingV1) -> Result<Self> {
        binding.verify_for(&binding.account, &local_endpoint.to_string(), now_ms())?;
        Ok(Self {
            local_endpoint,
            account: binding.account.clone(),
            binding: Arc::new(RwLock::new(binding)),
            permits: Arc::new(Semaphore::new(RECEIVE_BINDING_CONCURRENT_REQUESTS)),
        })
    }

    pub async fn replace(&self, binding: ReceiveEndpointBindingV1) -> Result<()> {
        binding.verify_for(&self.account, &self.local_endpoint.to_string(), now_ms())?;
        let mut current = self.binding.write().await;
        ensure!(
            binding.issued_at_ms >= current.issued_at_ms,
            "stale receive binding update"
        );
        *current = binding;
        Ok(())
    }

    async fn serve(&self, connection: &Connection) -> Result<()> {
        let (mut send, mut recv) = connection.accept_bi().await?;
        ensure!(
            recv.read_to_end(1).await? == [1],
            "unsupported binding request"
        );
        let binding = self.binding.read().await.clone();
        binding.verify_for(&self.account, &self.local_endpoint.to_string(), now_ms())?;
        let bytes = serde_json::to_vec(&binding)?;
        ensure!(
            bytes.len() <= RECEIVE_ENDPOINT_BINDING_MAX_BYTES,
            "binding response is too large"
        );
        send.write_all(&bytes).await?;
        send.finish()?;
        send.stopped().await?;
        Ok(())
    }
}

impl ProtocolHandler for ReceiveBindingProtocol {
    async fn accept(&self, connection: Connection) -> std::result::Result<(), AcceptError> {
        let connection = CloseBindingConnection(connection);
        let Ok(_permit) = self.permits.try_acquire() else {
            return Ok(());
        };
        timeout(RECEIVE_BINDING_SERVE_TIMEOUT, self.serve(&connection.0))
            .await
            .context("receive binding request timed out")
            .and_then(|result| result)
            .map_err(|error| AcceptError::from_boxed(error.into_boxed_dyn_error()))
    }
}

/// ownerが選んだ候補1件を照合する。deadlineはqueue待機前の受付時点で決定し、
/// このfutureの取消時は接続も閉じる。独自retryやpeer探索を起動しない。
pub async fn fetch_receive_endpoint_binding(
    endpoint: &Endpoint,
    candidate: EndpointAddr,
    expected_account: &Pubkey,
    deadline: Instant,
) -> Result<VerifiedReceiveEndpointBinding> {
    receive_route_for_account(expected_account)?;
    ensure!(deadline > Instant::now(), "receive binding request expired");
    timeout_at(deadline, async {
        let connection =
            CloseBindingConnection(endpoint.connect(candidate, RECEIVE_BINDING_ALPN).await?);
        let (mut send, mut recv) = connection.0.open_bi().await?;
        send.write_all(&[1]).await?;
        send.finish()?;
        let bytes = recv.read_to_end(RECEIVE_ENDPOINT_BINDING_MAX_BYTES).await?;
        let binding = ReceiveEndpointBindingV1::decode(&bytes)?;
        binding.verify_for(
            expected_account,
            &connection.0.remote_id().to_string(),
            now_ms(),
        )
    })
    .await
    .context("receive binding lookup timed out")?
}

struct CloseBindingConnection(Connection);

impl Drop for CloseBindingConnection {
    fn drop(&mut self) {
        self.0
            .close(0u32.into(), b"receive binding exchange complete");
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[cfg(test)]
#[path = "receive_binding_tests.rs"]
mod tests;
