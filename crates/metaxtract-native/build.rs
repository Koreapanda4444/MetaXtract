use std::{
    collections::hash_map::DefaultHasher,
    env,
    hash::{Hash, Hasher},
    path::PathBuf,
};

fn ci_build_root() -> Option<PathBuf> {
    // A single native CI job invokes Cargo sequentially with one immutable
    // checkout/toolchain. Reuse its CMake outputs across Cargo feature graphs.
    // Profiles and ABI flags stay separate; installed outputs are snapshotted
    // per Cargo build before another invocation can change the fixture setting.
    env::var_os("GITHUB_RUN_ID")?;
    let root = PathBuf::from(env::var_os("METAXTRACT_NATIVE_BUILD_ROOT")?);
    let mut key = DefaultHasher::new();
    for name in [
        "TARGET",
        "HOST",
        "PROFILE",
        "OPT_LEVEL",
        "DEBUG",
        "CARGO_CFG_TARGET_FEATURE",
        "CARGO_ENCODED_RUSTFLAGS",
        "METAXTRACT_SANITIZERS",
        "CC",
        "CXX",
        "AR",
        "CFLAGS",
        "CXXFLAGS",
        "LDFLAGS",
        "CMAKE_TOOLCHAIN_FILE",
        "CMAKE_GENERATOR",
        "MACOSX_DEPLOYMENT_TARGET",
        "VCToolsInstallDir",
        "WindowsSdkDir",
        "WindowsSDKVersion",
    ] {
        name.hash(&mut key);
        env::var_os(name).hash(&mut key);
    }
    Some(root.join(format!("{:016x}", key.finish())))
}

fn copy_tree(source: &std::path::Path, destination: &std::path::Path) {
    std::fs::create_dir_all(destination).expect("create native snapshot directory");
    for entry in std::fs::read_dir(source).expect("read native build outputs") {
        let entry = entry.expect("native build output");
        let target = destination.join(entry.file_name());
        if entry.file_type().expect("native output type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).expect("snapshot native build output");
        }
    }
}

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let native = root.join("native/metaxtract-extract");
    let msvc = env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    let static_crt = env::var("CARGO_CFG_TARGET_FEATURE")
        .unwrap_or_default()
        .split(',')
        .any(|feature| feature == "crt-static");
    let sanitizers = env::var("METAXTRACT_SANITIZERS").unwrap_or_default();
    assert!(
        sanitizers.is_empty() || sanitizers == "address,undefined",
        "METAXTRACT_SANITIZERS must be empty or address,undefined"
    );
    if !sanitizers.is_empty() {
        assert_eq!(
            env::var("CARGO_CFG_TARGET_OS").as_deref(),
            Ok("linux"),
            "Rust/C++ sanitizer builds currently require Linux"
        );
        println!("cargo:rustc-link-lib=dylib=asan");
        println!("cargo:rustc-link-lib=dylib=ubsan");
    }

    let mut config = cmake::Config::new(&root);
    println!("cargo:rerun-if-env-changed=METAXTRACT_NATIVE_BUILD_ROOT");
    println!("cargo:rerun-if-env-changed=GITHUB_RUN_ID");
    let shared = ci_build_root();
    if let Some(shared) = &shared {
        config.out_dir(shared);
    }
    if msvc {
        config
            .static_crt(static_crt)
            .cxxflag("/D_ITERATOR_DEBUG_LEVEL=0");
        config.define(
            "CMAKE_MSVC_RUNTIME_LIBRARY",
            if static_crt {
                "MultiThreaded"
            } else {
                "MultiThreadedDLL"
            },
        );
        config.define(
            "METAXTRACT_STATIC_CRT",
            if static_crt { "ON" } else { "OFF" },
        );
    }
    let mut built = config
        .define("BUILD_TESTING", "OFF")
        .define("METAXTRACT_WARNINGS_AS_ERRORS", "ON")
        .define("METAXTRACT_SANITIZERS", &sanitizers)
        .define(
            "METAXTRACT_CONFORMANCE",
            if env::var_os("CARGO_FEATURE_CONFORMANCE").is_some() {
                "ON"
            } else {
                "OFF"
            },
        )
        .build();
    if shared.is_some() {
        let snapshot =
            PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output")).join("native-snapshot");
        if snapshot.exists() {
            std::fs::remove_dir_all(&snapshot).expect("replace native snapshot");
        }
        for directory in ["lib", "share"] {
            copy_tree(&built.join(directory), &snapshot.join(directory));
        }
        built = snapshot;
    }

    let mut bridge = cxx_build::bridge("src/ffi.rs");
    bridge
        .file(native.join("src/bridge.cpp"))
        .include(native.join("include"))
        .std("c++20")
        .warnings(true)
        .warnings_into_errors(true);
    if msvc {
        bridge
            .flag("/EHsc")
            .define("_ITERATOR_DEBUG_LEVEL", "0")
            .static_crt(static_crt);
    }
    if !sanitizers.is_empty() {
        bridge
            .flag("-fsanitize=address,undefined")
            .flag("-fno-omit-frame-pointer");
        println!("cargo:rustc-link-arg=-fsanitize=address,undefined");
    }
    bridge.compile("metaxtract-native-bridge");
    println!(
        "cargo:rustc-link-search=native={}",
        built.join("lib").display()
    );
    println!("cargo:rustc-link-lib=static=metaxtract_extract");
    println!("cargo:rustc-link-lib=static=mx_archive");
    println!("cargo:rustc-link-lib=static=mx_lzma");
    println!("cargo:rustc-link-lib=static=mx_bzip2");
    println!("cargo:rustc-link-lib=static=mx_pugixml");
    println!("cargo:rustc-link-lib=static=mx_qpdf");
    println!("cargo:rustc-link-lib=static=mx_jpeg");
    println!("cargo:rustc-link-lib=static=mx_zlib");
    println!("cargo:rustc-link-lib=dylib=mx_libraw");
    println!("cargo:rustc-link-lib=dylib=tag");
    for library in ["avformat", "avcodec", "avutil", "swscale", "swresample"] {
        println!("cargo:rustc-link-lib=dylib={library}");
    }
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        for library in [
            "ws2_32", "shell32", "advapi32", "gdi32", "user32", "crypt32", "bcrypt",
        ] {
            println!("cargo:rustc-link-lib={library}");
        }
    }
    println!("cargo:rerun-if-env-changed=METAXTRACT_SANITIZERS");

    for path in [
        "src/ffi.rs",
        "../../CMakeLists.txt",
        "../../third-party.lock.toml",
        "../../native/metaxtract-extract",
        "../../conformance/parser.cpp",
        "../../conformance/parser.hpp",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
}
