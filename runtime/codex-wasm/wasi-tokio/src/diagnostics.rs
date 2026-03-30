//! Runtime diagnostics for the WASM cooperative scheduler.
//!
//! Provides channel registry, runtime state tracking, and diagnostic dump
//! facilities for debugging stalls and deadlocks in single-threaded WASM.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Monotonically increasing channel ID counter.
pub static NEXT_CHANNEL_ID: AtomicU64 = AtomicU64::new(1);

/// Point-in-time snapshot of a channel's state for diagnostics.
pub struct ChannelSnapshot {
    pub id: u64,
    /// Channel kind: "mpsc", "mpsc-unbounded", "oneshot", "broadcast", "watch"
    pub kind: &'static str,
    /// Short label like "file.rs:123" from #[track_caller]
    pub label: String,
    pub queue_len: usize,
    pub capacity: Option<usize>,
    pub closed: bool,
    pub sender_count: usize,
    pub receiver_alive: bool,
    pub pending_wakers: usize,
}

type SnapshotFn = Box<dyn Fn() -> Option<ChannelSnapshot>>;

thread_local! {
    static CHANNEL_REGISTRY: RefCell<Vec<SnapshotFn>> = RefCell::new(Vec::new());
}

/// Register a channel snapshot closure. The closure should return `None`
/// when the channel has been dropped (e.g. Weak::upgrade fails), which
/// causes the entry to be pruned from the registry.
pub fn register_channel(f: SnapshotFn) {
    CHANNEL_REGISTRY.with(|reg| reg.borrow_mut().push(f));
}

/// Collect snapshots from all live channels, pruning dead entries.
fn collect_channel_snapshots() -> Vec<ChannelSnapshot> {
    CHANNEL_REGISTRY.with(|reg| {
        let mut reg = reg.borrow_mut();
        let mut snapshots = Vec::new();
        reg.retain(|f| match f() {
            Some(snap) => {
                snapshots.push(snap);
                true
            }
            None => false,
        });
        snapshots
    })
}

/// Runtime state shared between block_on and diagnostics.
pub struct RuntimeState {
    pub iteration: u64,
    pub start_time: Option<Instant>,
    pub last_completion_time: Option<Instant>,
    pub completions_since_last_dump: u64,
}

thread_local! {
    pub static RUNTIME_STATE: RefCell<RuntimeState> = RefCell::new(RuntimeState {
        iteration: 0,
        start_time: None,
        last_completion_time: None,
        completions_since_last_dump: 0,
    });
}

/// Dump all runtime state to console via console_log!.
///
/// Uses `try_borrow()` to avoid panicking if called during an existing borrow
/// of the task or channel registries.
pub fn dump_runtime_state() {
    crate::log("[DIAG] === Runtime Diagnostics Dump ===".to_string());

    // Runtime state
    RUNTIME_STATE.with(|state| {
        if let Ok(s) = state.try_borrow() {
            let uptime = s
                .start_time
                .map(|t| t.elapsed().as_secs())
                .unwrap_or(0);
            let since_last = s
                .last_completion_time
                .map(|t| format!("{}s ago", t.elapsed().as_secs()))
                .unwrap_or_else(|| "never".to_string());
            crate::log(format!(
                "[DIAG] runtime: iteration={}, uptime={}s, last_completion={}, completions_since_dump={}",
                s.iteration, uptime, since_last, s.completions_since_last_dump
            ));
        } else {
            crate::log("[DIAG] runtime: <state borrowed, cannot read>".to_string());
        }
    });

    // Task state
    crate::TASK_REGISTRY.with(|reg| {
        if let Ok(tasks) = reg.try_borrow() {
            crate::log(format!("[DIAG] tasks: {} registered", tasks.len()));
            for task in tasks.iter() {
                let status = match task.status {
                    crate::TaskStatus::Pending => "pending",
                    crate::TaskStatus::Polling => "POLLING",
                    crate::TaskStatus::Completed => "completed",
                };
                let age = task.spawned_at.elapsed().as_secs();
                let poll_info = task
                    .last_poll_start
                    .map(|t| format!(", last_poll={}ms ago", t.elapsed().as_millis()))
                    .unwrap_or_default();
                let file = task.file.rsplit('/').next().unwrap_or(task.file);
                crate::log(format!(
                    "[DIAG]   task #{}: {}:{} status={} age={}s polls={}{}",
                    task.id, file, task.line, status, age, task.poll_count, poll_info
                ));
            }
        } else {
            crate::log("[DIAG] tasks: <registry borrowed, cannot read>".to_string());
        }
    });

    // Channel state
    CHANNEL_REGISTRY.with(|reg| {
        if let Ok(fns) = reg.try_borrow() {
            // We need to call each fn to get snapshots, but we can't mutate
            // (retain) during a shared borrow. Collect what we can.
            let mut snapshots = Vec::new();
            for f in fns.iter() {
                if let Some(snap) = f() {
                    snapshots.push(snap);
                }
            }
            crate::log(format!("[DIAG] channels: {} live", snapshots.len()));
            for ch in &snapshots {
                let cap_str = ch
                    .capacity
                    .map(|c| format!("/{c}"))
                    .unwrap_or_else(|| "/unbounded".to_string());
                crate::log(format!(
                    "[DIAG]   chan #{}: {} [{}] queue={}{} closed={} senders={} rx_alive={} wakers={}",
                    ch.id,
                    ch.kind,
                    ch.label,
                    ch.queue_len,
                    cap_str,
                    ch.closed,
                    ch.sender_count,
                    ch.receiver_alive,
                    ch.pending_wakers
                ));
            }
        } else {
            crate::log("[DIAG] channels: <registry borrowed, cannot read>".to_string());
        }
    });

    crate::log("[DIAG] === End Diagnostics Dump ===".to_string());
}
