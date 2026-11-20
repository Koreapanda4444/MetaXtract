include!("../../native/runtime.rs");

include!("../../native/windows_resources.rs");

fn main() {
    bundle_runtime_paths();
    windows_resources("metaxtract-gui.exe");
}
