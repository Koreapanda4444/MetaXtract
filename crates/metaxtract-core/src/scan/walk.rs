use crate::schema::RelativePath;
use cap_fs_ext::{FollowSymlinks, MetadataExt, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions, ReadDir},
};
use std::{
    collections::HashSet,
    ffi::OsStr,
    fs::File,
    io,
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::UNIX_EPOCH,
};

/// Original OS path units are retained separately from portable evidence names.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Eq, Hash, PartialEq)]
pub enum NativePath {
    Unix(Vec<u8>),
    Windows(Vec<u16>),
}
impl NativePath {
    pub fn from_path(path: &Path) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            Self::Unix(path.as_os_str().as_bytes().to_vec())
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            Self::Windows(path.as_os_str().encode_wide().collect())
        }
    }
    pub fn to_path(&self) -> io::Result<PathBuf> {
        match self {
            #[cfg(unix)]
            Self::Unix(bytes) => {
                use std::os::unix::ffi::OsStringExt;
                Ok(std::ffi::OsString::from_vec(bytes.clone()).into())
            }
            #[cfg(windows)]
            Self::Windows(units) => {
                use std::os::windows::ffi::OsStringExt;
                Ok(std::ffi::OsString::from_wide(units).into())
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "path belongs to a different operating system",
            )),
        }
    }
    pub fn name_bytes(&self) -> io::Result<Vec<u8>> {
        let path = self.to_path()?;
        let name = path
            .file_name()
            .ok_or_else(|| invalid("missing filename"))?;
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            Ok(name.as_bytes().to_vec())
        }
        #[cfg(windows)]
        {
            Ok(name.to_string_lossy().as_bytes().to_vec())
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct FileStamp {
    pub device: u64,
    pub file_id: u64,
    pub size_bytes: u64,
    pub modified_ns: Option<i128>,
    pub created_ns: Option<i128>,
    pub changed_ns: Option<i128>,
}
impl FileStamp {
    fn metadata(meta: &cap_std::fs::Metadata) -> Self {
        fn ns(value: io::Result<cap_std::time::SystemTime>) -> Option<i128> {
            value
                .ok()
                .map(|v| match v.into_std().duration_since(UNIX_EPOCH) {
                    Ok(d) => d.as_nanos() as i128,
                    Err(e) => -(e.duration().as_nanos() as i128),
                })
        }
        #[cfg(unix)]
        let changed_ns = {
            use cap_std::fs::MetadataExt as _;
            Some(i128::from(meta.ctime()) * 1_000_000_000 + i128::from(meta.ctime_nsec()))
        };
        #[cfg(windows)]
        let changed_ns = None;
        Self {
            device: MetadataExt::dev(meta),
            file_id: MetadataExt::ino(meta),
            size_bytes: meta.len(),
            modified_ns: ns(meta.modified()),
            created_ns: ns(meta.created()),
            changed_ns,
        }
    }
    pub fn read(file: &File) -> io::Result<Self> {
        Ok(Self::metadata(
            &cap_std::fs::File::from_std(file.try_clone()?).metadata()?,
        ))
    }
    pub fn identity(&self) -> crate::schema::FileIdentity {
        crate::schema::FileIdentity {
            size_bytes: self.size_bytes,
            modified_ns: self.modified_ns,
            created_ns: self.created_ns,
            stable_id: Some(format!("{}:{}", self.device, self.file_id)),
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, Eq, PartialEq)]
pub enum SymlinkPolicy {
    Skip,
    FollowWithinRoot,
}
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct WalkOptions {
    pub symlinks: SymlinkPolicy,
    pub include_hidden: bool,
    pub max_depth: u32,
    pub max_files: u64,
    pub max_entries: u64,
    pub max_directories: u64,
    pub excluded: Vec<NativePath>,
}
impl Default for WalkOptions {
    fn default() -> Self {
        Self {
            symlinks: SymlinkPolicy::Skip,
            include_hidden: false,
            max_depth: 64,
            max_files: 1_000_000,
            max_entries: 2_000_000,
            max_directories: 100_000,
            excluded: Vec::new(),
        }
    }
}

pub struct ScanRoot {
    dir: Dir,
    base: PathBuf,
    pub original: NativePath,
    pub stamp: FileStamp,
    single_file: Option<PathBuf>,
}
pub struct OpenedEvidence {
    pub file: File,
    pub relative: NativePath,
    pub evidence_path: RelativePath,
    pub stamp: FileStamp,
}
pub enum WalkEntry {
    File(OpenedEvidence),
    Notice {
        path: Option<NativePath>,
        code: String,
        message: String,
    },
}
impl ScanRoot {
    pub fn open(path: impl AsRef<Path>, policy: SymlinkPolicy) -> io::Result<Arc<Self>> {
        let path = path.as_ref();
        if policy == SymlinkPolicy::Skip && std::fs::symlink_metadata(path)?.is_symlink() {
            return Err(invalid("selected root is a symbolic link"));
        }
        let absolute = path.canonicalize()?;
        let metadata = std::fs::metadata(&absolute)?;
        let (base, single_file) = if metadata.is_dir() {
            (absolute, None)
        } else if metadata.is_file() {
            (
                absolute
                    .parent()
                    .ok_or_else(|| invalid("root has no parent"))?
                    .to_path_buf(),
                Some(PathBuf::from(
                    absolute
                        .file_name()
                        .ok_or_else(|| invalid("root has no name"))?,
                )),
            )
        } else {
            return Err(invalid("root is not a regular file or directory"));
        };
        let dir = Dir::open_ambient_dir(&base, ambient_authority())?;
        let stamp = if let Some(file) = &single_file {
            Self::open_in(&dir, file, policy)?.1
        } else {
            FileStamp::metadata(&dir.dir_metadata()?)
        };
        Ok(Arc::new(Self {
            dir,
            base,
            original: NativePath::from_path(&path.canonicalize()?),
            stamp,
            single_file,
        }))
    }
    fn open_in(dir: &Dir, path: &Path, policy: SymlinkPolicy) -> io::Result<(File, FileStamp)> {
        let mut options = OpenOptions::new();
        options.read(true);
        if policy == SymlinkPolicy::Skip {
            options.follow(FollowSymlinks::No);
        }
        options.nonblock(true); // Never wait on a raced FIFO.
        let file = dir.open_with(path, &options)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(invalid("evidence is not a regular file"));
        }
        Ok((file.into_std(), FileStamp::metadata(&metadata)))
    }
    pub fn open_file(
        &self,
        relative: &NativePath,
        policy: SymlinkPolicy,
    ) -> io::Result<OpenedEvidence> {
        let path = relative.to_path()?;
        if path.as_os_str().is_empty()
            || !path.components().all(|c| matches!(c, Component::Normal(_)))
        {
            return Err(invalid("unsafe relative path"));
        }
        if self.single_file.as_ref().is_some_and(|f| f != &path) {
            return Err(invalid("path is outside the selected file"));
        }
        let (file, stamp) = Self::open_in(&self.dir, &path, policy)?;
        Ok(OpenedEvidence {
            file,
            relative: relative.clone(),
            evidence_path: evidence_path(&path)?,
            stamp,
        })
    }
    pub fn relative_to_root(&self, absolute: &Path) -> Option<NativePath> {
        absolute
            .strip_prefix(&self.base)
            .ok()
            .filter(|p| !p.as_os_str().is_empty())
            .map(NativePath::from_path)
    }
    pub fn walk(self: &Arc<Self>, options: WalkOptions) -> io::Result<Walker> {
        if options.max_files == 0 || options.max_entries == 0 || options.max_directories == 0 {
            return Err(invalid("walk limits must be positive"));
        }
        let mut walker = Walker {
            root: Arc::clone(self),
            options,
            stack: Vec::new(),
            selected: self.single_file.clone(),
            visited: HashSet::new(),
            entries: 0,
            files: 0,
            directories: 1,
            done: false,
        };
        walker
            .visited
            .insert((self.stamp.device, self.stamp.file_id));
        if self.single_file.is_none() {
            walker
                .stack
                .push((PathBuf::new(), 0, self.dir.open_dir(".")?.entries()?));
        }
        Ok(walker)
    }
}
pub struct Walker {
    root: Arc<ScanRoot>,
    options: WalkOptions,
    stack: Vec<(PathBuf, u32, ReadDir)>,
    selected: Option<PathBuf>,
    visited: HashSet<(u64, u64)>,
    entries: u64,
    files: u64,
    directories: u64,
    done: bool,
}
impl Walker {
    fn notice(path: Option<&Path>, code: &str, message: impl ToString) -> WalkEntry {
        WalkEntry::Notice {
            path: path.map(NativePath::from_path),
            code: code.into(),
            message: message.to_string(),
        }
    }
    fn excluded(&self, path: &Path) -> bool {
        self.options
            .excluded
            .iter()
            .any(|p| p.to_path().is_ok_and(|p| path.starts_with(p)))
    }
    fn file(&mut self, path: &Path) -> WalkEntry {
        match self
            .root
            .open_file(&NativePath::from_path(path), self.options.symlinks)
        {
            Ok(file) => {
                self.files += 1;
                WalkEntry::File(file)
            }
            Err(e) => Self::notice(Some(path), "walk.open", e),
        }
    }
}
impl Iterator for Walker {
    type Item = WalkEntry;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        if let Some(path) = self.selected.take() {
            self.done = true;
            return if self.excluded(&path) {
                None
            } else {
                Some(self.file(&path))
            };
        }
        loop {
            if self.files >= self.options.max_files || self.entries >= self.options.max_entries {
                self.done = true;
                return Some(Self::notice(None, "walk.limit", "discovery limit reached"));
            }
            let (parent, depth, entry) = {
                let (path, depth, entries) = self.stack.last_mut()?;
                (path.clone(), *depth, entries.next())
            };
            let Some(entry) = entry else {
                self.stack.pop();
                continue;
            };
            self.entries += 1;
            let entry = match entry {
                Ok(v) => v,
                Err(e) => return Some(Self::notice(Some(&parent), "walk.read", e)),
            };
            let name = entry.file_name();
            let path = parent.join(&name);
            if self.excluded(&path)
                || (!self.options.include_hidden && name.to_string_lossy().starts_with('.'))
            {
                continue;
            }
            let ty = match entry.file_type() {
                Ok(v) => v,
                Err(e) => return Some(Self::notice(Some(&path), "walk.type", e)),
            };
            if ty.is_symlink() && self.options.symlinks == SymlinkPolicy::Skip {
                return Some(Self::notice(
                    Some(&path),
                    "walk.symlink",
                    "symbolic link skipped",
                ));
            }
            let meta = match self.root.dir.metadata(&path) {
                Ok(v) => v,
                Err(e) => return Some(Self::notice(Some(&path), "walk.metadata", e)),
            };
            if meta.is_dir() {
                if depth >= self.options.max_depth
                    || self.directories >= self.options.max_directories
                {
                    return Some(Self::notice(
                        Some(&path),
                        "walk.directory_limit",
                        "directory limit reached",
                    ));
                }
                let stamp = FileStamp::metadata(&meta);
                if !self.visited.insert((stamp.device, stamp.file_id)) {
                    return Some(Self::notice(
                        Some(&path),
                        "walk.cycle",
                        "directory alias or cycle skipped",
                    ));
                }
                match self.root.dir.open_dir(&path).and_then(|d| d.entries()) {
                    Ok(entries) => {
                        self.directories += 1;
                        self.stack.push((path, depth + 1, entries));
                    }
                    Err(e) => return Some(Self::notice(Some(&path), "walk.directory", e)),
                }
            } else if meta.is_file() {
                return Some(self.file(&path));
            } else {
                return Some(Self::notice(
                    Some(&path),
                    "walk.special",
                    "non-regular entry skipped",
                ));
            }
        }
    }
}
fn evidence_path(path: &Path) -> io::Result<RelativePath> {
    let pieces: Vec<String> = path
        .components()
        .map(|c| match c {
            Component::Normal(s) => encode_component(s),
            _ => String::new(),
        })
        .collect();
    RelativePath::parse(pieces.join("/")).map_err(invalid)
}
fn encode_component(name: &OsStr) -> String {
    if let Some(s) = name.to_str() {
        if !s.starts_with("~os-") && !s.contains('\\') && RelativePath::parse(s).is_ok() {
            return s.into();
        }
    }
    let raw = NativePath::from_path(Path::new(name));
    match raw {
        NativePath::Unix(bytes) => format!("~os-u{}", hex(&bytes)),
        NativePath::Windows(units) => format!(
            "~os-w{}",
            hex(&units
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>())
        ),
    }
}
pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut text = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut text, "{b:02x}");
    }
    text
}
pub(crate) fn invalid(message: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.to_string())
}
