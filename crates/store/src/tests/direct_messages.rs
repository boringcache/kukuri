use super::*;

async fn assert_due_outbox_lanes_remain_bounded_during_new_inserts<S: DirectMessageStore>(
    store: &S,
) {
    for index in 0..1_000 {
        DirectMessageStore::put_direct_message_outbox(
            store,
            DirectMessageOutboxRow {
                dm_id: format!("new-dm-{}", index % 17),
                message_id: format!("new-{index:04}"),
                peer_pubkey: format!("peer-{}", index % 17),
                frame_blob_hash: BlobHash::new("new-hash"),
                created_at: 42,
                last_attempt_at: None,
            },
        )
        .await
        .unwrap();
    }
    for index in 0..130 {
        DirectMessageStore::put_direct_message_outbox(
            store,
            DirectMessageOutboxRow {
                dm_id: format!("old-dm-{}", index % 17),
                message_id: format!("old-{index:04}"),
                peer_pubkey: format!("peer-{}", index % 17),
                frame_blob_hash: BlobHash::new("old-hash"),
                created_at: 1,
                last_attempt_at: Some(0),
            },
        )
        .await
        .unwrap();
    }
    assert!(
        DirectMessageStore::list_due_direct_message_outbox(store, 98, 4, 1)
            .await
            .is_err()
    );
    assert!(
        DirectMessageStore::list_due_direct_message_outbox(store, 98, 3, 2)
            .await
            .is_err()
    );
    let mut old_seen = std::collections::BTreeSet::new();
    for tick in 0..130 {
        let rows = DirectMessageStore::list_due_direct_message_outbox(store, 98, 3, 1)
            .await
            .unwrap();
        assert_eq!(rows.len(), 4);
        assert_eq!(
            rows.iter()
                .filter(|row| row.last_attempt_at.is_none())
                .count(),
            3
        );
        let retry = rows
            .iter()
            .find(|row| row.last_attempt_at.is_some())
            .unwrap();
        old_seen.insert(retry.message_id.clone());
        for row in rows {
            DirectMessageStore::touch_direct_message_outbox_attempt(
                store,
                &row.dm_id,
                &row.message_id,
                100,
            )
            .await
            .unwrap();
        }
        for insert in 0..3 {
            DirectMessageStore::put_direct_message_outbox(
                store,
                DirectMessageOutboxRow {
                    dm_id: "continuous-new".into(),
                    message_id: format!("later-{tick:03}-{insert}"),
                    peer_pubkey: "later-peer".into(),
                    frame_blob_hash: BlobHash::new("later-hash"),
                    created_at: 200,
                    last_attempt_at: None,
                },
            )
            .await
            .unwrap();
        }
    }
    assert_eq!(
        old_seen.len(),
        130,
        "continuous new rows cannot starve retries"
    );
}

#[tokio::test]
async fn due_dm_outbox_lanes_keep_new_and_old_work_bounded() {
    let memory = MemoryStore::default();
    assert_due_outbox_lanes_remain_bounded_during_new_inserts(&memory).await;
    let sqlite = SqliteStore::connect_memory().await.unwrap();
    assert_due_outbox_lanes_remain_bounded_during_new_inserts(&sqlite).await;
}

async fn assert_candidate_cursor_reaches_old_rows_during_new_inserts<S: DirectMessageStore>(
    store: &S,
) {
    for index in 0..15 {
        DirectMessageStore::put_direct_message_outbox(
            store,
            DirectMessageOutboxRow {
                dm_id: format!("candidate-dm-{index:02}"),
                message_id: format!("candidate-old-{index:02}"),
                peer_pubkey: format!("candidate-peer-{index:02}"),
                frame_blob_hash: BlobHash::new("candidate-hash"),
                created_at: 1,
                last_attempt_at: Some(index as i64),
            },
        )
        .await
        .unwrap();
    }
    let mut after = None;
    let mut cycle_end = None;
    let mut seen = std::collections::BTreeSet::new();
    for tick in 0..4 {
        let page = DirectMessageStore::list_direct_message_outbox_candidate_page(
            store,
            after.as_ref(),
            cycle_end.as_ref(),
            4,
        )
        .await
        .unwrap();
        assert!(page.items.len() <= 4);
        seen.extend(page.items.iter().map(|row| row.peer_pubkey.clone()));
        DirectMessageStore::put_direct_message_outbox(
            store,
            DirectMessageOutboxRow {
                dm_id: format!("candidate-new-dm-{tick}"),
                message_id: format!("candidate-new-{tick}"),
                peer_pubkey: format!("candidate-new-peer-{tick}"),
                frame_blob_hash: BlobHash::new("candidate-hash"),
                created_at: 100 + tick,
                last_attempt_at: None,
            },
        )
        .await
        .unwrap();
        after = page.next_cursor;
        cycle_end = after.as_ref().and(page.cycle_end);
    }
    assert_eq!(
        seen.len(),
        15,
        "new rows must not starve the cycle's old tail"
    );
    assert!(seen.iter().all(|peer| peer.starts_with("candidate-peer-")));
    let next = DirectMessageStore::list_direct_message_outbox_candidate_page(store, None, None, 4)
        .await
        .unwrap();
    assert_eq!(next.items.len(), 4);
}

#[tokio::test]
async fn account_candidate_pages_advance_independently_of_retry_attempts() {
    let memory = MemoryStore::default();
    assert_candidate_cursor_reaches_old_rows_during_new_inserts(&memory).await;
    let sqlite = SqliteStore::connect_memory().await.unwrap();
    assert_candidate_cursor_reaches_old_rows_during_new_inserts(&sqlite).await;
}

async fn assert_due_indexes_forget_removed_protected_rows<S: DirectMessageStore>(store: &S) {
    for (dm_id, message_id, attempted_at) in [
        ("dm-new", "new", None),
        ("dm-retry", "retry", Some(0)),
        ("dm-clear", "clear", None),
    ] {
        DirectMessageStore::put_direct_message_outbox(
            store,
            DirectMessageOutboxRow {
                dm_id: dm_id.into(),
                message_id: message_id.into(),
                peer_pubkey: "peer".into(),
                frame_blob_hash: BlobHash::new("hash"),
                created_at: 42,
                last_attempt_at: attempted_at,
            },
        )
        .await
        .unwrap();
    }
    assert_eq!(
        DirectMessageStore::list_due_direct_message_outbox(store, 98, 3, 1)
            .await
            .unwrap()
            .len(),
        3
    );
    DirectMessageStore::touch_direct_message_outbox_attempt(store, "dm-new", "new", 100)
        .await
        .unwrap();
    DirectMessageStore::remove_direct_message_outbox(store, "dm-retry", "retry")
        .await
        .unwrap();
    DirectMessageStore::clear_direct_message_local(store, "dm-clear")
        .await
        .unwrap();
    assert!(
        DirectMessageStore::list_due_direct_message_outbox(store, 98, 3, 1)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn due_outbox_indexes_forget_ack_and_local_clear() {
    let memory = MemoryStore::default();
    assert_due_indexes_forget_removed_protected_rows(&memory).await;
    let sqlite = SqliteStore::connect_memory().await.unwrap();
    assert_due_indexes_forget_removed_protected_rows(&sqlite).await;
}

async fn assert_peer_outbox_pages_are_bounded<S: DirectMessageStore>(store: &S) {
    let target = "a".repeat(64);
    let other = "b".repeat(64);
    for index in 0..1_000 {
        DirectMessageStore::put_direct_message_outbox(
            store,
            DirectMessageOutboxRow {
                dm_id: "dm-other".into(),
                message_id: format!("other-{index:04}"),
                peer_pubkey: other.clone(),
                frame_blob_hash: BlobHash::new("other-hash"),
                created_at: 42,
                last_attempt_at: None,
            },
        )
        .await
        .unwrap();
    }
    for index in 0..130 {
        DirectMessageStore::put_direct_message_outbox(
            store,
            DirectMessageOutboxRow {
                dm_id: "dm-target".into(),
                message_id: format!("target-{index:04}"),
                peer_pubkey: target.clone(),
                frame_blob_hash: BlobHash::new("target-hash"),
                created_at: 42,
                last_attempt_at: None,
            },
        )
        .await
        .unwrap();
    }
    assert!(
        DirectMessageStore::list_direct_message_outbox_for_peer_page(store, &target, None, None, 0)
            .await
            .is_err()
    );
    assert!(
        DirectMessageStore::list_direct_message_outbox_for_peer_page(
            store, &target, None, None, 65
        )
        .await
        .is_err()
    );
    let mut cursor = None;
    let mut cycle_end = None;
    let mut observed = Vec::new();
    for expected_len in [64, 64, 2] {
        let page = DirectMessageStore::list_direct_message_outbox_for_peer_page(
            store,
            &target,
            cursor.as_ref(),
            cycle_end.as_ref(),
            DIRECT_MESSAGE_OUTBOX_PAGE_LIMIT,
        )
        .await
        .unwrap();
        assert_eq!(page.items.len(), expected_len);
        assert!(page.items.iter().all(|row| row.peer_pubkey == target));
        observed.extend(page.items.iter().map(|row| row.message_id.clone()));
        cycle_end = page.cycle_end;
        cursor = page.next_cursor;
    }
    assert!(cursor.is_none());
    assert_eq!(observed.len(), 130);
    assert_eq!(observed[0], "target-0000");
    assert_eq!(observed[129], "target-0129");
    DirectMessageStore::remove_direct_message_outbox(store, "dm-target", "target-0000")
        .await
        .unwrap();
    let after_remove = DirectMessageStore::list_direct_message_outbox_for_peer_page(
        store, &target, None, None, 64,
    )
    .await
    .unwrap();
    assert_eq!(after_remove.items[0].message_id, "target-0001");
    DirectMessageStore::clear_direct_message_local(store, "dm-target")
        .await
        .unwrap();
    let after_clear = DirectMessageStore::list_direct_message_outbox_for_peer_page(
        store, &target, None, None, 64,
    )
    .await
    .unwrap();
    assert!(after_clear.items.is_empty());
    for dm_id in ["dm-tie-a", "dm-tie-b"] {
        DirectMessageStore::put_direct_message_outbox(
            store,
            DirectMessageOutboxRow {
                dm_id: dm_id.into(),
                message_id: "same-message".into(),
                peer_pubkey: target.clone(),
                frame_blob_hash: BlobHash::new("tie-hash"),
                created_at: 42,
                last_attempt_at: None,
            },
        )
        .await
        .unwrap();
    }
    let tie_first =
        DirectMessageStore::list_direct_message_outbox_for_peer_page(store, &target, None, None, 1)
            .await
            .unwrap();
    let tie_second = DirectMessageStore::list_direct_message_outbox_for_peer_page(
        store,
        &target,
        tie_first.next_cursor.as_ref(),
        tie_first.cycle_end.as_ref(),
        1,
    )
    .await
    .unwrap();
    assert_eq!(tie_first.items[0].dm_id, "dm-tie-a");
    assert_eq!(tie_second.items[0].dm_id, "dm-tie-b");
    assert!(tie_second.next_cursor.is_none());
    let unrelated_page =
        DirectMessageStore::list_direct_message_outbox_for_peer_page(store, &other, None, None, 64)
            .await
            .unwrap();
    assert_eq!(unrelated_page.items.len(), 64);
}

#[tokio::test]
async fn direct_message_outbox_peer_pages_ignore_other_peer_history() {
    let memory = MemoryStore::default();
    assert_peer_outbox_pages_are_bounded(&memory).await;
    let sqlite = SqliteStore::connect_memory().await.unwrap();
    assert_peer_outbox_pages_are_bounded(&sqlite).await;
}

async fn assert_new_rows_cannot_extend_an_existing_outbox_cycle<S: DirectMessageStore>(store: &S) {
    let peer = "c".repeat(64);
    for index in 0..130 {
        DirectMessageStore::put_direct_message_outbox(
            store,
            DirectMessageOutboxRow {
                dm_id: "dm-cycle".into(),
                message_id: format!("original-{index:04}"),
                peer_pubkey: peer.clone(),
                frame_blob_hash: BlobHash::new("cycle-hash"),
                created_at: 42,
                last_attempt_at: None,
            },
        )
        .await
        .unwrap();
    }
    let mut cursor = None;
    let mut cycle_end = None;
    for page_index in 0..3 {
        let page = DirectMessageStore::list_direct_message_outbox_for_peer_page(
            store,
            &peer,
            cursor.as_ref(),
            cycle_end.as_ref(),
            DIRECT_MESSAGE_OUTBOX_PAGE_LIMIT,
        )
        .await
        .unwrap();
        assert_eq!(page.items.len(), [64, 64, 2][page_index]);
        cycle_end = page.cycle_end;
        cursor = page.next_cursor;
        if page_index < 2 {
            for index in 0..64 {
                DirectMessageStore::put_direct_message_outbox(
                    store,
                    DirectMessageOutboxRow {
                        dm_id: "dm-cycle".into(),
                        message_id: format!("fresh-{page_index}-{index:04}"),
                        peer_pubkey: peer.clone(),
                        frame_blob_hash: BlobHash::new("cycle-hash"),
                        created_at: 43 + page_index as i64,
                        last_attempt_at: None,
                    },
                )
                .await
                .unwrap();
            }
        }
    }
    assert!(
        cursor.is_none(),
        "new rows must not keep a prior cycle open"
    );
    let restarted = DirectMessageStore::list_direct_message_outbox_for_peer_page(
        store,
        &peer,
        None,
        None,
        DIRECT_MESSAGE_OUTBOX_PAGE_LIMIT,
    )
    .await
    .unwrap();
    assert_eq!(restarted.items[0].message_id, "original-0000");
}

#[tokio::test]
async fn new_outbox_rows_cannot_starve_older_unacked_rows() {
    let memory = MemoryStore::default();
    assert_new_rows_cannot_extend_an_existing_outbox_cycle(&memory).await;
    let sqlite = SqliteStore::connect_memory().await.unwrap();
    assert_new_rows_cannot_extend_an_existing_outbox_cycle(&sqlite).await;
}

#[tokio::test]
async fn direct_message_outbox_peer_page_uses_the_sqlite_cursor_index() {
    use sqlx::Row;
    let store = SqliteStore::connect_memory().await.unwrap();
    let plan = sqlx::query(
        "EXPLAIN QUERY PLAN SELECT dm_id, message_id, peer_pubkey, frame_blob_hash, created_at, last_attempt_at \
         FROM dm_outbox WHERE peer_pubkey = ?1 AND (created_at, message_id, dm_id) > (?2, ?3, ?4) \
         AND (created_at, message_id, dm_id) <= (?5, ?6, ?7) \
         ORDER BY created_at ASC, message_id ASC, dm_id ASC LIMIT ?8",
    )
    .bind("a".repeat(64))
    .bind(42_i64)
    .bind("target-0000")
    .bind("dm-target")
    .bind(42_i64)
    .bind("target-0129")
    .bind("dm-target")
    .bind(65_i64)
    .fetch_all(store.pool())
    .await
    .unwrap();
    let details = plan
        .iter()
        .map(|row| row.get::<String, _>("detail"))
        .collect::<Vec<_>>();
    assert!(
        details
            .iter()
            .any(|detail| detail.contains("idx_dm_outbox_peer_cursor")),
        "peer cursor query must use its index: {details:?}"
    );
}

#[tokio::test]
async fn due_direct_message_outbox_uses_both_sqlite_lane_indexes() {
    use sqlx::Row;
    let store = SqliteStore::connect_memory().await.unwrap();
    for (query, expected) in [
        (
            "EXPLAIN QUERY PLAN SELECT dm_id FROM dm_outbox WHERE last_attempt_at IS NULL \
             ORDER BY created_at, message_id, dm_id, peer_pubkey LIMIT 3",
            "idx_dm_outbox_never_attempted",
        ),
        (
            "EXPLAIN QUERY PLAN SELECT dm_id FROM dm_outbox WHERE last_attempt_at IS NOT NULL \
             AND last_attempt_at <= 98 ORDER BY last_attempt_at, created_at, message_id, dm_id, peer_pubkey LIMIT 1",
            "idx_dm_outbox_attempted_due",
        ),
    ] {
        let details = sqlx::query(query)
            .fetch_all(store.pool())
            .await
            .unwrap()
            .into_iter()
            .map(|row| row.get::<String, _>("detail"))
            .collect::<Vec<_>>();
        assert!(
            details.iter().any(|detail| detail.contains(expected)),
            "{details:?}"
        );
    }
}

#[tokio::test]
async fn account_candidate_page_uses_stable_sqlite_cursor_index() {
    use sqlx::Row;
    let store = SqliteStore::connect_memory().await.unwrap();
    let plan = sqlx::query(
        "EXPLAIN QUERY PLAN SELECT dm_id FROM dm_outbox \
         WHERE (created_at, message_id, dm_id) > (1, 'a', 'a') \
         AND (created_at, message_id, dm_id) <= (999, 'z', 'z') \
         ORDER BY created_at, message_id, dm_id LIMIT 5",
    )
    .fetch_all(store.pool())
    .await
    .unwrap();
    let details = plan
        .iter()
        .map(|row| row.get::<String, _>("detail"))
        .collect::<Vec<_>>();
    assert!(
        details
            .iter()
            .any(|detail| detail.contains("idx_dm_outbox_candidate_cursor")),
        "candidate cursor must use its index: {details:?}"
    );
    assert!(
        details
            .iter()
            .all(|detail| !detail.contains("USE TEMP B-TREE")),
        "candidate page must not sort the full outbox: {details:?}"
    );
}

#[tokio::test]
async fn direct_message_delete_clears_outbox_but_keeps_tombstone() {
    let store = SqliteStore::connect_memory().await.expect("sqlite store");
    let dm_id = "dm-test";
    DirectMessageStore::upsert_direct_message_conversation(
        &store,
        DirectMessageConversationRow {
            dm_id: dm_id.into(),
            peer_pubkey: "b".repeat(64),
            updated_at: 10,
            last_message_at: Some(10),
            last_message_id: Some("message-1".into()),
            last_message_preview: Some("queued".into()),
        },
    )
    .await
    .expect("upsert conversation");
    DirectMessageStore::put_direct_message_message(
        &store,
        DirectMessageMessageRow {
            dm_id: dm_id.into(),
            message_id: "message-1".into(),
            sender_pubkey: "a".repeat(64),
            recipient_pubkey: "b".repeat(64),
            created_at: 10,
            text: Some("queued".into()),
            reply_to_message_id: None,
            attachment_manifest: None,
            outgoing: true,
            acked_at: None,
        },
    )
    .await
    .expect("put message");
    DirectMessageStore::put_direct_message_outbox(
        &store,
        DirectMessageOutboxRow {
            dm_id: dm_id.into(),
            message_id: "message-1".into(),
            peer_pubkey: "b".repeat(64),
            frame_blob_hash: BlobHash::new("frame-hash"),
            created_at: 10,
            last_attempt_at: None,
        },
    )
    .await
    .expect("put outbox");
    DirectMessageStore::put_direct_message_tombstone(
        &store,
        DirectMessageTombstoneRow {
            dm_id: dm_id.into(),
            message_id: "message-1".into(),
            deleted_at: 20,
        },
    )
    .await
    .expect("put tombstone");
    DirectMessageStore::delete_direct_message_message_local(&store, dm_id, "message-1")
        .await
        .expect("delete message");
    DirectMessageStore::clear_direct_message_local(&store, dm_id)
        .await
        .expect("clear conversation");

    assert!(
        DirectMessageStore::get_direct_message_outbox(&store, dm_id, "message-1")
            .await
            .expect("get outbox")
            .is_none()
    );
    assert!(
        DirectMessageStore::get_direct_message_conversation_by_dm_id(&store, dm_id)
            .await
            .expect("get conversation")
            .is_none()
    );
    assert!(
        DirectMessageStore::has_direct_message_tombstone(&store, dm_id, "message-1")
            .await
            .expect("has tombstone")
    );
}
#[tokio::test]
async fn direct_message_local_delete_prevents_duplicate_reinsert() {
    let store = SqliteStore::connect_memory().await.expect("sqlite store");
    let dm_id = "dm-test";
    let message = DirectMessageMessageRow {
        dm_id: dm_id.into(),
        message_id: "message-1".into(),
        sender_pubkey: "a".repeat(64),
        recipient_pubkey: "b".repeat(64),
        created_at: 10,
        text: Some("hello".into()),
        reply_to_message_id: None,
        attachment_manifest: None,
        outgoing: false,
        acked_at: None,
    };

    DirectMessageStore::put_direct_message_message(&store, message.clone())
        .await
        .expect("insert message");
    DirectMessageStore::put_direct_message_tombstone(
        &store,
        DirectMessageTombstoneRow {
            dm_id: dm_id.into(),
            message_id: "message-1".into(),
            deleted_at: 20,
        },
    )
    .await
    .expect("insert tombstone");
    DirectMessageStore::delete_direct_message_message_local(&store, dm_id, "message-1")
        .await
        .expect("delete local message");
    DirectMessageStore::put_direct_message_message(&store, message)
        .await
        .expect("reinsert ignored");

    let page = DirectMessageStore::list_direct_message_messages(&store, dm_id, None, 20)
        .await
        .expect("list messages");
    assert!(page.items.is_empty());
    assert!(
        DirectMessageStore::has_direct_message_tombstone(&store, dm_id, "message-1")
            .await
            .expect("has tombstone")
    );
}
