// Shared build-time policy for the three bundled Rust executables.
fn bundle_runtime_paths() {
    println!("cargo:rerun-if-changed=../../native/runtime.rs");
    match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("linux") => println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN/lib"),
        Ok("macos") => {
            println!("cargo:rustc-link-arg=-Wl,-rpath,@loader_path/lib");
            println!("cargo:rustc-link-arg=-Wl,-rpath,@loader_path/../Frameworks");
        }
        _ => {} // Windows loads bundled DLLs beside each executable.
    }
}
