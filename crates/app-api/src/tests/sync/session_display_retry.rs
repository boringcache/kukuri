use super::*;

#[tokio::test]
async fn visible_missing_manifest_retries_after_the_shared_deadline_without_another_event() {
    let f = SessionFixture::new("live").await;
    f.blobs
        .fail
        .store(true, std::sync::atomic::Ordering::SeqCst);
    f.event(&f.key).await;
    f.display(true, false).await;
    assert_eq!(f.fetches(), 1);
    timeout(Duration::from_secs(8), async {
        while f.fetches() < 2 {
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("one shared timer retries the visible failed session");
    assert_eq!(f.fetches(), 2);
    f.display(false, false).await;
    f.app.shutdown().await;
}

#[tokio::test]
async fn deferred_manifest_admission_retries_without_spending_a_network_attempt() {
    let f = SessionFixture::new("live").await;
    f.blobs
        .reject_admission
        .store(true, std::sync::atomic::Ordering::SeqCst);
    f.event(&f.key).await;
    f.display(true, false).await;
    assert_eq!(f.fetches(), 0);
    f.blobs
        .reject_admission
        .store(false, std::sync::atomic::Ordering::SeqCst);
    timeout(Duration::from_secs(8), async {
        while f.fetches() < 1 {
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("admission returns without another display event");
    assert_eq!(f.fetches(), 1);
    f.display(false, false).await;
    f.app.shutdown().await;
}
