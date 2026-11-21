//! A full-hash inventory for incremental comparisons. Metadata cache reuse still
//! independently verifies engine, options, filename and content digests.
use super::{FileStamp, NativePath, ScanOptions, ScanRoot, ScannedFile};
use crate::schema::{ContentDigest, DigestAlgorithm};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct InventoryEntry {
    pub relative: NativePath,
    pub stamp: FileStamp,
    pub digests: Vec<ContentDigest>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ScanInventory {
    pub version: u32,
    pub root: NativePath,
    pub root_device: u64,
    pub root_file_id: u64,
    pub engine: String,
    pub options_sha256: String,
    pub complete: bool,
    pub files: BTreeMap<String, InventoryEntry>,
    pub cache_hits: u64,
    bytes: usize,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct IncrementalSummary {
    pub baseline: String,
    pub added: u64,
    pub changed: u64,
    pub replaced: u64,
    pub unchanged: u64,
    pub removed: u64,
    pub unverified: u64,
    pub cache_hits: u64,
    pub baseline_compatible: bool,
    pub comparison_complete: bool,
    pub changes: Vec<(String, String)>,
    pub changes_truncated: bool,
}

impl ScanInventory {
    pub fn new(root: &ScanRoot, options: &ScanOptions, engine: &str) -> io::Result<Self> {
        let bytes = serde_json::to_vec(options).map_err(io::Error::other)?;
        let digest = Sha256::digest(bytes);
        Ok(Self {
            version: 1,
            root: root.original.clone(),
            root_device: root.stamp.device,
            root_file_id: root.stamp.file_id,
            engine: engine.into(),
            options_sha256: digest.iter().map(|b| format!("{b:02x}")).collect(),
            complete: true,
            files: BTreeMap::new(),
            cache_hits: 0,
            bytes: 0,
        })
    }

    pub fn record(&mut self, file: &ScannedFile) -> io::Result<()> {
        self.cache_hits += u64::from(file.cache_hit);
        let Some(hashes) = file.hashes.as_ref().filter(|h| h.stable && !file.unstable) else {
            self.complete = false;
            return Ok(());
        };
        if !hashes
            .digests
            .iter()
            .any(|d| d.algorithm == DigestAlgorithm::Sha256 && d.bytes.len() == 32)
        {
            self.complete = false;
            return Ok(());
        }
        let entry = InventoryEntry {
            relative: file.task.relative.clone(),
            stamp: hashes.after.clone(),
            digests: hashes.digests.clone(),
        };
        let key = file.task.evidence_path.to_string();
        let bytes = serde_json::to_vec(&entry).map_err(io::Error::other)?.len() + key.len();
        if self.files.len() >= 10_000 || self.bytes.saturating_add(bytes) > 4 * 1024 * 1024 {
            self.complete = false;
            return Ok(());
        }
        self.bytes += bytes;
        if self.files.insert(key, entry).is_some() {
            self.complete = false;
        }
        Ok(())
    }

    pub fn same_root(&self, other: &Self) -> bool {
        self.version == 1
            && other.version == 1
            && self.root == other.root
            && self.root_device == other.root_device
            && self.root_file_id == other.root_file_id
    }

    pub fn compare(&self, current: &Self, baseline: &str) -> io::Result<IncrementalSummary> {
        if !self.same_root(current) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "incremental baseline belongs to another root or root identity",
            ));
        }
        let compatible =
            self.engine == current.engine && self.options_sha256 == current.options_sha256;
        let complete = self.complete && current.complete && compatible;
        let mut out = IncrementalSummary {
            baseline: baseline.into(),
            cache_hits: current.cache_hits,
            baseline_compatible: compatible,
            comparison_complete: complete,
            ..Default::default()
        };
        for (path, entry) in &current.files {
            let kind = match self.files.get(path) {
                None if self.complete && compatible => {
                    out.added += 1;
                    "added"
                }
                None => {
                    out.unverified += 1;
                    "unverified"
                }
                Some(old)
                    if old.stamp.device != entry.stamp.device
                        || old.stamp.file_id != entry.stamp.file_id =>
                {
                    out.replaced += 1;
                    "replaced"
                }
                Some(old) if old.digests != entry.digests || old.stamp != entry.stamp => {
                    out.changed += 1;
                    "changed"
                }
                Some(_) => {
                    out.unchanged += 1;
                    "unchanged"
                }
            };
            if kind != "unchanged" {
                if out.changes.len() < 256 {
                    out.changes.push((path.clone(), kind.into()));
                } else {
                    out.changes_truncated = true;
                }
            }
        }
        if complete {
            for path in self
                .files
                .keys()
                .filter(|p| !current.files.contains_key(*p))
            {
                out.removed += 1;
                if out.changes.len() < 256 {
                    out.changes.push((path.clone(), "removed".into()));
                } else {
                    out.changes_truncated = true;
                }
            }
        } else {
            out.unverified += self
                .files
                .keys()
                .filter(|p| !current.files.contains_key(*p))
                .count() as u64;
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::DigestAlgorithm;
    fn inventory() -> ScanInventory {
        ScanInventory {
            version: 1,
            root: NativePath::Unix(b"/test".to_vec()),
            root_device: 1,
            root_file_id: 2,
            engine: "engine".into(),
            options_sha256: "options".into(),
            complete: true,
            files: BTreeMap::new(),
            cache_hits: 0,
            bytes: 0,
        }
    }
    fn entry(byte: u8) -> InventoryEntry {
        InventoryEntry {
            relative: NativePath::Unix(b"image.png".to_vec()),
            stamp: FileStamp {
                device: 1,
                file_id: 3,
                size_bytes: 1,
                modified_ns: Some(7),
                created_ns: None,
                changed_ns: None,
            },
            digests: vec![ContentDigest {
                algorithm: DigestAlgorithm::Sha256,
                bytes: vec![byte; 32],
            }],
        }
    }
    #[test]
    fn full_hash_detects_changes_with_equal_size_and_restored_mtime() {
        let mut old = inventory();
        old.files.insert("image.png".into(), entry(1));
        let mut current = old.clone();
        current.files.insert("image.png".into(), entry(2));
        let delta = old.compare(&current, "baseline").unwrap();
        assert_eq!(delta.changed, 1);
        assert_eq!(delta.unchanged, 0);
        current.files.get_mut("image.png").unwrap().stamp.file_id = 4;
        assert_eq!(old.compare(&current, "baseline").unwrap().replaced, 1);
    }
    #[test]
    fn incomplete_or_incompatible_inventory_never_claims_removal() {
        let mut old = inventory();
        old.files.insert("image.png".into(), entry(1));
        let mut current = inventory();
        assert_eq!(old.compare(&current, "baseline").unwrap().removed, 1);
        current.complete = false;
        assert_eq!(old.compare(&current, "baseline").unwrap().removed, 0);
        assert_eq!(old.compare(&current, "baseline").unwrap().unverified, 1);
        current.complete = true;
        current.engine = "new engine".into();
        assert!(
            !old.compare(&current, "baseline")
                .unwrap()
                .baseline_compatible
        );
        let mut partial = old.clone();
        partial.complete = false;
        let mut added = old.clone();
        added.files.insert("new.png".into(), entry(2));
        let delta = partial.compare(&added, "baseline").unwrap();
        assert_eq!(delta.added, 0);
        assert_eq!(delta.unverified, 1);
        current.root_file_id = 99;
        assert!(old.compare(&current, "baseline").is_err());
    }
}
