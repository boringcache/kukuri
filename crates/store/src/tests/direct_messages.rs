use super::*;

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
