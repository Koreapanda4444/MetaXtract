use std::error::Error;
use std::fmt::{self, Display, Formatter};

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RelativePath(String);

impl RelativePath {
    pub fn parse(value: impl Into<String>) -> Result<Self, PathError> {
        let value = value.into();
        if value.is_empty() {
            return Err(PathError::Empty);
        }
        if value.chars().any(char::is_control) {
            return Err(PathError::ControlCharacter);
        }

        let normalized = value.replace('\\', "/");
        let bytes = normalized.as_bytes();
        if normalized.starts_with('/')
            || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
        {
            return Err(PathError::Absolute);
        }

        for segment in normalized.split('/') {
            match segment {
                "" => return Err(PathError::EmptySegment),
                "." | ".." => return Err(PathError::Traversal),
                _ => {}
            }
        }
        Ok(Self(normalized))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn file_name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or(self.0.as_str())
    }
}

impl Display for RelativePath {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathError {
    Empty,
    Absolute,
    ControlCharacter,
    EmptySegment,
    Traversal,
}

impl Display for PathError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Empty => "path is empty",
            Self::Absolute => "absolute paths are not allowed",
            Self::ControlCharacter => "path contains a control character",
            Self::EmptySegment => "path contains an empty segment",
            Self::Traversal => "path contains a traversal segment",
        };
        formatter.write_str(message)
    }
}

impl Error for PathError {}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EvidenceKind {
    ArchiveMember,
    Attachment,
    EmbeddedObject,
    AlternateStream,
    Resource,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct EvidenceSegment {
    pub kind: EvidenceKind,
    pub path: RelativePath,
    pub ordinal: u32,
}

impl EvidenceSegment {
    pub fn new(kind: EvidenceKind, path: RelativePath, ordinal: u32) -> Self {
        Self {
            kind,
            path,
            ordinal,
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct EvidencePath {
    root: RelativePath,
    nested: Vec<EvidenceSegment>,
}

impl EvidencePath {
    pub fn root(path: RelativePath) -> Self {
        Self {
            root: path,
            nested: Vec::new(),
        }
    }

    pub fn push(&mut self, segment: EvidenceSegment) {
        self.nested.push(segment);
    }

    pub fn with_segment(mut self, segment: EvidenceSegment) -> Self {
        self.push(segment);
        self
    }

    pub fn root_path(&self) -> &RelativePath {
        &self.root
    }

    pub fn nested(&self) -> &[EvidenceSegment] {
        &self.nested
    }

    pub fn depth(&self) -> usize {
        self.nested.len()
    }
}

impl Display for EvidencePath {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.root.as_str())?;
        for segment in &self.nested {
            write!(formatter, "!/{path}", path = segment.path)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ByteRange {
    offset: u64,
    length: u64,
}

impl ByteRange {
    pub fn new(offset: u64, length: u64) -> Option<Self> {
        offset.checked_add(length)?;
        Some(Self { offset, length })
    }

    pub fn end(self) -> u64 {
        self.offset + self.length
    }

    pub fn offset(self) -> u64 {
        self.offset
    }

    pub fn length(self) -> u64 {
        self.length
    }

    pub fn contains(self, other: Self) -> bool {
        self.offset <= other.offset && self.end() >= other.end()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileIdentity {
    pub size_bytes: u64,
    pub modified_ns: Option<i128>,
    pub created_ns: Option<i128>,
    pub stable_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SourceKind {
    File,
    ArchiveMember,
    Attachment,
    EmbeddedObject,
    Stream,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceDescriptor {
    pub path: EvidencePath,
    pub kind: SourceKind,
    pub identity: FileIdentity,
    pub declared_media_type: Option<String>,
    pub detected_media_type: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum StreamKind {
    Metadata,
    Image,
    Audio,
    Video,
    Subtitle,
    Document,
    Data,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamDescriptor {
    pub id: String,
    pub kind: StreamKind,
    pub label: Option<String>,
    pub media_type: Option<String>,
    pub size_bytes: Option<u64>,
    pub source_range: Option<ByteRange>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContainerDescriptor {
    pub format: String,
    pub encrypted: Option<bool>,
    pub declared_entry_count: Option<u64>,
    pub streams: Vec<StreamDescriptor>,
}
