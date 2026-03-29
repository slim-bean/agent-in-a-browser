//! Stub — Linux landlock sandbox for wasip2.
#![allow(dead_code, unused_variables, unused_imports)]

pub fn create_linux_sandbox_command_args_for_policies<A, B, C, D, E, F, G, H>(
    _a: A, _b: B, _c: C, _d: D, _e: E, _f: F, _g: G, _h: H,
) -> Vec<String> {
    vec![]
}

pub fn allow_network_for_proxy(_enforce: bool) -> bool {
    false
}
