//! Synchronization primitives matching tokio::sync.
//!
//! WASM is single-threaded, so these are thin wrappers around std primitives
//! or simple channel implementations.

// Tokio-compatible RwLock with async read()/write() methods.
// In single-threaded WASM, these never contend.

/// RwLock matching tokio::sync::RwLock.
/// In WASM single-threaded mode, this wraps std::sync::RwLock
/// and adds async read() and write() methods.
pub struct RwLock<T> {
    inner: std::sync::RwLock<T>,
}

impl<T> RwLock<T> {
    pub const fn new(val: T) -> Self {
        Self {
            inner: std::sync::RwLock::new(val),
        }
    }

    /// Async read lock — in single-threaded WASM, this never contends.
    pub async fn read(&self) -> RwLockReadGuard<'_, T> {
        self.blocking_read()
    }

    /// Async write lock — in single-threaded WASM, this never contends.
    pub async fn write(&self) -> RwLockWriteGuard<'_, T> {
        self.blocking_write()
    }

    /// Blocking read lock — for use outside async context.
    pub fn blocking_read(&self) -> RwLockReadGuard<'_, T> {
        RwLockReadGuard {
            inner: self.inner.read().unwrap_or_else(|e| e.into_inner()),
        }
    }

    /// Blocking write lock — for use outside async context.
    pub fn blocking_write(&self) -> RwLockWriteGuard<'_, T> {
        RwLockWriteGuard {
            inner: self.inner.write().unwrap_or_else(|e| e.into_inner()),
        }
    }

    /// Try to acquire a read lock without blocking.
    pub fn try_read(&self) -> Result<RwLockReadGuard<'_, T>, TryLockError> {
        self.inner
            .try_read()
            .map(|inner| RwLockReadGuard { inner })
            .map_err(|_| TryLockError(()))
    }

    /// Try to acquire a write lock without blocking.
    pub fn try_write(&self) -> Result<RwLockWriteGuard<'_, T>, TryLockError> {
        self.inner
            .try_write()
            .map(|inner| RwLockWriteGuard { inner })
            .map_err(|_| TryLockError(()))
    }

    pub fn into_inner(self) -> T {
        self.inner.into_inner().unwrap_or_else(|e| e.into_inner())
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.inner.get_mut().unwrap_or_else(|e| e.into_inner())
    }
}

impl<T: Default> Default for RwLock<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for RwLock<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.inner.try_read() {
            Ok(guard) => f.debug_struct("RwLock").field("data", &&*guard).finish(),
            Err(_) => f.debug_struct("RwLock").field("data", &"<locked>").finish(),
        }
    }
}

/// RwLockReadGuard wrapper that is Send+Sync for single-threaded WASM.
pub struct RwLockReadGuard<'a, T: ?Sized> {
    inner: std::sync::RwLockReadGuard<'a, T>,
}

// SAFETY: Single-threaded WASM — no concurrent access.
unsafe impl<T: ?Sized> Send for RwLockReadGuard<'_, T> {}
unsafe impl<T: ?Sized> Sync for RwLockReadGuard<'_, T> {}

impl<T: ?Sized> std::ops::Deref for RwLockReadGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &*self.inner
    }
}

/// RwLockWriteGuard wrapper that is Send+Sync for single-threaded WASM.
pub struct RwLockWriteGuard<'a, T: ?Sized> {
    inner: std::sync::RwLockWriteGuard<'a, T>,
}

// SAFETY: Single-threaded WASM — no concurrent access.
unsafe impl<T: ?Sized> Send for RwLockWriteGuard<'_, T> {}
unsafe impl<T: ?Sized> Sync for RwLockWriteGuard<'_, T> {}

impl<T: ?Sized> std::ops::Deref for RwLockWriteGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &*self.inner
    }
}

impl<T: ?Sized> std::ops::DerefMut for RwLockWriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut *self.inner
    }
}

/// Mutex matching tokio::sync::Mutex.
/// In WASM single-threaded mode, this wraps std::sync::Mutex
/// and adds the async lock() and blocking_lock() methods.
pub struct Mutex<T> {
    inner: std::sync::Mutex<T>,
}

impl<T> Mutex<T> {
    pub const fn new(val: T) -> Self {
        Self {
            inner: std::sync::Mutex::new(val),
        }
    }

    /// Async lock — in single-threaded WASM, this never contends.
    pub async fn lock(&self) -> MutexGuard<'_, T> {
        self.blocking_lock()
    }

    /// Blocking lock — for use outside async context.
    pub fn blocking_lock(&self) -> MutexGuard<'_, T> {
        MutexGuard {
            guard: self.inner.lock().unwrap_or_else(|e| e.into_inner()),
        }
    }

    /// Try to lock without blocking.
    pub fn try_lock(&self) -> Result<MutexGuard<'_, T>, TryLockError> {
        self.inner
            .try_lock()
            .map(|guard| MutexGuard { guard })
            .map_err(|_| TryLockError(()))
    }

    /// Lock and return an owned guard (requires Arc<Mutex<T>>).
    /// SAFETY: Single-threaded WASM — we get a pointer to the inner data
    /// and keep the Arc alive to ensure the allocation persists.
    pub async fn lock_owned(self: std::sync::Arc<Self>) -> OwnedMutexGuard<T> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let ptr = &*guard as *const T as *mut T;
        // Drop the std guard immediately — in single-threaded WASM there's
        // no contention, and we hold the Arc to keep the data alive.
        drop(guard);
        OwnedMutexGuard { _mutex: self, ptr }
    }

    pub fn into_inner(self) -> T {
        self.inner.into_inner().unwrap_or_else(|e| e.into_inner())
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.inner.get_mut().unwrap_or_else(|e| e.into_inner())
    }
}

impl<T: Default> Default for Mutex<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Mutex<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.inner.try_lock() {
            Ok(guard) => f.debug_struct("Mutex").field("data", &&*guard).finish(),
            Err(_) => f.debug_struct("Mutex").field("data", &"<locked>").finish(),
        }
    }
}

/// MutexGuard matching tokio::sync::MutexGuard.
pub struct MutexGuard<'a, T: ?Sized> {
    guard: std::sync::MutexGuard<'a, T>,
}

// SAFETY: Single-threaded WASM — no concurrent access.
unsafe impl<T: ?Sized> Send for MutexGuard<'_, T> {}
unsafe impl<T: ?Sized> Sync for MutexGuard<'_, T> {}

impl<T: ?Sized> std::ops::Deref for MutexGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.guard
    }
}

impl<T: ?Sized> std::ops::DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.guard
    }
}

/// OwnedMutexGuard matching tokio::sync::OwnedMutexGuard.
/// In single-threaded WASM, this is a simple wrapper holding the value.
pub struct OwnedMutexGuard<T> {
    _mutex: std::sync::Arc<Mutex<T>>,
    // SAFETY: We hold the Arc keeping the Mutex alive.
    // In single-threaded WASM there is no contention.
    ptr: *mut T,
}

// SAFETY: Single-threaded WASM — no concurrent access.
unsafe impl<T> Send for OwnedMutexGuard<T> {}
unsafe impl<T> Sync for OwnedMutexGuard<T> {}

impl<T> std::ops::Deref for OwnedMutexGuard<T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.ptr }
    }
}

impl<T> std::ops::DerefMut for OwnedMutexGuard<T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.ptr }
    }
}

#[derive(Debug)]
pub struct TryLockError(());

impl std::fmt::Display for TryLockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "lock is held")
    }
}

impl std::error::Error for TryLockError {}

pub mod broadcast;
pub mod mpsc;
pub mod oneshot;
pub mod watch;

// ---------------------------------------------------------------------------
// Notify
// ---------------------------------------------------------------------------

/// Notification primitive matching tokio::sync::Notify.
///
/// Uses a shared atomic flag for coordination between tasks in the
/// cooperative single-threaded scheduler.
pub struct Notify {
    notified: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl std::fmt::Debug for Notify {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Notify").finish()
    }
}

impl Default for Notify {
    fn default() -> Self {
        Self::new()
    }
}

impl Notify {
    pub fn new() -> Self {
        Notify {
            notified: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    pub fn const_new() -> Self {
        Self::new()
    }

    pub fn notify_one(&self) {
        self.notified
            .store(true, std::sync::atomic::Ordering::Release);
    }

    pub fn notify_waiters(&self) {
        self.notified
            .store(true, std::sync::atomic::Ordering::Release);
    }

    pub fn notified(&self) -> Notified {
        Notified {
            flag: std::sync::Arc::clone(&self.notified),
            first_poll: true,
        }
    }

    pub fn notified_owned(self: &std::sync::Arc<Self>) -> Notified {
        Notified {
            flag: std::sync::Arc::clone(&self.notified),
            first_poll: true,
        }
    }
}

/// Future returned by [`Notify::notified()`].
pub struct Notified {
    flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    first_poll: bool,
}

impl std::future::Future for Notified {
    type Output = ();
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<()> {
        // If notification was already sent, consume it and return Ready
        if self
            .flag
            .compare_exchange(
                true,
                false,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .is_ok()
        {
            return std::task::Poll::Ready(());
        }
        // On first poll, return Pending to yield to the scheduler
        // (allows notify_one() to run in another task)
        if self.first_poll {
            self.first_poll = false;
            return std::task::Poll::Pending;
        }
        // On subsequent polls, check again
        if self.flag.load(std::sync::atomic::Ordering::Acquire) {
            self.flag.store(false, std::sync::atomic::Ordering::Release);
            std::task::Poll::Ready(())
        } else {
            std::task::Poll::Pending
        }
    }
}

// ---------------------------------------------------------------------------
// Barrier
// ---------------------------------------------------------------------------

/// Barrier matching tokio::sync::Barrier.
/// In single-threaded WASM, wait() always returns immediately as leader.
pub struct Barrier {
    _n: usize,
}

impl Barrier {
    pub fn new(n: usize) -> Self {
        Barrier { _n: n }
    }

    pub async fn wait(&self) -> BarrierWaitResult {
        BarrierWaitResult { is_leader: true }
    }
}

/// Result returned by [`Barrier::wait()`].
pub struct BarrierWaitResult {
    is_leader: bool,
}

impl BarrierWaitResult {
    pub fn is_leader(&self) -> bool {
        self.is_leader
    }
}

// ---------------------------------------------------------------------------
// OnceCell
// ---------------------------------------------------------------------------

/// OnceCell matching tokio::sync::OnceCell.
/// In single-threaded WASM, this is a simple Option wrapper.
pub struct OnceCell<T> {
    inner: std::sync::Mutex<Option<T>>,
}

impl<T> OnceCell<T> {
    pub fn new() -> Self {
        OnceCell {
            inner: std::sync::Mutex::new(None),
        }
    }

    pub const fn const_new() -> Self {
        OnceCell {
            inner: std::sync::Mutex::new(None),
        }
    }

    pub fn get(&self) -> Option<&T> {
        // SAFETY: single-threaded WASM — no concurrent mutation after set.
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_some() {
            // Re-borrow through the raw pointer to get a &T with the right lifetime.
            // Safe because single-threaded WASM ensures no concurrent mutation.
            let ptr = &*guard as *const Option<T>;
            unsafe { (*ptr).as_ref() }
        } else {
            None
        }
    }

    pub fn set(&self, value: T) -> Result<(), T> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_some() {
            Err(value)
        } else {
            *guard = Some(value);
            Ok(())
        }
    }

    pub async fn get_or_init<F, Fut>(&self, f: F) -> &T
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        // Check if already initialized — scope the guard so it's not
        // held across any .await points (avoids Send issues).
        let needs_init = {
            let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            guard.is_none()
        };
        if needs_init {
            let val = f().await;
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if guard.is_none() {
                *guard = Some(val);
            }
        }
        self.get().expect("OnceCell was just initialized")
    }

    pub async fn get_or_try_init<F, Fut, E>(&self, f: F) -> Result<&T, E>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, E>>,
    {
        // Check if already initialized — scope the guard so it's not
        // held across any .await points (avoids Send issues).
        let needs_init = {
            let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            guard.is_none()
        };
        if needs_init {
            let val = f().await?;
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if guard.is_none() {
                *guard = Some(val);
            }
        }
        Ok(self.get().expect("OnceCell was just initialized"))
    }
}

/// Semaphore (simplified for single-threaded WASM).
pub struct Semaphore {
    permits: std::sync::atomic::AtomicUsize,
}

impl Semaphore {
    pub const fn new(permits: usize) -> Self {
        Self {
            permits: std::sync::atomic::AtomicUsize::new(permits),
        }
    }

    pub async fn acquire(&self) -> Result<SemaphorePermit<'_>, AcquireError> {
        let current = self.permits.load(std::sync::atomic::Ordering::Relaxed);
        if current == 0 {
            return Err(AcquireError(()));
        }
        self.permits
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        Ok(SemaphorePermit { sem: self })
    }

    pub fn try_acquire(&self) -> Result<SemaphorePermit<'_>, TryAcquireError> {
        let current = self.permits.load(std::sync::atomic::Ordering::Relaxed);
        if current == 0 {
            return Err(TryAcquireError(()));
        }
        self.permits
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        Ok(SemaphorePermit { sem: self })
    }

    pub async fn acquire_owned(
        self: std::sync::Arc<Self>,
    ) -> Result<OwnedSemaphorePermit, AcquireError> {
        let current = self.permits.load(std::sync::atomic::Ordering::Relaxed);
        if current == 0 {
            return Err(AcquireError(()));
        }
        self.permits
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        Ok(OwnedSemaphorePermit { sem: self })
    }
}

pub struct SemaphorePermit<'a> {
    sem: &'a Semaphore,
}

impl<'a> Drop for SemaphorePermit<'a> {
    fn drop(&mut self) {
        self.sem
            .permits
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

pub struct OwnedSemaphorePermit {
    sem: std::sync::Arc<Semaphore>,
}

impl Drop for OwnedSemaphorePermit {
    fn drop(&mut self) {
        self.sem
            .permits
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

#[derive(Debug)]
pub struct AcquireError(());

impl std::fmt::Display for AcquireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "semaphore closed")
    }
}

impl std::error::Error for AcquireError {}

#[derive(Debug)]
pub struct TryAcquireError(());

impl std::fmt::Display for TryAcquireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "no permits available")
    }
}

impl std::error::Error for TryAcquireError {}
