use super::*;

fn key(scope: WorkScope, object: u8, now: Instant) -> WorkKey {
    WorkKey {
        scope,
        protocol: WorkProtocol::Blob,
        object: [object; 32],
        mode: WorkMode::Fetch,
        persistence: WorkPersistence::Store,
        byte_limit: 100_000,
        deadline: now + Duration::from_secs(30),
        lane: WorkLane::Interactive,
    }
}

fn admitted(result: WorkAdmission) -> WorkWaiter {
    match result {
        WorkAdmission::Admitted(waiter) => waiter,
        other => panic!("expected admitted, got {other:?}"),
    }
}

#[test]
fn admission_bounds_requests_bytes_waiters_and_running_before_io() {
    let now = Instant::now();
    let mut owner = NetworkWorkOwner::new(WorkLimits {
        scopes: 1,
        requests: 2,
        waiters_per_request: 2,
        metadata_bytes: 6,
        running: 1,
    });
    let scope = owner.register_scope().unwrap();
    assert!(owner.register_scope().is_none());
    let first_key = key(scope, 1, now);
    let first = admitted(owner.admit(first_key, b"abc", now));
    assert!(matches!(
        owner.admit(first_key, b"abc", now),
        WorkAdmission::Joined(_)
    ));
    assert!(matches!(
        owner.admit(first_key, b"abc", now),
        WorkAdmission::Deferred { .. }
    ));
    assert_eq!(
        owner.admit(key(scope, 2, now), b"1234567", now),
        WorkAdmission::Denied(AdmissionDenied::MetadataTooLarge)
    );
    assert!(matches!(
        owner.admit(key(scope, 2, now), b"1234", now),
        WorkAdmission::Deferred { .. }
    ));
    admitted(owner.admit(key(scope, 2, now), b"def", now));
    assert!(matches!(
        owner.admit(key(scope, 3, now), b"", now),
        WorkAdmission::Deferred { .. }
    ));
    assert_eq!(
        owner.usage(),
        WorkUsage {
            scopes: 1,
            requests: 2,
            running: 0,
            stopping: 0,
            metadata_bytes: 6,
            waiters: 3
        }
    );
    let running = owner.start_next(now).unwrap();
    assert_eq!(running.id, first.work_id());
    assert_eq!(&*running.metadata, b"abc");
    assert!(owner.start_next(now).is_none());
    // Dispatched metadata still consumes the budget until I/O finishes.
    assert_eq!(owner.usage().metadata_bytes, 6);
    assert_eq!(owner.complete(running.id, now), WorkCompletion::Publish);
    assert_eq!(owner.usage().metadata_bytes, 3);
    assert!(owner.start_next(now).is_some());
}

#[test]
fn unchanged_demand_joins_but_policy_scope_and_deadline_do_not() {
    let now = Instant::now();
    let mut owner = NetworkWorkOwner::new(WorkLimits::default());
    let scope = owner.register_scope().unwrap();
    let original = key(scope, 1, now);
    let first = admitted(owner.admit(original, b"target", now));
    let WorkAdmission::Joined(second) = owner.admit(original, b"target", now) else {
        panic!("must join")
    };
    assert_eq!(first.work_id(), second.work_id());
    assert_eq!(
        owner.admit(original, b"different target", now),
        WorkAdmission::Denied(AdmissionDenied::ConflictingMetadata)
    );
    let mut variants = vec![];
    let mut changed = original;
    changed.scope = owner.register_scope().unwrap();
    variants.push(changed);
    changed = original;
    changed.persistence = WorkPersistence::Ephemeral;
    variants.push(changed);
    changed = original;
    changed.byte_limit += 1;
    variants.push(changed);
    changed = original;
    changed.mode = WorkMode::Display;
    variants.push(changed);
    changed = original;
    changed.protocol = WorkProtocol::Docs;
    variants.push(changed);
    changed = original;
    changed.deadline += Duration::from_secs(1);
    variants.push(changed);
    for candidate in variants {
        assert_ne!(
            admitted(owner.admit(candidate, b"target", now)).work_id(),
            first.work_id()
        );
    }
    changed.mode = WorkMode::LocalOnly;
    assert_eq!(
        owner.admit(changed, b"target", now),
        WorkAdmission::Denied(AdmissionDenied::LocalOnly)
    );
    let mut starts = 0;
    while owner.start_next(now).is_some() {
        starts += 1;
    }
    assert_eq!(starts, 7, "unchanged demand adds no execution");
}

#[test]
fn queued_deadline_includes_waiting_time_and_reclaims_all_indexes() {
    let now = Instant::now();
    let mut owner = NetworkWorkOwner::new(WorkLimits::default());
    let scope = owner.register_scope().unwrap();
    let request = key(scope, 1, now);
    admitted(owner.admit(request, b"bytes", now));
    assert_eq!(owner.next_deadline(), Some(request.deadline));
    assert!(
        owner.start_next(request.deadline).is_none(),
        "expired queued work must never reach I/O"
    );
    assert_eq!(owner.usage().requests, 0);
    assert_eq!(owner.usage().metadata_bytes, 0);
    assert_eq!(owner.next_deadline(), None);
    assert_eq!(
        owner.admit(request, b"bytes", request.deadline),
        WorkAdmission::Denied(AdmissionDenied::Expired)
    );
    assert_eq!(
        owner.next_cancellation(),
        None,
        "queued work requires no I/O stop"
    );
}

#[test]
fn lanes_are_weighted_and_empty_lanes_lend_capacity() {
    let now = Instant::now();
    let mut owner = NetworkWorkOwner::new(WorkLimits {
        running: 1,
        ..WorkLimits::default()
    });
    let scope = owner.register_scope().unwrap();
    for (lane_index, lane) in [WorkLane::Interactive, WorkLane::Send, WorkLane::Background]
        .into_iter()
        .enumerate()
    {
        for index in 0..8 {
            let mut request = key(scope, (lane_index * 8 + index) as u8, now);
            request.lane = lane;
            admitted(owner.admit(request, b"", now));
        }
    }
    let mut selected = vec![];
    for _ in 0..14 {
        let next = owner.start_next(now).unwrap();
        selected.push(next.key.lane.index());
        assert_eq!(owner.complete(next.id, now), WorkCompletion::Publish);
    }
    assert_eq!(selected, LANES.repeat(2));
    // Interactive is empty. Remaining non-empty lanes still make progress.
    for _ in 0..10 {
        let next = owner.start_next(now).unwrap();
        assert_ne!(next.key.lane, WorkLane::Interactive);
        owner.complete(next.id, now);
    }
    assert!(owner.start_next(now).is_none());
}

#[test]
fn display_cancel_stops_io_but_normal_waiter_cancel_preserves_owned_result() {
    let now = Instant::now();
    let mut owner = NetworkWorkOwner::new(WorkLimits {
        running: 1,
        ..WorkLimits::default()
    });
    let scope = owner.register_scope().unwrap();
    let mut display = key(scope, 1, now);
    display.mode = WorkMode::Display;
    let first = admitted(owner.admit(display, b"", now));
    let WorkAdmission::Joined(second) = owner.admit(display, b"", now) else {
        panic!("must join")
    };
    let running = owner.start_next(now).unwrap();
    owner.release_waiter(first);
    assert_eq!(owner.next_cancellation(), None);
    owner.release_waiter(first); // An old/duplicate drop cannot remove another observer.
    assert_eq!(owner.next_cancellation(), None);
    owner.release_waiter(second);
    assert_eq!(owner.next_cancellation(), Some(running.id));
    assert_eq!(owner.next_cancellation(), None);
    let ordinary = admitted(owner.admit(key(scope, 2, now), b"", now));
    assert!(
        owner.start_next(now).is_none(),
        "stopping I/O still owns its slot"
    );
    assert_eq!(owner.complete(running.id, now), WorkCompletion::Discard);
    let running = owner.start_next(now).unwrap();
    owner.release_waiter(ordinary);
    assert_eq!(owner.next_cancellation(), None);
    assert_eq!(owner.complete(running.id, now), WorkCompletion::Publish);
    assert_eq!(owner.usage().requests, 0);
}

#[test]
fn revoked_scope_cannot_publish_or_join_replacement_and_other_scope_survives() {
    let now = Instant::now();
    let mut owner = NetworkWorkOwner::new(WorkLimits::default());
    let revoked = owner.register_scope().unwrap();
    let retained = owner.register_scope().unwrap();
    let old_key = key(revoked, 1, now);
    admitted(owner.admit(old_key, b"", now));
    let running = owner.start_next(now).unwrap();
    admitted(owner.admit(key(revoked, 2, now), b"", now));
    let retained_work = admitted(owner.admit(key(retained, 3, now), b"", now));
    owner.revoke_scope(revoked);
    assert_eq!(
        owner.usage().requests,
        2,
        "queued revoked work is removed immediately"
    );
    assert_eq!(owner.next_cancellation(), Some(running.id));
    assert_eq!(
        owner.admit(old_key, b"", now),
        WorkAdmission::Denied(AdmissionDenied::InactiveScope)
    );
    let replacement = owner.register_scope().unwrap();
    assert_ne!(replacement, revoked);
    admitted(owner.admit(key(replacement, 1, now), b"", now));
    assert_eq!(owner.complete(running.id, now), WorkCompletion::Discard);
    assert_eq!(owner.start_next(now).unwrap().id, retained_work.work_id());
    assert_eq!(owner.complete(running.id, now), WorkCompletion::Unknown);
}

#[test]
fn running_expiry_keeps_capacity_until_ack_and_late_completion_is_discarded() {
    let now = Instant::now();
    let mut owner = NetworkWorkOwner::new(WorkLimits {
        requests: 1,
        ..WorkLimits::default()
    });
    let scope = owner.register_scope().unwrap();
    let request = key(scope, 1, now);
    admitted(owner.admit(request, b"data", now));
    let running = owner.start_next(now).unwrap();
    owner.expire(request.deadline);
    assert_eq!(owner.usage().stopping, 1);
    assert_eq!(owner.usage().metadata_bytes, 4);
    assert_eq!(owner.next_cancellation(), Some(running.id));
    assert!(
        matches!(owner.admit(key(scope, 2, request.deadline), b"", request.deadline), WorkAdmission::Deferred { retry_at } if retry_at > request.deadline)
    );
    assert_eq!(
        owner.complete(running.id, request.deadline),
        WorkCompletion::Discard
    );
    // Completion itself checks expiry even without a preceding timer tick.
    admitted(owner.admit(key(scope, 3, now), b"", now));
    let running = owner.start_next(now).unwrap();
    assert_eq!(
        owner.complete(running.id, request.deadline),
        WorkCompletion::Discard
    );
    assert_eq!(owner.usage().requests, 0);
}

#[test]
fn foreign_owner_tokens_and_completions_cannot_change_current_work() {
    let now = Instant::now();
    let mut first = NetworkWorkOwner::new(WorkLimits::default());
    let mut second = NetworkWorkOwner::new(WorkLimits::default());
    let first_scope = first.register_scope().unwrap();
    let second_scope = second.register_scope().unwrap();
    let first_waiter = admitted(first.admit(key(first_scope, 1, now), b"", now));
    admitted(second.admit(key(second_scope, 1, now), b"", now));
    let first_work = first.start_next(now).unwrap();
    let second_work = second.start_next(now).unwrap();
    assert_eq!(
        second.admit(key(first_scope, 1, now), b"", now),
        WorkAdmission::Denied(AdmissionDenied::InactiveScope)
    );
    second.release_waiter(first_waiter);
    second.revoke_scope(first_scope);
    assert_eq!(second.complete(first_work.id, now), WorkCompletion::Unknown);
    assert_eq!(second.usage().waiters, 1);
    assert_eq!(
        second.complete(second_work.id, now),
        WorkCompletion::Publish
    );
}

#[test]
fn tenfold_registration_and_cancellation_history_leaves_no_retired_records() {
    let now = Instant::now();
    for history in [100, 1000] {
        let mut owner = NetworkWorkOwner::new(WorkLimits {
            scopes: 1,
            requests: 2,
            ..WorkLimits::default()
        });
        for _ in 0..history {
            let scope = owner.register_scope().unwrap();
            admitted(owner.admit(key(scope, 1, now), b"metadata", now));
            let running = owner.start_next(now).unwrap();
            let queued = admitted(owner.admit(key(scope, 2, now), b"queued", now));
            owner.release_waiter(queued);
            owner.revoke_scope(scope);
            assert_eq!(owner.next_cancellation(), Some(running.id));
            assert_eq!(owner.complete(running.id, now), WorkCompletion::Discard);
            assert_eq!(owner.usage(), WorkUsage::default());
            assert!(owner.keys.is_empty());
            assert!(owner.deadlines.is_empty());
            assert!(owner.queues.iter().all(BTreeSet::is_empty));
            assert!(owner.cancellations.is_empty());
        }
    }
}
