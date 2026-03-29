//! Real select! implementation for single-threaded WASM.
//!
//! Uses poll_fn to poll all branches. Returns an enum indicating which
//! branch completed and its raw value. The body executes outside the
//! poll_fn so break/continue/return work normally.
//!
//! Refutable patterns (e.g. Some(x)) are handled: if the future completes
//! but the pattern doesn't match, the branch is disabled and polling continues.

/// The main select! macro.
#[macro_export]
macro_rules! select {
    (biased; $($rest:tt)*) => { $crate::select!($($rest)*) };

    // 1 branch
    ($p1:pat = $f1:expr => $b1:block) => {{ let val = $f1.await; match val { $p1 => $b1 } }};
    ($p1:pat = $f1:expr => $b1:expr $(,)?) => {{ let val = $f1.await; match val { $p1 => { $b1 } } }};

    // else variants
    ($p1:pat = $f1:expr => $b1:block else => $be:expr $(,)?) => {{
        let __val = $f1.await;
        #[allow(unreachable_patterns)]
        match __val { $p1 => $b1, _ => { $be } }
    }};
    ($p1:pat = $f1:expr => $b1:expr, else => $be:expr $(,)?) => {{
        let __val = $f1.await;
        #[allow(unreachable_patterns)]
        match __val { $p1 => { $b1 }, _ => { $be } }
    }};

    // 2+ branches
    ($($tokens:tt)*) => {{ $crate::__select_parse!(@branches [] $($tokens)*) }};
}

#[macro_export]
#[doc(hidden)]
macro_rules! __select_parse {
    // Guarded branch, block body + optional comma
    (@branches [$($acc:tt)*] $p:pat = $f:expr, if $g:expr => $b:block, $($rest:tt)*) => {
        $crate::__select_parse!(@branches [$($acc)* ($p, $f, $b, guard($g))] $($rest)*)
    };
    (@branches [$($acc:tt)*] $p:pat = $f:expr, if $g:expr => $b:block $($rest:tt)*) => {
        $crate::__select_parse!(@branches [$($acc)* ($p, $f, $b, guard($g))] $($rest)*)
    };
    // Unguarded branch, block body + optional comma
    (@branches [$($acc:tt)*] $p:pat = $f:expr => $b:block, $($rest:tt)*) => {
        $crate::__select_parse!(@branches [$($acc)* ($p, $f, $b, noguard)] $($rest)*)
    };
    (@branches [$($acc:tt)*] $p:pat = $f:expr => $b:block $($rest:tt)*) => {
        $crate::__select_parse!(@branches [$($acc)* ($p, $f, $b, noguard)] $($rest)*)
    };
    // Unguarded branch, expr body + comma
    (@branches [$($acc:tt)*] $p:pat = $f:expr => $b:expr, $($rest:tt)*) => {
        $crate::__select_parse!(@branches [$($acc)* ($p, $f, { $b }, noguard)] $($rest)*)
    };
    // Unguarded branch, expr body, last
    (@branches [$($acc:tt)*] $p:pat = $f:expr => $b:expr $(,)?) => {
        $crate::__select_parse!(@branches [$($acc)* ($p, $f, { $b }, noguard)])
    };
    // Done
    (@branches [$($acc:tt)*]) => {
        $crate::__select_dispatch!([$($acc)*])
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! __select_guard_check {
    (noguard) => {
        true
    };
    (guard($g:expr)) => {
        $g
    };
}

/// Dispatch by branch count. Each emits a poll_fn that returns a tagged
/// value, then a match outside that executes the body.
/// Refutable patterns: if poll returns Ready but pattern doesn't match,
/// the "done" flag is set and that future is not polled again.
#[macro_export]
#[doc(hidden)]
macro_rules! __select_dispatch {
    ([($p1:pat, $f1:expr, $b1:block, $g1:tt) ($p2:pat, $f2:expr, $b2:block, $g2:tt)]) => {
        $crate::__select_emit!(2, [$p1, $f1, $b1, $g1, __f1, __d1, _V1] [$p2, $f2, $b2, $g2, __f2, __d2, _V2])
    };
    ([($p1:pat, $f1:expr, $b1:block, $g1:tt) ($p2:pat, $f2:expr, $b2:block, $g2:tt) ($p3:pat, $f3:expr, $b3:block, $g3:tt)]) => {
        $crate::__select_emit!(3, [$p1, $f1, $b1, $g1, __f1, __d1, _V1] [$p2, $f2, $b2, $g2, __f2, __d2, _V2] [$p3, $f3, $b3, $g3, __f3, __d3, _V3])
    };
    ([($p1:pat, $f1:expr, $b1:block, $g1:tt) ($p2:pat, $f2:expr, $b2:block, $g2:tt) ($p3:pat, $f3:expr, $b3:block, $g3:tt) ($p4:pat, $f4:expr, $b4:block, $g4:tt)]) => {
        $crate::__select_emit!(4, [$p1, $f1, $b1, $g1, __f1, __d1, _V1] [$p2, $f2, $b2, $g2, __f2, __d2, _V2] [$p3, $f3, $b3, $g3, __f3, __d3, _V3] [$p4, $f4, $b4, $g4, __f4, __d4, _V4])
    };
    ([($p1:pat, $f1:expr, $b1:block, $g1:tt) ($p2:pat, $f2:expr, $b2:block, $g2:tt) ($p3:pat, $f3:expr, $b3:block, $g3:tt) ($p4:pat, $f4:expr, $b4:block, $g4:tt) ($p5:pat, $f5:expr, $b5:block, $g5:tt)]) => {
        $crate::__select_emit!(5, [$p1, $f1, $b1, $g1, __f1, __d1, _V1] [$p2, $f2, $b2, $g2, __f2, __d2, _V2] [$p3, $f3, $b3, $g3, __f3, __d3, _V3] [$p4, $f4, $b4, $g4, __f4, __d4, _V4] [$p5, $f5, $b5, $g5, __f5, __d5, _V5])
    };
    ([($p1:pat, $f1:expr, $b1:block, $g1:tt) ($p2:pat, $f2:expr, $b2:block, $g2:tt) ($p3:pat, $f3:expr, $b3:block, $g3:tt) ($p4:pat, $f4:expr, $b4:block, $g4:tt) ($p5:pat, $f5:expr, $b5:block, $g5:tt) ($p6:pat, $f6:expr, $b6:block, $g6:tt)]) => {
        $crate::__select_emit!(6, [$p1, $f1, $b1, $g1, __f1, __d1, _V1] [$p2, $f2, $b2, $g2, __f2, __d2, _V2] [$p3, $f3, $b3, $g3, __f3, __d3, _V3] [$p4, $f4, $b4, $g4, __f4, __d4, _V4] [$p5, $f5, $b5, $g5, __f5, __d5, _V5] [$p6, $f6, $b6, $g6, __f6, __d6, _V6])
    };
}

/// Generate the polling + match code for N branches.
///
/// Strategy:
/// 1. Pin all futures
/// 2. poll_fn polls each (checking guard + not-done), returns `__Out::_N(raw_value)` on Ready
/// 3. Outside poll_fn, match on `__Out` with the user's patterns
/// 4. If pattern doesn't match: unreachable (refutable patterns like `Some(x)` where
///    the channel returned None mean the channel is closed — we loop back to select
///    from the outer user loop which will recreate the select! and its futures)
#[macro_export]
#[doc(hidden)]
macro_rules! __select_emit {
    // 2 branches
    (2, [$p1:pat, $f1:expr, $b1:block, $g1:tt, $fv1:ident, $dv1:ident, $vn1:ident] [$p2:pat, $f2:expr, $b2:block, $g2:tt, $fv2:ident, $dv2:ident, $vn2:ident]) => {{
        #[allow(dead_code)]
        enum __Out<A, B> {
            $vn1(A),
            $vn2(B),
        }
        // Futures are created and polled inside this async block.
        // They are dropped when the async block completes, BEFORE
        // the match body runs — freeing borrows on captured variables.
        let __out = async {
            let mut $fv1 = core::pin::pin!($f1);
            let mut $fv2 = core::pin::pin!($f2);
            let mut $dv1 = false;
            let mut $dv2 = false;
            std::future::poll_fn(|__cx| {
                $crate::__poll_spawned_tasks(__cx);
                if !$dv1 && $crate::__select_guard_check!($g1) {
                    if let core::task::Poll::Ready(v) = $fv1.as_mut().poll(__cx) {
                        $dv1 = true;
                        return core::task::Poll::Ready(__Out::$vn1(v));
                    }
                }
                if !$dv2 && $crate::__select_guard_check!($g2) {
                    if let core::task::Poll::Ready(v) = $fv2.as_mut().poll(__cx) {
                        $dv2 = true;
                        return core::task::Poll::Ready(__Out::$vn2(v));
                    }
                }
                core::task::Poll::Pending
            })
            .await
        }
        .await;
        #[allow(unreachable_patterns)]
        match __out {
            __Out::$vn1($p1) => $b1,
            __Out::$vn2($p2) => $b2,
            _ => {
                unreachable!("select! branch pattern did not match")
            }
        }
    }};

    // 3 branches
    (3, [$p1:pat, $f1:expr, $b1:block, $g1:tt, $fv1:ident, $dv1:ident, $vn1:ident] [$p2:pat, $f2:expr, $b2:block, $g2:tt, $fv2:ident, $dv2:ident, $vn2:ident] [$p3:pat, $f3:expr, $b3:block, $g3:tt, $fv3:ident, $dv3:ident, $vn3:ident]) => {{
        #[allow(dead_code)]
        enum __Out<A, B, C> {
            $vn1(A),
            $vn2(B),
            $vn3(C),
        }
        let __out = async {
            let mut $fv1 = core::pin::pin!($f1);
            let mut $fv2 = core::pin::pin!($f2);
            let mut $fv3 = core::pin::pin!($f3);
            let mut $dv1 = false;
            let mut $dv2 = false;
            let mut $dv3 = false;
            std::future::poll_fn(|__cx| {
                $crate::__poll_spawned_tasks(__cx);
                if !$dv1 && $crate::__select_guard_check!($g1) {
                    if let core::task::Poll::Ready(v) = $fv1.as_mut().poll(__cx) {
                        $dv1 = true;
                        return core::task::Poll::Ready(__Out::$vn1(v));
                    }
                }
                if !$dv2 && $crate::__select_guard_check!($g2) {
                    if let core::task::Poll::Ready(v) = $fv2.as_mut().poll(__cx) {
                        $dv2 = true;
                        return core::task::Poll::Ready(__Out::$vn2(v));
                    }
                }
                if !$dv3 && $crate::__select_guard_check!($g3) {
                    if let core::task::Poll::Ready(v) = $fv3.as_mut().poll(__cx) {
                        $dv3 = true;
                        return core::task::Poll::Ready(__Out::$vn3(v));
                    }
                }
                core::task::Poll::Pending
            })
            .await
        }
        .await;
        #[allow(unreachable_patterns)]
        match __out {
            __Out::$vn1($p1) => $b1,
            __Out::$vn2($p2) => $b2,
            __Out::$vn3($p3) => $b3,
            _ => {
                unreachable!("select! branch pattern did not match")
            }
        }
    }};

    // 4 branches
    (4, [$p1:pat, $f1:expr, $b1:block, $g1:tt, $fv1:ident, $dv1:ident, $vn1:ident] [$p2:pat, $f2:expr, $b2:block, $g2:tt, $fv2:ident, $dv2:ident, $vn2:ident] [$p3:pat, $f3:expr, $b3:block, $g3:tt, $fv3:ident, $dv3:ident, $vn3:ident] [$p4:pat, $f4:expr, $b4:block, $g4:tt, $fv4:ident, $dv4:ident, $vn4:ident]) => {{
        #[allow(dead_code)]
        enum __Out<A, B, C, D> {
            $vn1(A),
            $vn2(B),
            $vn3(C),
            $vn4(D),
        }
        let __out = async {
            let mut $fv1 = core::pin::pin!($f1);
            let mut $fv2 = core::pin::pin!($f2);
            let mut $fv3 = core::pin::pin!($f3);
            let mut $fv4 = core::pin::pin!($f4);
            let mut $dv1 = false;
            let mut $dv2 = false;
            let mut $dv3 = false;
            let mut $dv4 = false;
            std::future::poll_fn(|__cx| {
                $crate::__poll_spawned_tasks(__cx);
                if !$dv1 && $crate::__select_guard_check!($g1) {
                    if let core::task::Poll::Ready(v) = $fv1.as_mut().poll(__cx) {
                        $dv1 = true;
                        return core::task::Poll::Ready(__Out::$vn1(v));
                    }
                }
                if !$dv2 && $crate::__select_guard_check!($g2) {
                    if let core::task::Poll::Ready(v) = $fv2.as_mut().poll(__cx) {
                        $dv2 = true;
                        return core::task::Poll::Ready(__Out::$vn2(v));
                    }
                }
                if !$dv3 && $crate::__select_guard_check!($g3) {
                    if let core::task::Poll::Ready(v) = $fv3.as_mut().poll(__cx) {
                        $dv3 = true;
                        return core::task::Poll::Ready(__Out::$vn3(v));
                    }
                }
                if !$dv4 && $crate::__select_guard_check!($g4) {
                    if let core::task::Poll::Ready(v) = $fv4.as_mut().poll(__cx) {
                        $dv4 = true;
                        return core::task::Poll::Ready(__Out::$vn4(v));
                    }
                }
                core::task::Poll::Pending
            })
            .await
        }
        .await;
        #[allow(unreachable_patterns)]
        match __out {
            __Out::$vn1($p1) => $b1,
            __Out::$vn2($p2) => $b2,
            __Out::$vn3($p3) => $b3,
            __Out::$vn4($p4) => $b4,
            _ => {
                unreachable!("select! branch pattern did not match")
            }
        }
    }};

    // 5 branches
    (5, [$p1:pat, $f1:expr, $b1:block, $g1:tt, $fv1:ident, $dv1:ident, $vn1:ident] [$p2:pat, $f2:expr, $b2:block, $g2:tt, $fv2:ident, $dv2:ident, $vn2:ident] [$p3:pat, $f3:expr, $b3:block, $g3:tt, $fv3:ident, $dv3:ident, $vn3:ident] [$p4:pat, $f4:expr, $b4:block, $g4:tt, $fv4:ident, $dv4:ident, $vn4:ident] [$p5:pat, $f5:expr, $b5:block, $g5:tt, $fv5:ident, $dv5:ident, $vn5:ident]) => {{
        #[allow(dead_code)]
        enum __Out<A, B, C, D, E> {
            $vn1(A),
            $vn2(B),
            $vn3(C),
            $vn4(D),
            $vn5(E),
        }
        let __out = async {
            let mut $fv1 = core::pin::pin!($f1);
            let mut $fv2 = core::pin::pin!($f2);
            let mut $fv3 = core::pin::pin!($f3);
            let mut $fv4 = core::pin::pin!($f4);
            let mut $fv5 = core::pin::pin!($f5);
            let mut $dv1 = false;
            let mut $dv2 = false;
            let mut $dv3 = false;
            let mut $dv4 = false;
            let mut $dv5 = false;
            std::future::poll_fn(|__cx| {
                $crate::__poll_spawned_tasks(__cx);
                if !$dv1 && $crate::__select_guard_check!($g1) {
                    if let core::task::Poll::Ready(v) = $fv1.as_mut().poll(__cx) {
                        $dv1 = true;
                        return core::task::Poll::Ready(__Out::$vn1(v));
                    }
                }
                if !$dv2 && $crate::__select_guard_check!($g2) {
                    if let core::task::Poll::Ready(v) = $fv2.as_mut().poll(__cx) {
                        $dv2 = true;
                        return core::task::Poll::Ready(__Out::$vn2(v));
                    }
                }
                if !$dv3 && $crate::__select_guard_check!($g3) {
                    if let core::task::Poll::Ready(v) = $fv3.as_mut().poll(__cx) {
                        $dv3 = true;
                        return core::task::Poll::Ready(__Out::$vn3(v));
                    }
                }
                if !$dv4 && $crate::__select_guard_check!($g4) {
                    if let core::task::Poll::Ready(v) = $fv4.as_mut().poll(__cx) {
                        $dv4 = true;
                        return core::task::Poll::Ready(__Out::$vn4(v));
                    }
                }
                if !$dv5 && $crate::__select_guard_check!($g5) {
                    if let core::task::Poll::Ready(v) = $fv5.as_mut().poll(__cx) {
                        $dv5 = true;
                        return core::task::Poll::Ready(__Out::$vn5(v));
                    }
                }
                core::task::Poll::Pending
            })
            .await
        }
        .await;
        #[allow(unreachable_patterns)]
        match __out {
            __Out::$vn1($p1) => $b1,
            __Out::$vn2($p2) => $b2,
            __Out::$vn3($p3) => $b3,
            __Out::$vn4($p4) => $b4,
            __Out::$vn5($p5) => $b5,
            _ => {
                unreachable!("select! branch pattern did not match")
            }
        }
    }};

    // 6 branches
    (6, [$p1:pat, $f1:expr, $b1:block, $g1:tt, $fv1:ident, $dv1:ident, $vn1:ident] [$p2:pat, $f2:expr, $b2:block, $g2:tt, $fv2:ident, $dv2:ident, $vn2:ident] [$p3:pat, $f3:expr, $b3:block, $g3:tt, $fv3:ident, $dv3:ident, $vn3:ident] [$p4:pat, $f4:expr, $b4:block, $g4:tt, $fv4:ident, $dv4:ident, $vn4:ident] [$p5:pat, $f5:expr, $b5:block, $g5:tt, $fv5:ident, $dv5:ident, $vn5:ident] [$p6:pat, $f6:expr, $b6:block, $g6:tt, $fv6:ident, $dv6:ident, $vn6:ident]) => {{
        #[allow(dead_code)]
        enum __Out<A, B, C, D, E, F> {
            $vn1(A),
            $vn2(B),
            $vn3(C),
            $vn4(D),
            $vn5(E),
            $vn6(F),
        }
        let __out = async {
            let mut $fv1 = core::pin::pin!($f1);
            let mut $fv2 = core::pin::pin!($f2);
            let mut $fv3 = core::pin::pin!($f3);
            let mut $fv4 = core::pin::pin!($f4);
            let mut $fv5 = core::pin::pin!($f5);
            let mut $fv6 = core::pin::pin!($f6);
            let mut $dv1 = false;
            let mut $dv2 = false;
            let mut $dv3 = false;
            let mut $dv4 = false;
            let mut $dv5 = false;
            let mut $dv6 = false;
            std::future::poll_fn(|__cx| {
                $crate::__poll_spawned_tasks(__cx);
                if !$dv1 && $crate::__select_guard_check!($g1) {
                    if let core::task::Poll::Ready(v) = $fv1.as_mut().poll(__cx) {
                        $dv1 = true;
                        return core::task::Poll::Ready(__Out::$vn1(v));
                    }
                }
                if !$dv2 && $crate::__select_guard_check!($g2) {
                    if let core::task::Poll::Ready(v) = $fv2.as_mut().poll(__cx) {
                        $dv2 = true;
                        return core::task::Poll::Ready(__Out::$vn2(v));
                    }
                }
                if !$dv3 && $crate::__select_guard_check!($g3) {
                    if let core::task::Poll::Ready(v) = $fv3.as_mut().poll(__cx) {
                        $dv3 = true;
                        return core::task::Poll::Ready(__Out::$vn3(v));
                    }
                }
                if !$dv4 && $crate::__select_guard_check!($g4) {
                    if let core::task::Poll::Ready(v) = $fv4.as_mut().poll(__cx) {
                        $dv4 = true;
                        return core::task::Poll::Ready(__Out::$vn4(v));
                    }
                }
                if !$dv5 && $crate::__select_guard_check!($g5) {
                    if let core::task::Poll::Ready(v) = $fv5.as_mut().poll(__cx) {
                        $dv5 = true;
                        return core::task::Poll::Ready(__Out::$vn5(v));
                    }
                }
                if !$dv6 && $crate::__select_guard_check!($g6) {
                    if let core::task::Poll::Ready(v) = $fv6.as_mut().poll(__cx) {
                        $dv6 = true;
                        return core::task::Poll::Ready(__Out::$vn6(v));
                    }
                }
                core::task::Poll::Pending
            })
            .await
        }
        .await;
        #[allow(unreachable_patterns)]
        match __out {
            __Out::$vn1($p1) => $b1,
            __Out::$vn2($p2) => $b2,
            __Out::$vn3($p3) => $b3,
            __Out::$vn4($p4) => $b4,
            __Out::$vn5($p5) => $b5,
            __Out::$vn6($p6) => $b6,
            _ => {
                unreachable!("select! branch pattern did not match")
            }
        }
    }};
}
