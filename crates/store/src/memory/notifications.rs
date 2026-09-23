use super::*;

#[async_trait]
impl NotificationStore for MemoryStore {
    async fn put_notification_if_absent(&self, row: NotificationRow) -> Result<bool> {
        let mut notifications = self.notification_rows.write().await;
        if notifications
            .rows
            .contains_key(row.notification_id.as_str())
        {
            return Ok(false);
        }
        let next = notifications
            .last_sequence
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("notification dispatch sequence exhausted"))?;
        notifications.last_sequence = next;
        notifications
            .by_sequence
            .insert(next, row.notification_id.clone());
        notifications.rows.insert(row.notification_id.clone(), row);
        Ok(true)
    }

    async fn list_notifications(&self) -> Result<Vec<NotificationRow>> {
        let mut items = self
            .notification_rows
            .read()
            .await
            .rows
            .values()
            .cloned()
            .collect::<Vec<_>>();
        items.sort_by(|left, right| {
            right
                .received_at
                .cmp(&left.received_at)
                .then_with(|| right.notification_id.cmp(&left.notification_id))
        });
        Ok(items)
    }

    async fn list_notification_dispatch_after(
        &self,
        after_sequence: i64,
    ) -> Result<Vec<(i64, NotificationRow)>> {
        use std::ops::Bound::{Excluded, Unbounded};
        let notifications = self.notification_rows.read().await;
        Ok(notifications
            .by_sequence
            .range((Excluded(after_sequence), Unbounded))
            .take(crate::NOTIFICATION_DISPATCH_PAGE_SIZE)
            .map(|(sequence, id)| {
                (
                    *sequence,
                    notifications
                        .rows
                        .get(id)
                        .expect("notification dispatch index must reference a row")
                        .clone(),
                )
            })
            .collect())
    }

    async fn notification_dispatch_head(&self) -> Result<i64> {
        Ok(self.notification_rows.read().await.last_sequence)
    }

    async fn mark_notification_read(&self, notification_id: &str, read_at: i64) -> Result<()> {
        if let Some(row) = self
            .notification_rows
            .write()
            .await
            .rows
            .get_mut(notification_id)
        {
            row.read_at.get_or_insert(read_at);
        }
        Ok(())
    }

    async fn mark_all_notifications_read(&self, read_at: i64) -> Result<()> {
        for row in self.notification_rows.write().await.rows.values_mut() {
            row.read_at.get_or_insert(read_at);
        }
        Ok(())
    }

    async fn count_unread_notifications(&self) -> Result<usize> {
        Ok(self
            .notification_rows
            .read()
            .await
            .rows
            .values()
            .filter(|row| row.read_at.is_none())
            .count())
    }
}
