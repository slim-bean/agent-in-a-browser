//! Stub — macOS seatbelt sandbox for wasip2.
#![allow(dead_code, unused_variables, unused_imports)]

pub const MACOS_PATH_TO_SEATBELT_EXECUTABLE: &str = "/usr/bin/sandbox-exec";

pub fn create_seatbelt_command_args_for_policies_with_extensions<A, B, C, D, E, F, G>(
    _a: A, _b: B, _c: C, _d: D, _e: E, _f: F, _g: G,
) -> Vec<String> {
    vec![]
}

pub fn allow_network_for_proxy(_enforce: bool) -> bool {
    false
}
