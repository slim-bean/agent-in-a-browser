#![allow(dead_code, unused_variables, unused_imports, async_fn_in_trait)]
//! wasi-tokio: A tokio-compatible API shim for wasip2 environments.
//!
//! This crate provides the subset of tokio's public API that Codex uses,
//! backed by WASI primitives instead of OS threads and epoll/kqueue.
//! WASM is single-threaded, so:
//! - `spawn()` queues futures onto a global task list polled by `block_on`
//! - `select!` polls all branches and returns the first Ready result
//! - `Mutex`/`RwLock` are just `std::sync` wrappers (no contention possible)
//! - Channels use single-threaded implementations with waker registration
//! - File I/O routes through `wasi:filesystem`
//! - Process spawning routes through WIT shell interfaces
//! - Time operations route through `wasi:clocks`

use std::cell::RefCell;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use std::time::Instant;

// ---------------------------------------------------------------------------
// Global task queue for spawned futures
// ---------------------------------------------------------------------------

/// Monotonically increasing task ID counter.
static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);

/// Status of a registered task.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum TaskStatus {
    Pending,
    Polling,
    Completed,
}

/// Entry in the task registry for diagnostics.
pub(crate) struct TaskEntry {
    pub id: u64,
    pub file: &'static str,
    pub line: u32,
    pub status: TaskStatus,
    pub spawned_at: Instant,
    pub last_poll_start: Option<Instant>,
    pub poll_count: u64,
}

thread_local! {
    /// Registry of all spawned tasks for diagnostics.
    pub(crate) static TASK_REGISTRY: RefCell<Vec<TaskEntry>> = RefCell::new(Vec::new());
}

/// Type-erased future stored in the global task queue, with spawn location metadata.
struct SpawnedTask {
    future: Pin<Box<dyn Future<Output = ()> + 'static>>,
    file: &'static str,
    line: u32,
    task_id: u64,
    /// Shared cancellation flag — set by JoinHandle::abort().
    cancelled: Arc<AtomicBool>,
}

/// Single-write log to avoid WASI stderr fragmentation.
fn log(msg: String) {
    console_log::console_log!("{msg}");
}

thread_local! {
    /// Global queue of spawned tasks. `block_on` drains this each iteration.
    static TASK_QUEUE: RefCell<Vec<SpawnedTask>> = RefCell::new(Vec::new());
}

/// Maximum time a single task poll should take before we consider it a
/// potential deadlock. In WASM, a task that takes >5s is almost certainly
/// an infinite loop or a blocking operation.
const TASK_POLL_DEADLINE_MS: u64 = 5000;

/// Drain all spawned tasks and poll each one. Tasks that return Pending are
/// re-queued for the next iteration. Logs completions, slow polls, and
/// periodic pending-task summaries. Updates the task registry for diagnostics.
fn poll_spawned_tasks(cx: &mut Context<'_>) {
    static LAST_PENDING_LOG: std::sync::Mutex<Option<std::time::Instant>> =
        std::sync::Mutex::new(None);

    let tasks: Vec<SpawnedTask> = { TASK_QUEUE.with(|q| std::mem::take(&mut *q.borrow_mut())) };

    let mut pending = Vec::new();
    let mut pending_names = Vec::new();

    for mut task in tasks.into_iter() {
        let file = task.file.rsplit('/').next().unwrap_or(task.file);
        let line = task.line;
        let task_id = task.task_id;

        // If the task has been cancelled via JoinHandle::abort(), skip it
        // and mark it as completed without polling.
        if task.cancelled.load(Ordering::Acquire) {
            log(format!("[poll_tasks] cancelled #{task_id} {file}:{line}"));
            TASK_REGISTRY.with(|reg| {
                if let Ok(mut entries) = reg.try_borrow_mut() {
                    if let Some(entry) = entries.iter_mut().find(|e| e.id == task_id) {
                        entry.status = TaskStatus::Completed;
                    }
                }
            });
            diagnostics::RUNTIME_STATE.with(|state| {
                if let Ok(mut s) = state.try_borrow_mut() {
                    s.completions_since_last_dump += 1;
                    s.last_completion_time = Some(Instant::now());
                }
            });
            continue;
        }

        let before = std::time::Instant::now();

        // Update task registry: mark as Polling
        TASK_REGISTRY.with(|reg| {
            if let Ok(mut entries) = reg.try_borrow_mut() {
                if let Some(entry) = entries.iter_mut().find(|e| e.id == task_id) {
                    entry.status = TaskStatus::Polling;
                    entry.last_poll_start = Some(before);
                    entry.poll_count += 1;
                }
            }
        });

        match task.future.as_mut().poll(cx) {
            Poll::Pending => {
                // Update task registry: mark as Pending
                TASK_REGISTRY.with(|reg| {
                    if let Ok(mut entries) = reg.try_borrow_mut() {
                        if let Some(entry) = entries.iter_mut().find(|e| e.id == task_id) {
                            entry.status = TaskStatus::Pending;
                        }
                    }
                });
                pending_names.push(format!("{file}:{line}"));
                pending.push(task);
            }
            Poll::Ready(()) => {
                log(format!("[poll_tasks] completed #{task_id} {file}:{line}"));
                // Update task registry: mark as Completed
                TASK_REGISTRY.with(|reg| {
                    if let Ok(mut entries) = reg.try_borrow_mut() {
                        if let Some(entry) = entries.iter_mut().find(|e| e.id == task_id) {
                            entry.status = TaskStatus::Completed;
                        }
                    }
                });
                // Update runtime state completion counter
                diagnostics::RUNTIME_STATE.with(|state| {
                    if let Ok(mut s) = state.try_borrow_mut() {
                        s.completions_since_last_dump += 1;
                        s.last_completion_time = Some(Instant::now());
                    }
                });
            }
        }
        let elapsed_ms = before.elapsed().as_millis();
        if elapsed_ms > 100 {
            log(format!(
                "[poll_tasks] SLOW #{task_id} {file}:{line} took {elapsed_ms}ms"
            ));
        }
    }

    // Prune completed tasks older than 30s from the registry
    TASK_REGISTRY.with(|reg| {
        if let Ok(mut entries) = reg.try_borrow_mut() {
            entries.retain(|e| {
                !(e.status == TaskStatus::Completed && e.spawned_at.elapsed().as_secs() >= 30)
            });
        }
    });

    // Log pending task summary every 5 seconds
    if !pending_names.is_empty() {
        let mut last = LAST_PENDING_LOG.lock().unwrap_or_else(|e| e.into_inner());
        let now = std::time::Instant::now();
        let should_log = match *last {
            None => true,
            Some(t) => now.duration_since(t).as_secs() >= 5,
        };
        if should_log {
            *last = Some(now);
            log(format!(
                "[poll_tasks] {} pending: {}",
                pending_names.len(),
                pending_names.join(", ")
            ));
        }
    }

    if !pending.is_empty() {
        TASK_QUEUE.with(|q| {
            let mut queue = q.borrow_mut();
            pending.append(&mut *queue);
            *queue = pending;
        });
    }
}

/// Returns true if there are spawned tasks waiting to be polled.
fn has_spawned_tasks() -> bool {
    TASK_QUEUE.with(|q| !q.borrow().is_empty())
}

// ---------------------------------------------------------------------------
// spawn / JoinHandle
// ---------------------------------------------------------------------------

/// Spawn a future onto the global task queue. The future will be polled
/// cooperatively by `block_on` alongside the main future.
#[track_caller]
pub fn spawn<F>(future: F) -> JoinHandle<F::Output>
where
    F: Future + 'static,
    F::Output: 'static,
{
    use std::sync::{Arc, Mutex};

    let loc = std::panic::Location::caller();
    let file = loc.file();
    let line = loc.line();
    let task_id = NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed);

    // Register in the task registry for diagnostics
    TASK_REGISTRY.with(|reg| {
        reg.borrow_mut().push(TaskEntry {
            id: task_id,
            file,
            line,
            status: TaskStatus::Pending,
            spawned_at: Instant::now(),
            last_poll_start: None,
            poll_count: 0,
        });
    });

    let result_slot: Arc<Mutex<Option<F::Output>>> = Arc::new(Mutex::new(None));
    let slot_clone = result_slot.clone();
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancelled_clone = cancelled.clone();

    // Wrap the future to store its result when it completes
    let wrapped = async move {
        let val = future.await;
        *slot_clone.lock().unwrap_or_else(|e| e.into_inner()) = Some(val);
    };

    TASK_QUEUE.with(|q| {
        q.borrow_mut().push(SpawnedTask {
            future: Box::pin(wrapped),
            file,
            line,
            task_id,
            cancelled: cancelled_clone,
        });
    });

    JoinHandle {
        result_slot: Some(result_slot),
        task_id,
        cancelled,
    }
}

/// A handle to a spawned task's result.
pub struct JoinHandle<T> {
    pub(crate) result_slot: Option<std::sync::Arc<std::sync::Mutex<Option<T>>>>,
    /// Unique task ID for diagnostics.
    pub task_id: u64,
    /// Shared cancellation flag — when set, poll_spawned_tasks skips the task.
    cancelled: Arc<AtomicBool>,
}

impl<T> JoinHandle<T> {
    /// Abort the task. Marks the task for cancellation so it will not be
    /// polled again by `poll_spawned_tasks`. In single-threaded WASM we
    /// cannot interrupt a running poll, but we prevent future polls.
    pub fn abort(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// Check if the task has finished.
    pub fn is_finished(&self) -> bool {
        match &self.result_slot {
            Some(slot) => slot.lock().unwrap_or_else(|e| e.into_inner()).is_some(),
            None => true,
        }
    }
}

impl<T> std::fmt::Debug for JoinHandle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JoinHandle")
            .field("finished", &self.is_finished())
            .finish()
    }
}

impl<T> Future for JoinHandle<T>
where
    T: Unpin,
{
    type Output = Result<T, JoinError>;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        // If the task was cancelled, return a cancellation error
        if self.cancelled.load(Ordering::Acquire) {
            return Poll::Ready(Err(JoinError { cancelled: true }));
        }
        match &self.result_slot {
            Some(slot) => {
                let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
                match guard.take() {
                    Some(val) => Poll::Ready(Ok(val)),
                    None => Poll::Pending, // Task hasn't completed yet
                }
            }
            None => Poll::Ready(Err(JoinError { cancelled: false })),
        }
    }
}

/// Error returned when a spawned task fails.
#[derive(Debug)]
pub struct JoinError {
    cancelled: bool,
}

impl std::fmt::Display for JoinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.cancelled {
            write!(f, "task was cancelled")
        } else {
            write!(f, "task failed")
        }
    }
}

impl std::error::Error for JoinError {}

impl JoinError {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    pub fn is_panic(&self) -> bool {
        false
    }
}

impl From<JoinError> for std::io::Error {
    fn from(e: JoinError) -> Self {
        std::io::Error::other(e.to_string())
    }
}

// ---------------------------------------------------------------------------
// block_on — polls a future to completion with cooperative task scheduling
// ---------------------------------------------------------------------------

/// Atomic function pointer for yielding to the JS event loop.
/// Set by the WASM component before calling block_on.
static YIELD_FN: AtomicYieldFn = AtomicYieldFn::new();

struct AtomicYieldFn(std::sync::atomic::AtomicUsize);

impl AtomicYieldFn {
    const fn new() -> Self {
        Self(std::sync::atomic::AtomicUsize::new(0))
    }

    fn store(&self, f: fn()) {
        self.0
            .store(f as usize, std::sync::atomic::Ordering::SeqCst);
    }

    fn load(&self) -> Option<fn()> {
        let ptr = self.0.load(std::sync::atomic::Ordering::SeqCst);
        if ptr == 0 {
            None
        } else {
            Some(unsafe { core::mem::transmute(ptr) })
        }
    }
}

/// Register a yield function that block_on calls when a future returns Pending.
/// The component should set this to a function that calls wasi:io/poll or
/// wasi:clocks to trigger JSPI suspension and yield to the JS event loop.
pub fn set_yield_fn(f: fn()) {
    YIELD_FN.store(f);
}

/// Poll a future to completion cooperatively. Each iteration:
/// 1. Poll the main future
/// 2. Poll all spawned tasks
/// 3. **Always** yield to the JS event loop via JSPI — this is critical
///    so the browser can process fetch responses, repaint, and deliver
///    events back to WASM. Without yielding, the poll loop spins forever
///    and starves the JS event loop of execution time.
pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = core::pin::pin!(future);

    // Create a waker that does nothing — in WASM, waking is handled by
    // JSPI suspension/resumption and the cooperative polling loop.
    fn noop_clone(_: *const ()) -> RawWaker {
        RawWaker::new(core::ptr::null(), &NOOP_VTABLE)
    }
    fn noop(_: *const ()) {}
    static NOOP_VTABLE: RawWakerVTable = RawWakerVTable::new(noop_clone, noop, noop, noop);
    let raw = RawWaker::new(core::ptr::null(), &NOOP_VTABLE);
    let waker = unsafe { Waker::from_raw(raw) };
    let mut cx = Context::from_waker(&waker);

    let mut iteration: u64 = 0;
    let mut last_heartbeat = std::time::Instant::now();
    let start = std::time::Instant::now();

    // Initialize runtime state
    diagnostics::RUNTIME_STATE.with(|state| {
        let mut s = state.borrow_mut();
        s.start_time = Some(start);
        s.iteration = 0;
        s.completions_since_last_dump = 0;
    });

    let mut last_loop_log = std::time::Instant::now();
    loop {
        iteration += 1;

        // Update runtime state iteration counter
        diagnostics::RUNTIME_STATE.with(|state| {
            if let Ok(mut s) = state.try_borrow_mut() {
                s.iteration = iteration;
            }
        });

        // Log loop liveness every 5s so we can detect hangs
        let loop_now = std::time::Instant::now();
        if loop_now.duration_since(last_loop_log).as_secs() >= 5 {
            let task_count = TASK_QUEUE.with(|q| q.borrow().len());
            log(format!(
                "[block_on] loop alive: iter={iteration}, tasks={task_count}"
            ));
            last_loop_log = loop_now;
        }

        // Poll the main future
        let before_main = std::time::Instant::now();
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(val) => return val,
            Poll::Pending => {
                let main_elapsed = before_main.elapsed();

                // Poll spawned tasks
                let before_tasks = std::time::Instant::now();
                poll_spawned_tasks(&mut cx);
                let tasks_elapsed = before_tasks.elapsed();

                // Heartbeat every 10s
                let now = std::time::Instant::now();
                if now.duration_since(last_heartbeat).as_secs() >= 10 {
                    let task_count = TASK_QUEUE.with(|q| q.borrow().len());
                    let uptime = start.elapsed().as_secs();
                    log(format!(
                        "[block_on] heartbeat: iter={iteration}, uptime={uptime}s, tasks={task_count}, main={}ms tasks={}ms",
                        main_elapsed.as_millis(), tasks_elapsed.as_millis()));
                    last_heartbeat = now;

                    // Stall detection: if no completions in 30s and pending tasks > 0
                    let should_dump = diagnostics::RUNTIME_STATE.with(|state| {
                        if let Ok(s) = state.try_borrow() {
                            let no_completions_30s = s
                                .last_completion_time
                                .map(|t| t.elapsed().as_secs() >= 30)
                                .unwrap_or(uptime >= 30);
                            no_completions_30s
                                && s.completions_since_last_dump == 0
                                && task_count > 0
                        } else {
                            false
                        }
                    });
                    if should_dump {
                        log("[block_on] STALL DETECTED: no task completions in 30s, dumping diagnostics".to_string());
                        diagnostics::dump_runtime_state();
                        diagnostics::RUNTIME_STATE.with(|state| {
                            if let Ok(mut s) = state.try_borrow_mut() {
                                s.completions_since_last_dump = 0;
                            }
                        });
                    }
                }

                // Log slow iterations (>1s)
                if tasks_elapsed.as_secs() >= 1 || main_elapsed.as_secs() >= 1 {
                    log(format!(
                        "[block_on] SLOW iter={iteration}: main={}ms tasks={}ms",
                        main_elapsed.as_millis(),
                        tasks_elapsed.as_millis()
                    ));
                }

                // Yield to JS event loop via JSPI suspension
                if let Some(yield_fn) = YIELD_FN.load() {
                    yield_fn();
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Submodules matching tokio's module structure
// ---------------------------------------------------------------------------

pub mod diagnostics;
pub mod fs;
pub mod io;
pub mod net;
pub mod process;
pub mod process_backend;
pub mod runtime;
pub mod signal;
pub mod sync;
pub mod task;
pub mod thread_spawn;
pub mod time;
pub mod websocket_backend;

pub use diagnostics::dump_runtime_state;

// ---------------------------------------------------------------------------
// select! macro — proc macro that generates polling code with proper
// borrow separation between guards and futures
// ---------------------------------------------------------------------------

/// Re-export the proc macro as `tokio::select!`
pub use wasi_tokio_macros::select;

#[doc(hidden)]
pub mod select_impl;

/// Public helpers called by the select! macro. Must be `pub` for cross-crate access.
#[doc(hidden)]
pub fn __poll_spawned_tasks(cx: &mut Context<'_>) {
    poll_spawned_tasks(cx);
}

// ---------------------------------------------------------------------------
// Per-call-site select! branch tracking
// ---------------------------------------------------------------------------

/// State for one select! call site.
struct SelectSiteState {
    file: &'static str,
    line: u32,
    num_branches: usize,
    wins: Vec<u64>,
    last_win: Vec<Option<Instant>>,
    total: u64,
    last_log: Instant,
    created: Instant,
}

impl SelectSiteState {
    fn new(file: &'static str, line: u32, num_branches: usize) -> Self {
        Self {
            file,
            line,
            num_branches,
            wins: vec![0; num_branches],
            last_win: vec![None; num_branches],
            total: 0,
            last_log: Instant::now(),
            created: Instant::now(),
        }
    }

    fn record_win(&mut self, branch: usize) {
        self.wins[branch] += 1;
        self.last_win[branch] = Some(Instant::now());
        self.total += 1;

        // Check for stalls every 5 seconds
        if self.last_log.elapsed().as_secs() >= 5 {
            self.log_status();
            self.last_log = Instant::now();
        }
    }

    fn log_status(&self) {
        let now = Instant::now();
        let mut any_starved = false;
        let summary: Vec<String> = (0..self.num_branches)
            .map(|i| {
                let ago = match self.last_win[i] {
                    Some(t) => {
                        let secs = now.duration_since(t).as_secs();
                        if secs >= 10 {
                            any_starved = true;
                        }
                        format!("{}s", secs)
                    }
                    None => {
                        if self.total > 100 {
                            any_starved = true;
                        }
                        "never".to_string()
                    }
                };
                format!("b{}={} ({})", i, self.wins[i], ago)
            })
            .collect();

        let file = self.file.rsplit('/').next().unwrap_or(self.file);
        if any_starved {
            console_log::console_log!(
                "[SELECT STALL] {}:{} total={} branches: {}",
                file,
                self.line,
                self.total,
                summary.join(", ")
            );

            // Panic after 60 seconds of stall to surface deadlocks during development.
            // Check if any branch has NEVER won after significant total polls.
            let stall_secs = self.created.elapsed().as_secs();
            if stall_secs >= 60 {
                for i in 0..self.num_branches {
                    if self.last_win[i].is_none() && self.total > 500 {
                        panic!(
                            "[SELECT DEADLOCK] {}:{} branch b{} has NEVER fired after {}s ({} total polls). \
                             This select! loop is deadlocked.",
                            file, self.line, i, stall_secs, self.total
                        );
                    }
                }
            }
        }
    }
}

/// Register a branch win for a specific call site. Called by select! generated code.
/// Uses a simple global Vec since WASM is single-threaded.
#[doc(hidden)]
pub fn __select_site_win(
    site_id: &std::sync::atomic::AtomicU64,
    file: &'static str,
    line: u32,
    branch: usize,
    num_branches: usize,
) {
    use std::sync::atomic::Ordering;

    thread_local! {
        static SITES: RefCell<Vec<SelectSiteState>> = RefCell::new(Vec::new());
    }

    // Lazily assign a site ID on first call
    let mut id = site_id.load(Ordering::Relaxed);
    if id == 0 {
        SITES.with(|sites| {
            let mut sites = sites.borrow_mut();
            sites.push(SelectSiteState::new(file, line, num_branches));
            id = sites.len() as u64;
            site_id.store(id, Ordering::Relaxed);
        });
    }

    SITES.with(|sites| {
        let mut sites = sites.borrow_mut();
        if let Some(site) = sites.get_mut((id - 1) as usize) {
            site.record_win(branch);
        }
    });
}

/// Return a random starting branch index for select! polling.
/// Uses xorshift64+ PRNG (same algorithm as real tokio) seeded from
/// wasi:random via getrandom. Matches tokio's `thread_rng_n()`.
#[doc(hidden)]
pub fn __select_start(num_branches: usize) -> usize {
    /// xorshift64+ PRNG — identical to tokio::util::rand::FastRand.
    struct FastRand {
        one: u32,
        two: u32,
    }

    impl FastRand {
        fn from_seed(seed: [u8; 8]) -> Self {
            let one = u32::from_le_bytes([seed[0], seed[1], seed[2], seed[3]]);
            let two = u32::from_le_bytes([seed[4], seed[5], seed[6], seed[7]]);
            Self {
                one: one | 1, // must be non-zero
                two: two | 1,
            }
        }

        fn fastrand(&mut self) -> u32 {
            let mut s1 = self.one;
            let s0 = self.two;
            s1 ^= s1 << 17;
            s1 = s1 ^ s0 ^ (s1 >> 7) ^ (s0 >> 16);
            self.one = s0;
            self.two = s1;
            s0.wrapping_add(s1)
        }

        /// Lemire's fast modulo reduction — same as tokio.
        fn fastrand_n(&mut self, n: u32) -> u32 {
            let mul = (self.fastrand() as u64).wrapping_mul(n as u64);
            (mul >> 32) as u32
        }
    }

    thread_local! {
        static RNG: std::cell::RefCell<FastRand> = std::cell::RefCell::new({
            let mut seed = [0u8; 8];
            getrandom::getrandom(&mut seed).unwrap_or_else(|_| {
                // Fallback: use a fixed seed (still better than no randomization)
                seed = [0xde, 0xad, 0xbe, 0xef, 0xca, 0xfe, 0xba, 0xbe];
            });
            FastRand::from_seed(seed)
        });
    }

    RNG.with(|rng| rng.borrow_mut().fastrand_n(num_branches as u32) as usize)
}

/// Poll spawned tasks then yield to JS event loop.
/// Called from the select! polling loop between iterations.
#[doc(hidden)]
pub fn __poll_spawned_tasks_yield() {
    let waker = __noop_waker();
    let mut cx = Context::from_waker(&waker);
    poll_spawned_tasks(&mut cx);
}

/// Yield to the JS event loop via the registered yield function.
#[doc(hidden)]
pub fn __yield_to_js() {
    if let Some(yield_fn) = YIELD_FN.load() {
        yield_fn();
    }
}

/// Synchronous yield — used by select! macro's loop to yield between iterations.
/// Polls spawned tasks then yields to JS event loop.
#[doc(hidden)]
pub fn __yield_once_sync() {
    let waker = __noop_waker();
    let mut cx = Context::from_waker(&waker);
    poll_spawned_tasks(&mut cx);
    __yield_to_js();
}

/// Create a noop waker for manual polling contexts.
#[doc(hidden)]
pub fn __noop_waker() -> Waker {
    fn noop_clone(_: *const ()) -> RawWaker {
        RawWaker::new(core::ptr::null(), &NOOP_VTABLE)
    }
    fn noop(_: *const ()) {}
    static NOOP_VTABLE: RawWakerVTable = RawWakerVTable::new(noop_clone, noop, noop, noop);
    let raw = RawWaker::new(core::ptr::null(), &NOOP_VTABLE);
    unsafe { Waker::from_raw(raw) }
}

// ---------------------------------------------------------------------------
// pin! macro
// ---------------------------------------------------------------------------

/// Pin a value to the stack, matching tokio::pin!.
#[macro_export]
macro_rules! pin {
    ($($x:ident),+ $(,)?) => {
        $(
            let mut $x = $x;
            #[allow(unused_mut)]
            let mut $x = unsafe { std::pin::Pin::new_unchecked(&mut $x) };
        )+
    };
}

// ---------------------------------------------------------------------------
// join! macro
// ---------------------------------------------------------------------------

/// Await multiple futures, matching tokio::join!.
/// In single-threaded WASM, futures are awaited sequentially.
#[macro_export]
macro_rules! join {
    ($($fut:expr),+ $(,)?) => {
        ( $($fut.await),+ )
    };
}

// Re-export PhantomData to suppress unused import warnings in transformed code
pub use std::marker::PhantomData as _phantom;
