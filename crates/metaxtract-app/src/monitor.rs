//! Shared monitor configuration excludes the case, checkpoint and metadata cache.
use crate::core::scan::*;
use std::{
    io,
    path::{Path, PathBuf},
};

pub fn start(
    source: PathBuf,
    case_path: &Path,
    include_hidden: bool,
    control: &ScanControl,
) -> io::Result<DirectoryMonitor> {
    let root = ScanRoot::open(&source, SymlinkPolicy::Skip)?;
    let case = case_path.canonicalize()?;
    let mut journal = case.as_os_str().to_owned();
    journal.push(".scan.sqlite");
    let cache = crate::workflows::cache_path(&case);
    let mut options = MonitorOptions::default();
    options.walk.include_hidden = include_hidden;
    for path in [case, PathBuf::from(journal), cache] {
        for suffix in ["", "-journal", "-wal", "-shm"] {
            let mut name = path.as_os_str().to_owned();
            name.push(suffix);
            if let Some(relative) = root.relative_to_root(Path::new(&name)) {
                options.walk.excluded.push(relative);
            }
        }
    }
    DirectoryMonitor::new(source, options, control)
}
