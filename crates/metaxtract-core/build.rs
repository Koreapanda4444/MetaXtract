use sha2::{Digest, Sha256};
use std::{
    env, fs,
    path::{Path, PathBuf},
};
fn collect(path: &Path, files: &mut Vec<PathBuf>) {
    if path.is_dir() {
        for item in fs::read_dir(path).expect("read engine source") {
            collect(&item.expect("source entry").path(), files);
        }
    } else if path.is_file() {
        files.push(path.to_owned());
    }
}
fn main() {
    let root =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory")).join("../..");
    let mut files = Vec::new();
    for path in [
        "Cargo.toml",
        "Cargo.lock",
        "third-party.lock.toml",
        "crates/metaxtract-core/src",
        "crates/metaxtract-core/build.rs",
        "crates/metaxtract-core/Cargo.toml",
        "crates/metaxtract-schema/src",
        "crates/metaxtract-schema/Cargo.toml",
        "crates/metaxtract-native/src",
        "crates/metaxtract-native/build.rs",
        "crates/metaxtract-native/Cargo.toml",
        "crates/metaxtract-worker/src",
        "crates/metaxtract-worker/build.rs",
        "crates/metaxtract-worker/Cargo.toml",
        "native/metaxtract-extract",
        "CMakeLists.txt",
    ] {
        println!("cargo:rerun-if-changed=../../{path}");
        collect(&root.join(path), &mut files);
    }
    files.sort();
    let mut hash = Sha256::new();
    for path in files {
        let name = path
            .strip_prefix(&root)
            .expect("source root")
            .to_string_lossy()
            .replace('\\', "/");
        let bytes = fs::read(path).expect("read engine input");
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    for name in ["TARGET", "PROFILE", "OPT_LEVEL", "METAXTRACT_SANITIZERS"] {
        println!("cargo:rerun-if-env-changed={name}");
        hash.update(name.as_bytes());
        hash.update(env::var(name).unwrap_or_default().as_bytes());
    }
    println!(
        "cargo:rustc-env=METAXTRACT_ENGINE_FINGERPRINT={:x}",
        hash.finalize()
    );
}
