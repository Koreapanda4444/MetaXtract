//! Opt-in, bounded polling. Events schedule capability-based rescans; paths in
//! notifications are never trusted as already-open evidence.
use super::{
    FileStamp, HashOptions, NativePath, ScanControl, ScanRoot, SymlinkPolicy, WalkEntry,
    WalkOptions, hash_evidence,
};
use crate::schema::ContentDigest;
use std::{collections::BTreeMap, io, path::PathBuf, sync::Arc};

#[derive(Clone, Debug)]
pub struct MonitorOptions {
    pub walk: WalkOptions,
    pub hash: HashOptions,
    pub max_poll_bytes: u64,
    pub queue_capacity: usize,
    pub poll_wall_time_ms: u64,
}
impl Default for MonitorOptions {
    fn default() -> Self {
        Self {
            walk: WalkOptions { max_files: 10_000, max_entries: 20_000, max_directories: 10_000, ..Default::default() },
            hash: HashOptions { sha256: true, blake3: false, max_source_bytes: 32 * 1024 * 1024, ..Default::default() },
            max_poll_bytes: 64 * 1024 * 1024,
            queue_capacity: 256,
            poll_wall_time_ms: 10_000,
        }
    }
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DirectoryChange {
    pub relative: NativePath,
    pub evidence_path: String,
    pub kind: String,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ChangeBatch {
    pub changes: Vec<DirectoryChange>,
    pub full_rescan_required: bool,
    pub coverage_complete: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct Snapshot {
    relative: NativePath,
    stamp: FileStamp,
    digests: Option<Vec<ContentDigest>>,
}
pub struct DirectoryMonitor {
    root: Arc<ScanRoot>,
    source: PathBuf,
    options: MonitorOptions,
    previous: BTreeMap<String, Snapshot>,
    pending: BTreeMap<String, DirectoryChange>,
    paused: bool,
    full_rescan_required: bool,
    coverage_complete: bool,
}
impl DirectoryMonitor {
    pub fn new(source: PathBuf, options: MonitorOptions, control: &ScanControl) -> io::Result<Self> {
        if options.walk.max_files > 10_000 || options.walk.max_entries > 20_000
            || options.walk.max_directories > 10_000 || options.walk.symlinks != SymlinkPolicy::Skip
            || !(1..=4096).contains(&options.queue_capacity)
            || !(1..=256 * 1024 * 1024).contains(&options.max_poll_bytes)
            || !(1..=60_000).contains(&options.poll_wall_time_ms)
        {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid directory monitor bounds"));
        }
        options.hash.validate()?;
        let root = ScanRoot::open(&source, SymlinkPolicy::Skip)?;
        if root.is_single_file() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "monitoring requires a directory"));
        }
        let source = root.original.to_path()?;
        let mut monitor = Self {
            root, source, options, previous: BTreeMap::new(), pending: BTreeMap::new(),
            paused: false, full_rescan_required: false, coverage_complete: false,
        };
        let (snapshot, complete) = monitor.capture(control)?;
        monitor.previous = snapshot;
        monitor.coverage_complete = complete;
        monitor.full_rescan_required = !complete;
        Ok(monitor)
    }

    fn capture(&self, control: &ScanControl) -> io::Result<(BTreeMap<String, Snapshot>, bool)> {
        let root = ScanRoot::open(&self.source, SymlinkPolicy::Skip)?;
        if root.stamp.device != self.root.stamp.device || root.stamp.file_id != self.root.stamp.file_id {
            return Err(io::Error::other("monitored root identity changed; select the directory again"));
        }
        let context = control.context(self.options.poll_wall_time_ms);
        let mut remaining = self.options.max_poll_bytes;
        let mut files = BTreeMap::new();
        let mut complete = true;
        for entry in root.walk(self.options.walk.clone())? {
            if context.checkpoint().is_err() { complete = false; break; }
            match entry {
                WalkEntry::File(mut file) => {
                    let digests = if file.stamp.size_bytes <= remaining
                        && file.stamp.size_bytes <= self.options.hash.max_source_bytes {
                        remaining -= file.stamp.size_bytes;
                        match hash_evidence(&mut file, &self.options.hash, || context.checkpoint()) {
                            Ok(hashes) if hashes.stable => Some(hashes.digests),
                            _ => { complete = false; None }
                        }
                    } else { complete = false; None };
                    files.insert(file.evidence_path.to_string(), Snapshot {
                        relative: file.relative, stamp: file.stamp, digests,
                    });
                }
                WalkEntry::Notice { .. } => complete = false,
            }
        }
        control.checkpoint()?;
        Ok((files, complete))
    }

    fn enqueue(&mut self, path: &str, snapshot: &Snapshot, kind: &str) {
        if !self.pending.contains_key(path) && self.pending.len() >= self.options.queue_capacity {
            self.pending.clear();
            self.full_rescan_required = true;
            return;
        }
        self.pending.insert(path.into(), DirectoryChange {
            relative: snapshot.relative.clone(), evidence_path: path.into(), kind: kind.into(),
        });
    }

    pub fn poll(&mut self, control: &ScanControl) -> io::Result<()> {
        if self.paused { return Ok(()); }
        let (mut current, complete) = self.capture(control)?;
        let mut events = Vec::new();
        for (path, snapshot) in &current {
            let kind = match self.previous.get(path) {
                None => Some("added"),
                Some(old) if old.stamp.device != snapshot.stamp.device || old.stamp.file_id != snapshot.stamp.file_id => Some("replaced"),
                Some(old) if old != snapshot => Some("changed"),
                Some(_) => None,
            };
            if let Some(kind) = kind { events.push((path.clone(), snapshot.clone(), kind)); }
        }
        if complete {
            for (path, snapshot) in &self.previous {
                if !current.contains_key(path) { events.push((path.clone(), snapshot.clone(), "removed")); }
            }
        } else {
            for (path, snapshot) in &self.previous {
                current.entry(path.clone()).or_insert_with(|| snapshot.clone());
            }
            if self.coverage_complete { self.full_rescan_required = true; }
        }
        for (path, snapshot, kind) in events { self.enqueue(&path, &snapshot, kind); }
        self.previous = current;
        self.coverage_complete = complete;
        Ok(())
    }

    pub fn pause(&mut self) {
        self.paused = true;
        self.pending.clear();
    }

    pub fn resume(&mut self) {
        self.paused = false;
        self.request_rescan();
    }

    pub fn request_rescan(&mut self) {
        self.full_rescan_required = true;
    }

    pub fn drain(&mut self) -> ChangeBatch {
        ChangeBatch {
            changes: std::mem::take(&mut self.pending).into_values().collect(),
            full_rescan_required: std::mem::take(&mut self.full_rescan_required),
            coverage_complete: self.coverage_complete,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_changes_deduplicate_pause_resume_and_overflow() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("a");
        std::fs::write(&source, b"AAAA").unwrap();
        let control = ScanControl::new();
        let options = MonitorOptions { queue_capacity: 1, ..Default::default() };
        let mut monitor = DirectoryMonitor::new(temp.path().into(), options, &control).unwrap();
        assert!(monitor.drain().changes.is_empty());
        let modified = std::fs::metadata(&source).unwrap().modified().unwrap();
        std::fs::write(&source, b"BBBB").unwrap();
        std::fs::OpenOptions::new().write(true).open(&source).unwrap().set_times(
            std::fs::FileTimes::new().set_modified(modified)).unwrap();
        monitor.poll(&control).unwrap();
        monitor.poll(&control).unwrap();
        assert_eq!(monitor.drain().changes.len(), 1);
        monitor.pause();
        std::fs::write(&source, b"CCCC").unwrap();
        monitor.poll(&control).unwrap();
        assert!(monitor.drain().changes.is_empty());
        monitor.resume();monitor.poll(&control).unwrap();
        assert!(monitor.drain().full_rescan_required);
        std::fs::rename(&source, temp.path().join("b")).unwrap();
        monitor.poll(&control).unwrap();
        assert!(monitor.drain().full_rescan_required);
        std::fs::remove_file(temp.path().join("b")).unwrap();
        monitor.poll(&control).unwrap();
        assert_eq!(monitor.drain().changes[0].kind, "removed");
        control.cancel();
        assert!(monitor.poll(&control).is_err());
    }
    #[test]
    fn incomplete_budget_does_not_claim_removal() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("large"), b"1234").unwrap();
        let options = MonitorOptions { max_poll_bytes: 1, ..Default::default() };
        let mut monitor = DirectoryMonitor::new(temp.path().into(), options, &ScanControl::new()).unwrap();
        let batch = monitor.drain();
        assert!(!batch.coverage_complete);
        assert!(batch.full_rescan_required);
    }
}
