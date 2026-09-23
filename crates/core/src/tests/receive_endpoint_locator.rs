use crate::{
    KukuriKeys, RECEIVE_ENDPOINT_LOCATOR_MAX_BYTES, ReceiveEndpointLocatorV1,
    receive_route_for_account,
};

fn keys() -> KukuriKeys {
    KukuriKeys::parse("0000000000000000000000000000000000000000000000000000000000000001").unwrap()
}

#[test]
fn locator_signature_binds_account_route_and_endpoint_without_a_time_renewal() {
    let account = keys().public_key();
    let locator = ReceiveEndpointLocatorV1::sign(&keys(), &"ab".repeat(32)).unwrap();
    assert_eq!(locator.endpoint_id, "ab".repeat(32));
    assert_eq!(locator.route, receive_route_for_account(&account).unwrap());
    locator.verify_signature_for(&account).unwrap();
    assert!(
        locator
            .verify_signature_for(&KukuriKeys::generate().public_key())
            .is_err()
    );

    let mut changed = locator.clone();
    changed.endpoint_id = "cd".repeat(32);
    assert!(changed.verify_signature_for(&account).is_err());
    let mut changed = locator.clone();
    changed.route = receive_route_for_account(&KukuriKeys::generate().public_key()).unwrap();
    assert!(changed.verify_signature_for(&account).is_err());
    let mut changed = locator.clone();
    changed.version += 1;
    assert!(changed.verify_signature_for(&account).is_err());
}

#[test]
fn locator_wire_rejects_extra_fields_and_oversize_values() {
    let locator = ReceiveEndpointLocatorV1::sign(&keys(), &"ab".repeat(32)).unwrap();
    let encoded = serde_json::to_vec(&locator).unwrap();
    assert_eq!(ReceiveEndpointLocatorV1::decode(&encoded).unwrap(), locator);
    assert!(
        ReceiveEndpointLocatorV1::decode(&vec![b' '; RECEIVE_ENDPOINT_LOCATOR_MAX_BYTES + 1])
            .is_err()
    );
    let mut value = serde_json::to_value(&locator).unwrap();
    value["unverified_address"] = serde_json::json!("127.0.0.1:1");
    assert!(ReceiveEndpointLocatorV1::decode(&serde_json::to_vec(&value).unwrap()).is_err());
    assert!(ReceiveEndpointLocatorV1::sign(&keys(), &"AB".repeat(32)).is_err());
}
