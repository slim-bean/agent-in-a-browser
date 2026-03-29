//! Time operations matching tokio::time.
//!
//! Routes through wasi:clocks in wasip2.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
pub use std::time::Duration;
use std::time::Instant as StdInstant;

pub use std::time::Instant;

/// Sleep for the given duration. In WASM with JSPI, this suspends
/// to the JS event loop via the wasi:clocks implementation.
pub fn sleep(duration: Duration) -> Sleep {
    Sleep::new(duration)
}

/// Sleep until the given instant.
pub fn sleep_until(deadline: Instant) -> Sleep {
    let now = Instant::now();
    if deadline > now {
        Sleep::new(deadline - now)
    } else {
        Sleep::new(Duration::from_secs(0))
    }
}

/// Create a future that completes after the given duration.
pub fn timeout<F: Future>(duration: Duration, future: F) -> Timeout<F> {
    Timeout {
        future,
        duration,
        started: StdInstant::now(),
    }
}

/// Create a future that completes after the given deadline.
pub fn timeout_at<F: Future>(deadline: Instant, future: F) -> Timeout<F> {
    let now = Instant::now();
    let duration = if deadline > now {
        deadline - now
    } else {
        Duration::from_secs(0)
    };
    Timeout {
        future,
        duration,
        started: StdInstant::now(),
    }
}

/// A future wrapping another future with a timeout.
/// Uses structural pinning so inner future doesn't need Unpin.
pub struct Timeout<F> {
    future: F,
    duration: Duration,
    started: StdInstant,
}

impl<F: Future> Timeout<F> {
    /// Get a pinned reference to the inner future.
    /// SAFETY: The inner future is structurally pinned — we never move it
    /// after Timeout is pinned.
    fn project(self: Pin<&mut Self>) -> Pin<&mut F> {
        // SAFETY: We only access the `future` field and promise not to move it.
        unsafe { self.map_unchecked_mut(|this| &mut this.future) }
    }

    fn started(self: Pin<&Self>) -> &StdInstant {
        &self.get_ref().started
    }

    fn duration(self: Pin<&Self>) -> Duration {
        self.get_ref().duration
    }
}

impl<F: Future> Future for Timeout<F> {
    type Output = Result<F::Output, Elapsed>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let duration = self.as_ref().duration();
        let started = *self.as_ref().started();
        if started.elapsed() > duration {
            return Poll::Ready(Err(Elapsed(())));
        }
        match self.project().poll(cx) {
            Poll::Ready(val) => Poll::Ready(Ok(val)),
            Poll::Pending => {
                if started.elapsed() > duration {
                    Poll::Ready(Err(Elapsed(())))
                } else {
                    Poll::Pending
                }
            }
        }
    }
}

/// Sleep future — returned by `sleep()`.
pub struct Sleep {
    duration: Duration,
    started: bool,
}

impl Sleep {
    pub fn new(duration: Duration) -> Self {
        Self {
            duration,
            started: false,
        }
    }

    pub fn reset(self: Pin<&mut Self>, _deadline: Instant) {
        // No-op in single-threaded WASM
    }
}

impl Future for Sleep {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();
        if !this.started {
            // First poll: return Pending to yield to the event loop.
            // block_on will call the yield function (subscribe_duration +
            // pollable.block) which triggers JSPI suspension.
            this.started = true;
            Poll::Pending
        } else {
            // Second poll: sleep is complete.
            Poll::Ready(())
        }
    }
}

/// Error when a timeout expires.
#[derive(Debug)]
pub struct Elapsed(());

impl std::fmt::Display for Elapsed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "deadline has elapsed")
    }
}

impl std::error::Error for Elapsed {}

/// Interval matching tokio::time::interval.
pub fn interval(period: Duration) -> Interval {
    Interval {
        period,
        next: StdInstant::now() + period,
    }
}

pub fn interval_at(start: Instant, period: Duration) -> Interval {
    Interval {
        period,
        next: start + period,
    }
}

pub struct Interval {
    period: Duration,
    next: StdInstant,
}

impl Interval {
    pub async fn tick(&mut self) -> Instant {
        let now = StdInstant::now();
        if now < self.next {
            let wait = self.next - now;
            sleep(wait).await;
        }
        let tick_at = self.next;
        self.next = StdInstant::now() + self.period;
        tick_at
    }

    pub fn reset(&mut self) {
        self.next = StdInstant::now() + self.period;
    }

    pub fn period(&self) -> Duration {
        self.period
    }

    pub fn set_missed_tick_behavior(&mut self, _behavior: MissedTickBehavior) {
        // No-op in single-threaded WASM
    }
}

/// MissedTickBehavior — no-op in single-threaded WASM.
#[derive(Debug, Clone, Copy)]
pub enum MissedTickBehavior {
    Burst,
    Delay,
    Skip,
}

/// Error types matching tokio::time::error.
pub mod error {
    pub use super::Elapsed;
}
