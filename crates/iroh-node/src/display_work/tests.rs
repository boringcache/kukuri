use super::*;
use std::time::Duration;

fn owner() -> Arc<DisplayWorkAdmission> {
    Arc::new(DisplayWorkAdmission::new(WorkLimits {
        scopes: 3,
        requests: 3,
        running: 1,
        ..WorkLimits::default()
    }))
}

#[tokio::test(start_paused = true)]
async fn queued_display_work_is_bounded_and_dropped_waiters_do_not_run() {
    let owner = owner();
    let deadline = Instant::now() + Duration::from_secs(30);
    let first = owner.acquire([1; 32], deadline).await.unwrap();
    let mut second = Box::pin(owner.acquire([2; 32], deadline));
    let mut third = Box::pin(owner.acquire([3; 32], deadline));
    assert!(futures_util::poll!(&mut second).is_pending());
    assert!(futures_util::poll!(&mut third).is_pending());
    assert!(matches!(
        owner.acquire([4; 32], deadline).await,
        Err(DisplayAdmissionError::Deferred)
    ));
    drop(second);
    drop(first);
    let third = third.await.unwrap();
    assert_eq!(owner.state.lock().unwrap().policy.usage().running, 1);
    assert!(third.finish());
    let state = owner.state.lock().unwrap();
    assert_eq!(state.policy.usage(), Default::default());
    assert!(state.scopes.is_empty() && state.ready.is_empty() && state.cancelled.is_empty());
}

#[tokio::test(start_paused = true)]
async fn display_admission_deadline_and_close_discard_prepared_results() {
    let owner = owner();
    let deadline = Instant::now() + Duration::from_secs(30);
    let active = owner.acquire([1; 32], deadline).await.unwrap();
    assert!(matches!(
        owner.acquire([2; 32], deadline).await,
        Err(DisplayAdmissionError::Expired)
    ));
    active.cancelled().await;
    assert!(!active.finish(), "a late result must not leave the adapter");
    let deadline = Instant::now() + Duration::from_secs(30);
    let prepared = owner.acquire([3; 32], deadline).await.unwrap();
    let mut queued = Box::pin(owner.acquire([4; 32], deadline));
    assert!(futures_util::poll!(&mut queued).is_pending());
    owner.close();
    assert!(matches!(queued.await, Err(DisplayAdmissionError::Closed)));
    assert_eq!(
        owner.state.lock().unwrap().policy.usage().running,
        1,
        "prepared work owns its slot until acknowledged"
    );
    prepared.cancelled().await;
    assert!(!prepared.finish());
    assert!(matches!(
        owner.acquire([5; 32], deadline).await,
        Err(DisplayAdmissionError::Closed)
    ));
    assert_eq!(
        owner.state.lock().unwrap().policy.usage(),
        Default::default()
    );
}

#[tokio::test(start_paused = true)]
async fn display_node_close_does_not_stop_another_nodes_work() {
    let first = owner();
    let second = owner();
    let deadline = Instant::now() + Duration::from_secs(30);
    let first_work = first.acquire([1; 32], deadline).await.unwrap();
    let second_work = second.acquire([1; 32], deadline).await.unwrap();
    first.close();
    first_work.cancelled().await;
    assert!(!first_work.finish());
    assert!(second_work.finish());
}
