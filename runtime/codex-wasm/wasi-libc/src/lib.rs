#![allow(non_camel_case_types, dead_code)]
//! Minimal libc shim for wasip2 — provides only the types, constants and functions
//! that Codex code actually references. All functions return error/zero values since
//! platform-specific code paths are unreachable in WASM.

// Types
pub type uid_t = u32;
pub type gid_t = u32;
pub type pid_t = i32;
pub type size_t = usize;
pub type c_int = i32;
pub type c_long = i64;
pub type c_char = i8;
pub type c_uint = u32;
pub type c_ulong = u64;
pub type c_void = std::ffi::c_void;

// passwd struct (used by shell.rs for user shell detection)
#[repr(C)]
pub struct passwd {
    pub pw_name: *mut c_char,
    pub pw_passwd: *mut c_char,
    pub pw_uid: uid_t,
    pub pw_gid: gid_t,
    pub pw_gecos: *mut c_char,
    pub pw_dir: *mut c_char,
    pub pw_shell: *mut c_char,
}

// Signal constants
pub const SIGSYS: c_int = 31;
pub const SIGTERM: c_int = 15;
pub const SIGKILL: c_int = 9;
pub const SIGHUP: c_int = 1;
pub const SIGINT: c_int = 2;

// Error constants
pub const ERANGE: c_int = 34;

// sysconf keys
pub const _SC_GETPW_R_SIZE_MAX: c_int = 70;

// Process/user functions — all return error/stub values in WASM
pub unsafe fn getuid() -> uid_t {
    0
}

pub unsafe fn getpid() -> pid_t {
    1
}

pub unsafe fn sysconf(_name: c_int) -> c_long {
    -1 // Error: not available
}

pub unsafe fn getpwuid_r(
    _uid: uid_t,
    _pwd: *mut passwd,
    _buf: *mut c_char,
    _buflen: size_t,
    _result: *mut *mut passwd,
) -> c_int {
    // Set result to null (user not found)
    if !_result.is_null() {
        *_result = std::ptr::null_mut();
    }
    -1 // Error
}

// Process group functions
pub unsafe fn setpgid(_pid: pid_t, _pgid: pid_t) -> c_int {
    0
}

pub unsafe fn setsid() -> pid_t {
    1
}

pub unsafe fn kill(_pid: pid_t, _sig: c_int) -> c_int {
    0
}

/// ioctl stub — takes a third arg as a raw pointer (most common usage pattern).
/// C-variadic functions are unstable in Rust, so we use a fixed-arg version.
pub unsafe fn ioctl(_fd: c_int, _request: c_ulong, _arg: *mut c_void) -> c_int {
    -1
}

// tiocnotty
pub const TIOCNOTTY: c_ulong = 0x5422;

// prctl (Linux)
pub const PR_SET_PDEATHSIG: c_int = 1;
pub unsafe fn prctl(_option: c_int, _arg2: c_ulong, _arg3: c_ulong, _arg4: c_ulong, _arg5: c_ulong) -> c_int {
    0
}

// waitpid
pub const WNOHANG: c_int = 1;
pub unsafe fn waitpid(_pid: pid_t, _status: *mut c_int, _options: c_int) -> pid_t {
    -1
}

// Terminal detection (is-terminal crate)
pub unsafe fn isatty(_fd: c_int) -> c_int {
    1 // Pretend stdout is a terminal (for TUI rendering)
}

// pthread stubs (parking_lot_core)
pub type pthread_mutex_t = c_int;
pub type pthread_cond_t = c_int;
pub type pthread_mutexattr_t = c_int;
pub type pthread_condattr_t = c_int;

pub const PTHREAD_MUTEX_INITIALIZER: pthread_mutex_t = 0;
pub const PTHREAD_COND_INITIALIZER: pthread_cond_t = 0;

pub unsafe fn pthread_mutex_init(_m: *mut pthread_mutex_t, _a: *const pthread_mutexattr_t) -> c_int { 0 }
pub unsafe fn pthread_mutex_lock(_m: *mut pthread_mutex_t) -> c_int { 0 }
pub unsafe fn pthread_mutex_unlock(_m: *mut pthread_mutex_t) -> c_int { 0 }
pub unsafe fn pthread_mutex_destroy(_m: *mut pthread_mutex_t) -> c_int { 0 }
pub unsafe fn pthread_cond_init(_c: *mut pthread_cond_t, _a: *const pthread_condattr_t) -> c_int { 0 }
pub unsafe fn pthread_cond_wait(_c: *mut pthread_cond_t, _m: *mut pthread_mutex_t) -> c_int { 0 }
pub unsafe fn pthread_cond_signal(_c: *mut pthread_cond_t) -> c_int { 0 }
pub unsafe fn pthread_cond_broadcast(_c: *mut pthread_cond_t) -> c_int { 0 }
pub unsafe fn pthread_cond_destroy(_c: *mut pthread_cond_t) -> c_int { 0 }
pub unsafe fn pthread_cond_timedwait(
    _c: *mut pthread_cond_t,
    _m: *mut pthread_mutex_t,
    _t: *const timespec,
) -> c_int {
    ETIMEDOUT // Always "timed out" since we can't actually wait
}

// Time types
pub type time_t = i64;

#[repr(C)]
pub struct timespec {
    pub tv_sec: time_t,
    pub tv_nsec: c_long,
}

#[repr(C)]
pub struct timeval {
    pub tv_sec: time_t,
    pub tv_usec: c_long,
}

pub unsafe fn gettimeofday(tp: *mut timeval, _tz: *mut c_void) -> c_int {
    if !tp.is_null() {
        (*tp).tv_sec = 0;
        (*tp).tv_usec = 0;
    }
    0
}

// Error constants
pub const ETIMEDOUT: c_int = 110;
pub const EINVAL: c_int = 22;

// sysctl (macOS)
pub unsafe fn sysctlbyname(
    _name: *const c_char,
    _oldp: *mut c_void,
    _oldlenp: *mut size_t,
    _newp: *const c_void,
    _newlen: size_t,
) -> c_int {
    -1 // Not available
}
