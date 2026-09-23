use super::*;

const NOW: i64 = 1_790_000_000_000;

fn reference(scope: ReceiveOfferScopeV1) -> ReceiveOfferReferenceV1 {
    ReceiveOfferReferenceV1 {
        provider_endpoint_id: "ab".repeat(32),
        payload_hash: BlobHash("cd".repeat(32)),
        payload_bytes: 128,
        scope,
    }
}

#[test]
fn receive_offer_only_recipient_can_open_and_sender_is_authenticated() {
    let sender = KukuriKeys::generate();
    let recipient = KukuriKeys::generate();
    let reference = reference(ReceiveOfferScopeV1::DirectMessage);
    let offer = seal_receive_offer(
        &sender,
        &recipient.public_key(),
        reference.clone(),
        NOW,
        NOW + 60_000,
    )
    .unwrap();
    let wire = offer.encode().unwrap();
    assert!(wire.len() <= RECEIVE_OFFER_MAX_BYTES);
    let decoded = SealedReceiveOfferV1::decode(&wire).unwrap();
    let opened = decoded.open(&recipient, NOW).unwrap();
    assert_eq!(opened.sender(), &sender.public_key());
    assert_eq!(opened.reference(), &reference);
    assert!(decoded.open(&KukuriKeys::generate(), NOW).is_err());
    assert!(decoded.open(&recipient, NOW + 60_000).is_err());
    let text = String::from_utf8(wire).unwrap();
    assert!(!text.contains(sender.public_key_hex().as_str()));
    assert!(!text.contains(recipient.public_key_hex().as_str()));
    assert!(!text.contains(reference.payload_hash.as_str()));
}

#[test]
fn inline_dm_frame_and_signed_ack_fit_without_a_manifest_blob() {
    let sender = KukuriKeys::generate();
    let recipient = KukuriKeys::generate();
    let dm_id =
        crate::direct_message_id_for_participants(&sender.public_key(), &recipient.public_key());
    let message_id = "dm-message-1790000000000-0123456789abcdef";
    let frame = ReceiveOfferReferenceV1::inline(
        "ab".repeat(32),
        ReceiveOfferScopeV1::DirectMessageFrame {
            dm_id: dm_id.clone(),
            message_id: message_id.into(),
            frame_hash: BlobHash("cd".repeat(32)),
        },
    )
    .unwrap();
    let sealed = seal_receive_offer(
        &sender,
        &recipient.public_key(),
        frame.clone(),
        NOW,
        NOW + 60_000,
    )
    .unwrap();
    assert!(sealed.encode().unwrap().len() <= RECEIVE_OFFER_MAX_BYTES);
    assert_eq!(sealed.open(&recipient, NOW).unwrap().reference(), &frame);

    let ack = crate::build_direct_message_ack(
        &recipient,
        &dm_id,
        message_id,
        &sender.public_key(),
        NOW + 100,
    )
    .unwrap();
    let inline_ack = ReceiveOfferReferenceV1::inline(
        String::new(),
        ReceiveOfferScopeV1::DirectMessageAck {
            dm_id: ack.dm_id.clone(),
            message_id: ack.message_id.clone(),
            acked_at: ack.acked_at,
            signature: ack.signature.clone(),
        },
    )
    .unwrap();
    let sealed_ack = seal_receive_offer(
        &recipient,
        &sender.public_key(),
        inline_ack.clone(),
        NOW,
        NOW + 60_000,
    )
    .unwrap();
    assert!(sealed_ack.encode().unwrap().len() <= RECEIVE_OFFER_MAX_BYTES);
    assert_eq!(
        sealed_ack.open(&sender, NOW).unwrap().reference(),
        &inline_ack
    );

    let mut invalid = inline_ack;
    invalid.payload_bytes = 1;
    assert!(
        seal_receive_offer(&recipient, &sender.public_key(), invalid, NOW, NOW + 60_000).is_err()
    );
}

#[test]
fn receive_offer_tampering_and_excessive_input_are_rejected() {
    let sender = KukuriKeys::generate();
    let recipient = KukuriKeys::generate();
    let offer = seal_receive_offer(
        &sender,
        &recipient.public_key(),
        reference(ReceiveOfferScopeV1::PublicSource),
        NOW,
        NOW + 60_000,
    )
    .unwrap();
    let mut tampered = offer.clone();
    tampered.nonce_hex.replace_range(
        0..2,
        if &offer.nonce_hex[..2] == "00" {
            "01"
        } else {
            "00"
        },
    );
    assert!(tampered.open(&recipient, NOW).is_err());
    let mut tampered = offer.clone();
    tampered.version = 2;
    assert!(tampered.open(&recipient, NOW).is_err());
    let mut tampered = offer;
    tampered
        .ciphertext_hex
        .push_str(&"ab".repeat(RECEIVE_OFFER_MAX_BYTES));
    assert!(tampered.open(&recipient, NOW).is_err());
    assert!(SealedReceiveOfferV1::decode(&vec![b' '; RECEIVE_OFFER_MAX_BYTES + 1]).is_err());
}

#[test]
fn receive_offer_maximal_private_reference_fits_gossip_without_locator() {
    let sender = KukuriKeys::generate();
    let recipient = KukuriKeys::generate();
    let mut reference = reference(ReceiveOfferScopeV1::EpochControl {
        epoch_key_id: "ef".repeat(32),
    });
    reference.payload_bytes = RECEIVE_PAYLOAD_MAX_BYTES as u32;
    let offer = seal_receive_offer(
        &sender,
        &recipient.public_key(),
        reference,
        NOW,
        NOW + RECEIVE_OFFER_MAX_LIFETIME_MS,
    )
    .unwrap();
    assert!(offer.encode().unwrap().len() <= RECEIVE_OFFER_MAX_BYTES);
    let framed =
        serde_json::to_vec(&serde_json::json!({"AccountReceive": {"offer": offer}})).unwrap();
    assert!(framed.len() < 4096);
}

#[test]
fn receive_offer_rejects_bad_scope_provider_and_payload_size() {
    let sender = KukuriKeys::generate();
    let recipient = KukuriKeys::generate();
    let mut invalid = reference(ReceiveOfferScopeV1::PrivateSource {
        epoch_key_id: "unknown".into(),
    });
    assert!(
        seal_receive_offer(
            &sender,
            &recipient.public_key(),
            invalid.clone(),
            NOW,
            NOW + 1
        )
        .is_err()
    );
    invalid.scope = ReceiveOfferScopeV1::PublicSource;
    invalid.payload_bytes = 0;
    assert!(
        seal_receive_offer(
            &sender,
            &recipient.public_key(),
            invalid.clone(),
            NOW,
            NOW + 1
        )
        .is_err()
    );
    invalid.payload_bytes = RECEIVE_PAYLOAD_MAX_BYTES as u32 + 1;
    assert!(
        seal_receive_offer(
            &sender,
            &recipient.public_key(),
            invalid.clone(),
            NOW,
            NOW + 1
        )
        .is_err()
    );
    invalid.payload_bytes = 1;
    invalid.provider_endpoint_id = "AB".repeat(32);
    assert!(seal_receive_offer(&sender, &recipient.public_key(), invalid, NOW, NOW + 1).is_err());
}

#[test]
fn receive_offer_reencryption_cannot_forge_sender_or_change_signed_reference() {
    let sender = KukuriKeys::generate();
    let recipient = KukuriKeys::generate();
    let mut signed = SignedReceiveOffer {
        version: 1,
        sender: sender.public_key(),
        recipient: recipient.public_key(),
        reference: reference(ReceiveOfferScopeV1::PublicSource),
        issued_at_ms: NOW,
        expires_at_ms: NOW + 60_000,
        signature: String::new(),
    };
    signed.signature = sender.sign_schnorr(&signed.digest().unwrap()).to_string();
    let mut forged_sender = signed.clone();
    forged_sender.sender = KukuriKeys::generate().public_key();
    let mut changed_reference = signed.clone();
    changed_reference.reference.payload_hash = BlobHash("ef".repeat(32));
    let mut changed_provider = signed.clone();
    changed_provider.reference.provider_endpoint_id = "12".repeat(32);
    let mut changed_expiry = signed;
    changed_expiry.expires_at_ms += 1;
    for forged in [
        forged_sender,
        changed_reference,
        changed_provider,
        changed_expiry,
    ] {
        // Public key encryption is available to any sender, so AEAD alone cannot
        // authenticate the claimed sender or the referenced source.
        let sealed = seal_signed_offer(&forged).unwrap();
        let error = sealed.open(&recipient, NOW).unwrap_err();
        assert!(error.to_string().contains("signature"), "{error}");
    }
}

#[test]
fn receive_offer_epoch_selector_is_domain_separated_and_unambiguous() {
    let secret = [7; 32];
    let id = receive_epoch_key_id(&secret, "channel-a", "epoch-1").unwrap();
    assert_ne!(
        id,
        hex::encode(epoch_key(&secret, "channel-a", "epoch-1", PRIVATE_KEY_DOMAIN).unwrap())
    );
    assert_ne!(
        receive_epoch_key_id(&secret, "ab", "c").unwrap(),
        receive_epoch_key_id(&secret, "a", "bc").unwrap()
    );
    assert!(receive_epoch_key_id(&secret, "", "epoch-1").is_err());
    assert!(receive_epoch_key_id(&secret, &"a".repeat(1025), "epoch-1").is_err());
}

#[test]
fn private_receive_payload_requires_same_channel_epoch_and_secret() {
    let secret = [7; 32];
    let bytes = b"private-source-locator";
    let encrypted = seal_private_receive_payload(&secret, "channel-a", "epoch-1", bytes).unwrap();
    assert_eq!(
        encrypted.open(&secret, "channel-a", "epoch-1").unwrap(),
        bytes
    );
    assert!(encrypted.open(&[8; 32], "channel-a", "epoch-1").is_err());
    assert!(encrypted.open(&secret, "channel-b", "epoch-1").is_err());
    assert!(encrypted.open(&secret, "channel-a", "epoch-2").is_err());
    let other = seal_private_receive_payload(&secret, "channel-a", "epoch-1", bytes).unwrap();
    assert_ne!(other.ciphertext_hex, encrypted.ciphertext_hex);
    assert_eq!(other.epoch_key_id, encrypted.epoch_key_id);
    let encoded = encrypted.encode().unwrap();
    assert!(!String::from_utf8_lossy(&encoded).contains("channel-a"));
    assert!(!String::from_utf8_lossy(&encoded).contains("private-source-locator"));
}

#[test]
fn private_receive_payload_is_bounded_and_cannot_be_relabelled() {
    let secret = [7; 32];
    let largest = vec![42; PRIVATE_RECEIVE_PAYLOAD_MAX_PLAINTEXT_BYTES];
    let encrypted =
        seal_private_receive_payload(&secret, "channel-a", "epoch-1", &largest).unwrap();
    let wire = encrypted.encode().unwrap();
    assert!(wire.len() <= RECEIVE_PAYLOAD_MAX_BYTES);
    assert_eq!(
        PrivateReceivePayloadV1::decode(&wire)
            .unwrap()
            .open(&secret, "channel-a", "epoch-1")
            .unwrap(),
        largest
    );
    assert!(
        seal_private_receive_payload(
            &secret,
            "channel-a",
            "epoch-1",
            &vec![0; PRIVATE_RECEIVE_PAYLOAD_MAX_PLAINTEXT_BYTES + 1]
        )
        .is_err()
    );
    let mut relabelled = encrypted;
    relabelled.epoch_key_id = receive_epoch_key_id(&secret, "channel-a", "epoch-2").unwrap();
    assert!(relabelled.open(&secret, "channel-a", "epoch-2").is_err());
}
