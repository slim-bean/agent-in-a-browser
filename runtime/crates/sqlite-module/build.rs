fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if !target.starts_with("wasm32-wasi") {
        return;
    }

    // libsqlite3-sys (bundled) compiles sqlite3.c with _WASI_EMULATED_* defines
    // (set via CFLAGS_wasm32_wasip2 in moon.yml). The corresponding emulation
    // libraries must be linked. We also need a -L search path pointing to the
    // wasi-sdk sysroot so the linker can find them.
    if let Ok(sdk) = std::env::var("WASI_SDK_PATH") {
        let sub = if target.contains("wasip2") {
            "wasm32-wasip2"
        } else if target.contains("wasip1") {
            "wasm32-wasip1"
        } else {
            "wasm32-wasi"
        };
        println!(
            "cargo:rustc-link-search=native={}/share/wasi-sysroot/lib/{}",
            sdk, sub
        );
    }

    println!("cargo:rustc-link-lib=static=wasi-emulated-mman");
    println!("cargo:rustc-link-lib=static=wasi-emulated-getpid");
    println!("cargo:rustc-link-lib=static=wasi-emulated-signal");
    println!("cargo:rustc-link-lib=static=wasi-emulated-process-clocks");
}
