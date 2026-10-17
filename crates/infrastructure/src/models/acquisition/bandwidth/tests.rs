use super::*;

#[tokio::test(start_paused = true)]
async fn small_limits_split_large_chunks_without_deadlock() {
    let limiter = BandwidthLimiter::new(NonZeroU32::new(1));
    let cancel = CancelToken::new();
    let started = Instant::now();
    let mut left = 512;
    tokio::time::timeout(Duration::from_secs(3), async {
        while left > 0 {
            let grant = limiter.acquire(left, &cancel).await.unwrap();
            assert!((1..=102).contains(&grant));
            left -= grant;
        }
    })
    .await
    .unwrap();
    assert!(512.0 <= 1024.0 * started.elapsed().as_secs_f64() + 102.0);
    let maximum = BandwidthLimiter::new(NonZeroU32::new(u32::MAX));
    assert_eq!(
        maximum.acquire(usize::MAX, &cancel).await.unwrap(),
        MAX_GRANT
    );
}

#[tokio::test(start_paused = true)]
async fn queued_waiters_cancel_without_leaking_capacity() {
    let limiter = Arc::new(BandwidthLimiter::new(NonZeroU32::new(1)));
    let cancel = CancelToken::new();
    assert_eq!(limiter.acquire(1024, &cancel).await.unwrap(), 102);
    let first_cancel = CancelToken::new();
    let first = {
        let limiter = limiter.clone();
        let cancel = first_cancel.clone();
        tokio::spawn(async move { limiter.acquire(1024, &cancel).await })
    };
    // Queue the head before its successor, then drop another queued future.
    tokio::task::yield_now().await;
    let dropped = {
        let limiter = limiter.clone();
        tokio::spawn(async move { limiter.acquire(1024, &CancelToken::new()).await })
    };
    tokio::task::yield_now().await;
    dropped.abort();
    let _ = dropped.await;
    let second = {
        let limiter = limiter.clone();
        tokio::spawn(async move { limiter.acquire(1024, &CancelToken::new()).await })
    };
    first_cancel.cancel();
    assert_eq!(first.await.unwrap(), Err(BandwidthError::Cancelled));
    let grant = tokio::time::timeout(Duration::from_secs(1), second)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(grant > 0);
    assert!(limiter.bucket.lock().unwrap().queue.is_empty());
    // Cancelling a waiter did not deduct its requested 1024 bytes.
    assert!(limiter.acquire(1, &cancel).await.unwrap() > 0);
}

#[tokio::test(start_paused = true)]
async fn fifo_grants_and_live_limit_changes_wake_waiters() {
    let limiter = Arc::new(BandwidthLimiter::new(NonZeroU32::new(1)));
    limiter.acquire(1024, &CancelToken::new()).await.unwrap();
    let mut tasks = Vec::new();
    let (order, mut granted) = tokio::sync::mpsc::unbounded_channel();
    for index in 0..3 {
        let limiter = limiter.clone();
        let order = order.clone();
        tasks.push(tokio::spawn(async move {
            let grant = limiter.acquire(1, &CancelToken::new()).await.unwrap();
            order.send(index).unwrap();
            (index, grant)
        }));
        tokio::task::yield_now().await;
    }
    limiter.set_limit(None);
    for (index, task) in tasks.into_iter().enumerate() {
        assert_eq!(task.await.unwrap(), (index, 1));
        assert_eq!(granted.recv().await.unwrap(), index);
    }
    limiter.set_limit(NonZeroU32::new(1));
    assert!(
        limiter
            .acquire(MAX_GRANT, &CancelToken::new())
            .await
            .unwrap()
            <= 102
    );
}
