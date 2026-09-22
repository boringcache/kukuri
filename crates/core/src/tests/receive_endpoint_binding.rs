use crate::{
    KukuriKeys, RECEIVE_ENDPOINT_BINDING_MAX_BYTES, RECEIVE_ENDPOINT_BINDING_MAX_LIFETIME_MS,
    ReceiveEndpointBindingV1, receive_route_for_account,
};

const NOW: i64 = 1_790_000_000_000;

fn keys() -> KukuriKeys {
    KukuriKeys::parse("0000000000000000000000000000000000000000000000000000000000000001").unwrap()
}

fn binding() -> ReceiveEndpointBindingV1 {
    ReceiveEndpointBindingV1::sign(&keys(), &"ab".repeat(32), NOW, NOW + 60_000).unwrap()
}

#[test]
fn receive_endpoint_binding_requires_account_and_connected_endpoint() {
    let binding = binding();
    let verified = binding
        .verify_for(&keys().public_key(), &"ab".repeat(32), NOW)
        .unwrap();
    assert_eq!(verified.account(), &keys().public_key());
    assert_eq!(verified.endpoint_id(), "ab".repeat(32));
    assert_eq!(
        verified.route(),
        &receive_route_for_account(&keys().public_key()).unwrap()
    );
    assert!(
        binding
            .verify_for(&KukuriKeys::generate().public_key(), &"ab".repeat(32), NOW)
            .is_err()
    );
    assert!(
        binding
            .verify_for(&keys().public_key(), &"cd".repeat(32), NOW)
            .is_err()
    );
}

#[test]
fn receive_endpoint_binding_covers_every_field_with_signature() {
    let original = binding();
    let mut changes = Vec::new();
    let mut changed = original.clone();
    changed.account = KukuriKeys::generate().public_key();
    changes.push(changed);
    let mut changed = original.clone();
    changed.endpoint_id = "cd".repeat(32);
    changes.push(changed);
    let mut changed = original.clone();
    changed.route = receive_route_for_account(&KukuriKeys::generate().public_key()).unwrap();
    changes.push(changed);
    let mut changed = original.clone();
    changed.issued_at_ms -= 1;
    changes.push(changed);
    let mut changed = original.clone();
    changed.expires_at_ms += 1;
    changes.push(changed);
    let mut changed = original.clone();
    changed.version = 2;
    changes.push(changed);
    for changed in changes {
        assert!(
            changed
                .verify_for(&changed.account, &changed.endpoint_id, NOW)
                .is_err()
        );
    }
}

#[test]
fn receive_endpoint_binding_expires_and_rejects_invalid_intervals() {
    let binding = binding();
    assert!(
        binding
            .verify_for(&binding.account, &binding.endpoint_id, NOW + 59_999)
            .is_ok()
    );
    assert!(
        binding
            .verify_for(&binding.account, &binding.endpoint_id, NOW + 60_000)
            .is_err()
    );
    assert!(
        binding
            .verify_for(&binding.account, &binding.endpoint_id, NOW - 60_001)
            .is_err()
    );
    for (issued, expires) in [
        (-1, NOW),
        (NOW, NOW),
        (NOW + 1, NOW),
        (0, i64::MAX),
        (NOW, NOW + RECEIVE_ENDPOINT_BINDING_MAX_LIFETIME_MS + 1),
    ] {
        assert!(
            ReceiveEndpointBindingV1::sign(&keys(), &"ab".repeat(32), issued, expires).is_err()
        );
    }
}

#[test]
fn receive_endpoint_binding_decode_is_bounded_and_strict() {
    let original = binding();
    let bytes = serde_json::to_vec(&original).unwrap();
    assert!(bytes.len() < RECEIVE_ENDPOINT_BINDING_MAX_BYTES);
    let decoded = ReceiveEndpointBindingV1::decode(&bytes).unwrap();
    assert_eq!(decoded, original);
    assert!(
        ReceiveEndpointBindingV1::decode(&vec![b' '; RECEIVE_ENDPOINT_BINDING_MAX_BYTES + 1])
            .is_err()
    );
    let mut value = serde_json::to_value(&original).unwrap();
    value["unrecognized"] = true.into();
    assert!(ReceiveEndpointBindingV1::decode(&serde_json::to_vec(&value).unwrap()).is_err());
    let mut invalid = original.clone();
    invalid.endpoint_id = "AB".repeat(32);
    assert!(
        invalid
            .verify_for(&invalid.account, &invalid.endpoint_id, NOW)
            .is_err()
    );
    assert!(ReceiveEndpointBindingV1::sign(&keys(), &"ab".repeat(33), NOW, NOW + 1).is_err());
}

#[test]
fn receive_endpoint_binding_allows_multiple_devices_without_changing_route() {
    let first = binding();
    let second =
        ReceiveEndpointBindingV1::sign(&keys(), &"cd".repeat(32), NOW, NOW + 60_000).unwrap();
    assert_eq!(first.route, second.route);
    assert!(
        first
            .verify_for(&first.account, &first.endpoint_id, NOW)
            .is_ok()
    );
    assert!(
        second
            .verify_for(&second.account, &second.endpoint_id, NOW)
            .is_ok()
    );
    assert_ne!(
        first.route,
        receive_route_for_account(&KukuriKeys::generate().public_key()).unwrap()
    );
    assert!(receive_route_for_account(&"not-a-public-key".into()).is_err());
}
