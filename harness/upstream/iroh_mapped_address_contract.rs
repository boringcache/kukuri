// Included only in the pinned upstream crate's test configuration.
use super::tests::make_remote_map;
use super::*;
use iroh_base::SecretKey;

#[test]
fn kukuri_mapped_address_history_stays_bounded() {
    let mut retained_counts = Vec::new();
    for history in [100u64, 1_000] {
        let (mut remote_map, _shutdown, _guards) = make_remote_map();
        let active = SecretKey::from_bytes(&[255; 32]).public();
        let active_addr = remote_map.mapped_addrs.endpoint_addrs.get(&active);
        let relay: RelayUrl = "https://relay.example.invalid".parse().unwrap();
        let active_relay_addr = remote_map
            .mapped_addrs
            .relay_addrs
            .get(&(relay.clone(), active));
        let mut past = Vec::new();
        for index in 0..history {
            let mut seed = [0; 32];
            seed[..8].copy_from_slice(&index.to_be_bytes());
            let peer = SecretKey::from_bytes(&seed).public();
            let addr = remote_map.mapped_addrs.endpoint_addrs.get(&peer);
            let relay_addr = remote_map
                .mapped_addrs
                .relay_addrs
                .get(&(relay.clone(), peer));
            // The production cleanup callback receives an empty remainder when
            // a remote actor finished. No actor creation or I/O is needed here.
            assert!(remote_map.remove_or_restart_actor(peer, Vec::new()));
            past.push((peer, addr, relay_addr));
        }
        let retained = 1 + past
            .iter()
            .filter(|(_, addr, _)| {
                remote_map
                    .mapped_addrs
                    .endpoint_addrs
                    .lookup(addr)
                    .is_some()
            })
            .count();
        retained_counts.push(retained);
        assert_eq!(retained, 64, "retired history must not remain allocated");
        assert_eq!(
            remote_map.mapped_addrs.endpoint_addrs.lookup(&active_addr),
            Some(active)
        );
        assert_eq!(
            remote_map
                .mapped_addrs
                .relay_addrs
                .lookup(&active_relay_addr),
            Some((relay.clone(), active))
        );
        for (peer, addr, relay_addr) in &past {
            if remote_map
                .mapped_addrs
                .endpoint_addrs
                .lookup(addr)
                .is_none()
            {
                assert!(
                    remote_map
                        .mapped_addrs
                        .relay_addrs
                        .lookup(relay_addr)
                        .is_none(),
                    "retired relay reverse entry survived for {peer}"
                );
            }
        }
        let (last, old_addr, _) = past.last().unwrap();
        let reopened = remote_map.mapped_addrs.endpoint_addrs.get(last);
        assert_ne!(reopened, *old_addr, "a retired address must not be reused");
        assert_eq!(
            remote_map.mapped_addrs.endpoint_addrs.lookup(&reopened),
            Some(*last)
        );
    }
    assert_eq!(retained_counts, [64, 64]);
}
