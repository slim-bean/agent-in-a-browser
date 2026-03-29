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

use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

// ---------------------------------------------------------------------------
// Global task queue for spawned futures
// ---------------------------------------------------------------------------

/// Type-erased future stored in the global task queue, with spawn location metadata.
struct SpawnedTask {
    future: Pin<Box<dyn Future<Output = ()> + Send + 'static>>,
    file: &'static str,
    line: u32,
}

/// Single-write log to avoid WASI stderr fragmentation.
fn log(msg: String) {
    eprintln!("{msg}");
}

/// Global queue of spawned tasks. `block_on` drains this each iteration.
static TASK_QUEUE: std::sync::Mutex<Vec<SpawnedTask>> = std::sync::Mutex::new(Vec::new());

/// Maximum time a single task poll should take before we consider it a
/// potential deadlock. In WASM, a task that takes >5s is almost certainly
/// an infinite loop or a blocking operation.
const TASK_POLL_DEADLINE_MS: u64 = 5000;

/// Drain all spawned tasks and poll each one. Tasks that return Pending are
/// re-queued for the next iteration. Logs completions, slow polls, and
/// periodic pending-task summaries.
fn poll_spawned_tasks(cx: &mut Context<'_>) {
    static LAST_PENDING_LOG: std::sync::Mutex<Option<std::time::Instant>> =
        std::sync::Mutex::new(None);

    let tasks: Vec<SpawnedTask> = {
        let mut queue = TASK_QUEUE.lock().unwrap_or_else(|e| e.into_inner());
        std::mem::take(&mut *queue)
    };

    let mut pending = Vec::new();
    let mut pending_names = Vec::new();

    for mut task in tasks.into_iter() {
        let file = task.file.rsplit('/').next().unwrap_or(task.file);
        let line = task.line;
        let before = std::time::Instant::now();
        match task.future.as_mut().poll(cx) {
            Poll::Pending => {
                pending_names.push(format!("{file}:{line}"));
                pending.push(task);
            }
            Poll::Ready(()) => {
                log(format!("[poll_tasks] completed {file}:{line}"));
            }
        }
        let elapsed_ms = before.elapsed().as_millis();
        if elapsed_ms > 100 {
            log(format!(
                "[poll_tasks] SLOW {file}:{line} took {elapsed_ms}ms"
            ));
        }
    }

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
        let mut queue = TASK_QUEUE.lock().unwrap_or_else(|e| e.into_inner());
        pending.append(&mut *queue);
        *queue = pending;
    }
}

/// Returns true if there are spawned tasks waiting to be polled.
fn has_spawned_tasks() -> bool {
    let queue = TASK_QUEUE.lock().unwrap_or_else(|e| e.into_inner());
    !queue.is_empty()
}

// ---------------------------------------------------------------------------
// spawn / JoinHandle
// ---------------------------------------------------------------------------

/// Spawn a future onto the global task queue. The future will be polled
/// cooperatively by `block_on` alongside the main future.
#[track_caller]
pub fn spawn<F>(future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    use std::sync::{Arc, Mutex};

    let loc = std::panic::Location::caller();
    let file = loc.file();
    let line = loc.line();

    let result_slot: Arc<Mutex<Option<F::Output>>> = Arc::new(Mutex::new(None));
    let slot_clone = result_slot.clone();

    // Wrap the future to store its result when it completes
    let wrapped = async move {
        let val = future.await;
        *slot_clone.lock().unwrap_or_else(|e| e.into_inner()) = Some(val);
    };

    let mut queue = TASK_QUEUE.lock().unwrap_or_else(|e| e.into_inner());
    queue.push(SpawnedTask {
        future: Box::pin(wrapped),
        file,
        line,
    });

    JoinHandle {
        result_slot: Some(result_slot),
    }
}

/// A handle to a spawned task's result.
pub struct JoinHandle<T> {
    pub(crate) result_slot: Option<std::sync::Arc<std::sync::Mutex<Option<T>>>>,
}

impl<T> JoinHandle<T> {
    /// Abort the task. In single-threaded WASM this is a no-op
    /// (we can't remove a specific task from the queue easily).
    pub fn abort(&self) {}

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
        match &self.result_slot {
            Some(slot) => {
                let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
                match guard.take() {
                    Some(val) => Poll::Ready(Ok(val)),
                    None => Poll::Pending, // Task hasn't completed yet
                }
            }
            None => Poll::Ready(Err(JoinError { _priv: () })),
        }
    }
}

/// Error returned when a spawned task fails.
#[derive(Debug)]
pub struct JoinError {
    _priv: (),
}

impl std::fmt::Display for JoinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "task failed")
    }
}

impl std::error::Error for JoinError {}

impl JoinError {
    pub fn is_cancelled(&self) -> bool {
        false
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

    let mut last_loop_log = std::time::Instant::now();
    loop {
        iteration += 1;

        // Log loop liveness every 5s so we can detect hangs
        let loop_now = std::time::Instant::now();
        if loop_now.duration_since(last_loop_log).as_secs() >= 5 {
            let task_count = { TASK_QUEUE.lock().unwrap_or_else(|e| e.into_inner()).len() };
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
                    let task_count = { TASK_QUEUE.lock().unwrap_or_else(|e| e.into_inner()).len() };
                    let uptime = start.elapsed().as_secs();
                    log(format!(
                        "[block_on] heartbeat: iter={iteration}, uptime={uptime}s, tasks={task_count}, main={}ms tasks={}ms",
                        main_elapsed.as_millis(), tasks_elapsed.as_millis()));
                    last_heartbeat = now;
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
