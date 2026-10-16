include!("../../native/runtime.rs");

fn main() {
    bundle_runtime_paths();
    println!("cargo:rerun-if-env-changed=METAXTRACT_SANITIZERS");
    if std::env::var("METAXTRACT_SANITIZERS").as_deref() == Ok("address,undefined") {
        // Declare the runtime on this binary, before libc from transitive Rust
        // dependencies. The native library's link-arg does not propagate here.
        println!("cargo:rustc-link-lib=dylib=asan");
        println!("cargo:rustc-link-lib=dylib=ubsan");
    }
}
