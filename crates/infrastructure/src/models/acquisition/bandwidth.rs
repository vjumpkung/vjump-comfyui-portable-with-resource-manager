//! One live token bucket for native writes and opaque HF relay downloads.
use application::shared::CancelToken;
use std::{
    collections::VecDeque,
    num::NonZeroU32,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{sync::Notify, time::Instant};

/// Every grant is bounded, including while unlimited.
pub const MAX_GRANT: usize = 16 * 1024;
const NANOS: u128 = 1_000_000_000;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BandwidthError {
    #[error("bandwidth wait cancelled")]
    Cancelled,
}

struct Bucket {
    rate: Option<u64>,
    credit: u128,
    at: Instant,
    queue: VecDeque<Arc<()>>,
}
impl Bucket {
    // Burst: at most 100 ms of traffic, capped at 16 KiB (at least one byte).
    fn capacity(&self) -> u128 {
        self.rate
            .map_or(MAX_GRANT as u64, |r| (r / 10).clamp(1, MAX_GRANT as u64)) as u128
            * NANOS
    }
    fn refill(&mut self) {
        let now = Instant::now();
        if let Some(rate) = self.rate {
            self.credit = self
                .credit
                .saturating_add(
                    now.duration_since(self.at)
                        .as_nanos()
                        .saturating_mul(rate as u128),
                )
                .min(self.capacity());
        }
        self.at = now;
    }
}

pub struct BandwidthLimiter {
    bucket: Mutex<Bucket>,
    changed: Notify,
}
impl BandwidthLimiter {
    pub fn new(limit_kib_s: Option<NonZeroU32>) -> Self {
        let mut bucket = Bucket {
            rate: limit_kib_s.map(|n| u64::from(n.get()) * 1024),
            credit: 0,
            at: Instant::now(),
            queue: VecDeque::new(),
        };
        bucket.credit = bucket.capacity();
        Self {
            bucket: Mutex::new(bucket),
            changed: Notify::new(),
        }
    }

    pub fn set_limit(&self, limit_kib_s: Option<NonZeroU32>) {
        let mut bucket = self.bucket.lock().expect("bandwidth bucket");
        bucket.refill();
        let was_unlimited = bucket.rate.is_none();
        bucket.rate = limit_kib_s.map(|n| u64::from(n.get()) * 1024);
        // Enabling a cap does not bank unlimited time or mint another burst.
        bucket.credit = if was_unlimited {
            0
        } else {
            bucket.credit.min(bucket.capacity())
        };
        drop(bucket);
        self.changed.notify_waiters();
    }

    pub async fn acquire(
        &self,
        max_bytes: usize,
        cancel: &CancelToken,
    ) -> Result<usize, BandwidthError> {
        if cancel.is_cancelled() {
            return Err(BandwidthError::Cancelled);
        }
        if max_bytes == 0 {
            return Ok(0);
        }
        let ticket = Arc::new(());
        self.bucket
            .lock()
            .expect("bandwidth bucket")
            .queue
            .push_back(ticket.clone());
        let _waiting = Waiting {
            limiter: self,
            ticket: ticket.clone(),
        };
        loop {
            if cancel.is_cancelled() {
                return Err(BandwidthError::Cancelled);
            }
            {
                let mut bucket = self.bucket.lock().expect("bandwidth bucket");
                bucket.refill();
                if bucket
                    .queue
                    .front()
                    .is_some_and(|head| Arc::ptr_eq(head, &ticket))
                {
                    let available = if bucket.rate.is_none() {
                        MAX_GRANT
                    } else {
                        (bucket.credit / NANOS) as usize
                    };
                    let grant = max_bytes.min(MAX_GRANT).min(available);
                    if grant > 0 {
                        if bucket.rate.is_some() {
                            bucket.credit -= grant as u128 * NANOS;
                        }
                        return Ok(grant);
                    }
                }
            }
            tokio::select! {
                _ = self.changed.notified() => {},
                _ = tokio::time::sleep(Duration::from_millis(10)) => {},
            }
        }
    }
}

// Also removes a waiter when its future is dropped by select!/task cancellation.
struct Waiting<'a> {
    limiter: &'a BandwidthLimiter,
    ticket: Arc<()>,
}
impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.limiter
            .bucket
            .lock()
            .expect("bandwidth bucket")
            .queue
            .retain(|ticket| !Arc::ptr_eq(ticket, &self.ticket));
        self.limiter.changed.notify_waiters();
    }
}

#[cfg(test)]
mod tests;
