use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let native = root.join("native/metaxtract-extract");

    cxx_build::bridge("src/ffi.rs")
        .file(native.join("src/budget.cpp"))
        .file(native.join("src/bridge.cpp"))
        .file(native.join("src/core.cpp"))
        .file(native.join("src/registry.cpp"))
        .include(native.join("include"))
        .std("c++20")
        .warnings(true)
        .compile("metaxtract-native-bridge");

    for path in [
        "src/ffi.rs",
        "../../native/metaxtract-extract/include/metaxtract/native/bridge.hpp",
        "../../native/metaxtract-extract/include/metaxtract/native/core.hpp",
        "../../native/metaxtract-extract/src/bridge.cpp",
        "../../native/metaxtract-extract/src/budget.cpp",
        "../../native/metaxtract-extract/src/core.cpp",
        "../../native/metaxtract-extract/src/registry.cpp",
        "../../native/metaxtract-extract/include/metaxtract/native/input.hpp",
        "../../native/metaxtract-extract/include/metaxtract/native/budget.hpp",
        "../../native/metaxtract-extract/include/metaxtract/native/parser.hpp",
        "../../native/metaxtract-extract/include/metaxtract/native/registry.hpp",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
}
