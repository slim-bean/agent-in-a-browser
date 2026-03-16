/**
 * LD_PRELOAD shim to disable pidfd_open() syscall.
 *
 * tokio >= 1.36 uses pidfd_open() on Linux to monitor child processes.
 * In containerized CI environments (GitHub Actions, LXC), the pidfd may
 * never become readable when a child exits, causing tokio's process wait
 * to hang indefinitely. This shim intercepts the libc syscall() wrapper
 * and returns ENOSYS for SYS_pidfd_open, forcing tokio to fall back to
 * reliable SIGCHLD-based reaping.
 *
 * Build:  gcc -shared -fPIC -o no_pidfd.so scripts/no_pidfd.c -ldl
 * Usage:  LD_PRELOAD=./no_pidfd.so moon run ...
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <stdarg.h>
#include <sys/syscall.h>
#include <unistd.h>

typedef long (*real_syscall_fn)(long, ...);

long syscall(long number, ...) {
    /* Block pidfd_open — force SIGCHLD-based process reaping */
    if (number == SYS_pidfd_open) {
        errno = ENOSYS;
        return -1;
    }

    /* Forward everything else to the real syscall() */
    static real_syscall_fn real_syscall = NULL;
    if (!real_syscall) {
        real_syscall = (real_syscall_fn)dlsym(RTLD_NEXT, "syscall");
    }

    va_list args;
    va_start(args, number);
    long a1 = va_arg(args, long);
    long a2 = va_arg(args, long);
    long a3 = va_arg(args, long);
    long a4 = va_arg(args, long);
    long a5 = va_arg(args, long);
    long a6 = va_arg(args, long);
    va_end(args);

    return real_syscall(number, a1, a2, a3, a4, a5, a6);
}
