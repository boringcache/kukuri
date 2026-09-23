use super::*;
use sqlx::Row;

fn row(index: usize) -> NotificationRow {
    NotificationRow {
        notification_id: format!("notification-{index}"),
        recipient_pubkey: "recipient".into(),
        kind: NotificationKind::Mention,
        actor_pubkey: "actor".into(),
        source_envelope_id: None,
        source_replica_id: None,
        topic_id: None,
        channel_id: None,
        object_id: None,
        dm_id: None,
        message_id: None,
        preview_text: None,
        content_labels: None,
        created_at: 10,
        received_at: 20,
        read_at: None,
    }
}

async fn assert_dispatch_pages(store: &dyn NotificationStore) {
    assert_eq!(store.notification_dispatch_head().await.unwrap(), 0);
    for index in 0..129 {
        assert!(store.put_notification_if_absent(row(index)).await.unwrap());
    }
    assert!(!store.put_notification_if_absent(row(0)).await.unwrap());
    assert_eq!(store.notification_dispatch_head().await.unwrap(), 129);
    store
        .mark_notification_read("notification-64", 30)
        .await
        .unwrap();

    let first = store.list_notification_dispatch_after(0).await.unwrap();
    let second = store.list_notification_dispatch_after(64).await.unwrap();
    let third = store.list_notification_dispatch_after(128).await.unwrap();
    assert_eq!(first.len(), NOTIFICATION_DISPATCH_PAGE_SIZE);
    assert_eq!(second.len(), NOTIFICATION_DISPATCH_PAGE_SIZE);
    assert_eq!(third.len(), 1);
    assert_eq!(first[0].0, 1);
    assert_eq!(first[0].1.notification_id, "notification-0");
    assert_eq!(second[0].0, 65);
    assert_eq!(second[0].1.read_at, Some(30));
    assert_eq!(third[0].0, 129);
    assert!(
        store
            .list_notification_dispatch_after(129)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn dispatch_pages_follow_insertion_order_for_tied_timestamps() {
    assert_dispatch_pages(&MemoryStore::default()).await;
    let dir = tempdir().unwrap();
    let store = SqliteStore::connect_file(dir.path().join("notifications.db"))
        .await
        .unwrap();
    assert_dispatch_pages(&store).await;
    let plan = sqlx::query(
        "EXPLAIN QUERY PLAN SELECT notification_id FROM notifications WHERE dispatch_seq > 0 ORDER BY dispatch_seq ASC LIMIT 64",
    )
    .fetch_all(store.pool())
    .await
    .unwrap()
    .iter()
    .map(|row| row.get::<String, _>("detail"))
    .collect::<Vec<_>>()
    .join(" | ");
    assert!(plan.contains("idx_notifications_dispatch_seq"), "{plan}");
    drop(store);
    let reopened = SqliteStore::connect_file(dir.path().join("notifications.db"))
        .await
        .unwrap();
    assert_eq!(reopened.notification_dispatch_head().await.unwrap(), 129);
    assert_eq!(
        reopened
            .list_notification_dispatch_after(128)
            .await
            .unwrap()[0]
            .1
            .notification_id,
        "notification-128"
    );
}

#[tokio::test]
async fn dispatch_migration_excludes_existing_inbox_rows_without_rewriting_them() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql(include_str!(
        "../../migrations/20260405000000_notifications.up.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO notifications (notification_id, recipient_pubkey, kind, actor_pubkey, created_at, received_at) VALUES ('legacy', 'recipient', 'mention', 'actor', 10, 20)",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::raw_sql(include_str!(
        "../../migrations/20260923000000_notification_dispatch_sequence.up.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    let old_sequence: Option<i64> =
        sqlx::query_scalar("SELECT dispatch_seq FROM notifications WHERE notification_id='legacy'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        old_sequence, None,
        "existing inbox history must not be re-toasted"
    );
    sqlx::query(
        "INSERT INTO notifications (notification_id, recipient_pubkey, kind, actor_pubkey, created_at, received_at) VALUES ('new', 'recipient', 'mention', 'actor', 10, 20)",
    )
    .execute(&pool)
    .await
    .unwrap();
    let new_sequence: i64 =
        sqlx::query_scalar("SELECT dispatch_seq FROM notifications WHERE notification_id='new'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(new_sequence, 1);
}
