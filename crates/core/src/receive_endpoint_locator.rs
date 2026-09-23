//! Change-driven account-to-endpoint claim. A signed claim is only a dial
//! candidate; the live receive binding must still authenticate the QUIC peer.

use std::str::FromStr;

use anyhow::{Context, Result, ensure};
use secp256k1::{SECP256K1, XOnlyPublicKey, schnorr::Signature};
use serde::{Deserialize, Serialize};

use crate::crypto::sha256_digest;
use crate::{KukuriKeys, Pubkey, TopicId, receive_route_for_account};

pub const RECEIVE_ENDPOINT_LOCATOR_MAX_BYTES: usize = 1_024;
const LOCATOR_DOMAIN: &str = "kukuri:receive-endpoint-locator:v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiveEndpointLocatorV1 {
    pub version: u8,
    pub account: Pubkey,
    pub route: TopicId,
    pub endpoint_id: String,
    pub signature: String,
}

impl ReceiveEndpointLocatorV1 {
    pub fn sign(keys: &KukuriKeys, endpoint_id: &str) -> Result<Self> {
        let account = keys.public_key();
        let mut locator = Self {
            version: 1,
            route: receive_route_for_account(&account)?,
            account,
            endpoint_id: endpoint_id.to_owned(),
            signature: String::new(),
        };
        locator.validate_claim()?;
        locator.signature = keys.sign_schnorr(&locator.digest()?).to_string();
        ensure!(
            serde_json::to_vec(&locator)?.len() <= RECEIVE_ENDPOINT_LOCATOR_MAX_BYTES,
            "receive locator is too large"
        );
        Ok(locator)
    }

    /// Decoding does not establish account ownership or endpoint liveness.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= RECEIVE_ENDPOINT_LOCATOR_MAX_BYTES,
            "receive locator is too large"
        );
        serde_json::from_slice(bytes).context("invalid receive endpoint locator")
    }

    /// Verifies only the account's signature over this candidate. The caller
    /// must obtain a fresh binding from the authenticated QUIC endpoint.
    pub fn verify_signature_for(&self, expected_account: &Pubkey) -> Result<()> {
        self.validate_claim()?;
        ensure!(
            &self.account == expected_account,
            "receive locator account mismatch"
        );
        ensure_canonical_hex(&self.signature, 64)?;
        let signature =
            Signature::from_str(&self.signature).context("invalid receive locator signature")?;
        let account = XOnlyPublicKey::from_str(self.account.as_str())?;
        SECP256K1
            .verify_schnorr(&signature, &self.digest()?, &account)
            .context("receive locator signature verification failed")
    }

    fn validate_claim(&self) -> Result<()> {
        ensure!(self.version == 1, "unsupported receive locator version");
        ensure_canonical_hex(&self.endpoint_id, 32)?;
        ensure!(
            self.route == receive_route_for_account(&self.account)?,
            "receive locator route mismatch"
        );
        Ok(())
    }

    fn digest(&self) -> Result<[u8; 32]> {
        let canonical = serde_json::to_vec(&(
            LOCATOR_DOMAIN,
            self.version,
            &self.account,
            &self.route,
            &self.endpoint_id,
        ))?;
        Ok(sha256_digest(&canonical))
    }
}

fn ensure_canonical_hex(value: &str, bytes: usize) -> Result<()> {
    ensure!(
        value.len() == bytes * 2
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "invalid canonical receive locator hex"
    );
    Ok(())
}
