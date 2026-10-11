use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let native = root.join("native/metaxtract-extract");

    cxx_build::bridge("src/ffi.rs")
        .file(native.join("src/bridge.cpp"))
        .file(native.join("src/core.cpp"))
        .include(native.join("include"))
        .std("c++20")
        .warnings(true)
        .compile("metaxtract-native-bridge");

    for path in [
        "src/ffi.rs",
        "../../native/metaxtract-extract/include/metaxtract/native/bridge.hpp",
        "../../native/metaxtract-extract/include/metaxtract/native/core.hpp",
        "../../native/metaxtract-extract/src/bridge.cpp",
        "../../native/metaxtract-extract/src/core.cpp",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
}
