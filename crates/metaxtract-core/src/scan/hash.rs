use super::{FileStamp, OpenedEvidence, walk::invalid};
use crate::schema::{ContentDigest, DigestAlgorithm};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
};

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct HashOptions {
    pub sha256: bool,
    pub blake3: bool,
    pub buffer_bytes: usize,
    pub max_source_bytes: u64,
}
impl Default for HashOptions {
    fn default() -> Self {
        Self {
            sha256: true,
            blake3: true,
            buffer_bytes: 1024 * 1024,
            max_source_bytes: 1024 * 1024 * 1024,
        }
    }
}
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct EvidenceHashes {
    pub digests: Vec<ContentDigest>,
    pub bytes_read: u64,
    pub prefix: Vec<u8>,
    pub before: FileStamp,
    pub after: FileStamp,
    pub stable: bool,
}
impl HashOptions {
    pub(crate) fn validate(&self) -> io::Result<()> {
        if (!self.sha256 && !self.blake3)
            || !(4096..=16 * 1024 * 1024).contains(&self.buffer_bytes)
            || self.max_source_bytes == 0
        {
            return Err(invalid("invalid hash policy"));
        }
        Ok(())
    }
}
pub fn hash_evidence(
    evidence: &mut OpenedEvidence,
    options: &HashOptions,
    checkpoint: impl Fn() -> io::Result<()>,
) -> io::Result<EvidenceHashes> {
    hash_capture(evidence, options, checkpoint, None, &mut Vec::new())
}
pub(crate) fn hash_capture(
    evidence: &mut OpenedEvidence,
    options: &HashOptions,
    checkpoint: impl Fn() -> io::Result<()>,
    mut snapshot: Option<&mut File>,
    buffer: &mut Vec<u8>,
) -> io::Result<EvidenceHashes> {
    options.validate()?;
    checkpoint()?;
    let before = FileStamp::read(&evidence.file)?;
    if before.size_bytes > options.max_source_bytes {
        return Err(invalid("file exceeds hash source limit"));
    }
    evidence.file.seek(SeekFrom::Start(0))?;
    if let Some(dest) = snapshot.as_mut() {
        dest.set_len(0)?;
        dest.seek(SeekFrom::Start(0))?;
    }
    buffer
        .try_reserve(options.buffer_bytes.saturating_sub(buffer.len()))
        .map_err(|e| io::Error::other(e.to_string()))?;
    buffer.resize(options.buffer_bytes, 0);
    let mut sha = options.sha256.then(Sha256::new);
    let mut blake = options.blake3.then(blake3::Hasher::new);
    let mut prefix = Vec::with_capacity(64 * 1024);
    let mut count = 0_u64;
    loop {
        checkpoint()?;
        let read = match evidence.file.read(buffer) {
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if read == 0 {
            break;
        }
        count = count
            .checked_add(read as u64)
            .ok_or_else(|| invalid("hash byte counter overflow"))?;
        if count > options.max_source_bytes {
            return Err(invalid("file grew beyond the hash limit"));
        }
        let data = &buffer[..read];
        if let Some(hash) = &mut sha {
            hash.update(data);
        }
        if let Some(hash) = &mut blake {
            hash.update(data);
        }
        if let Some(dest) = snapshot.as_mut() {
            dest.write_all(data)?;
        }
        let retained = (64 * 1024 - prefix.len()).min(read);
        prefix.extend_from_slice(&data[..retained]);
    }
    checkpoint()?;
    let after = FileStamp::read(&evidence.file)?;
    if let Some(dest) = snapshot.as_mut() {
        dest.flush()?;
        dest.seek(SeekFrom::Start(0))?;
    }
    evidence.file.seek(SeekFrom::Start(0))?;
    let mut digests = Vec::new();
    if let Some(hash) = sha {
        digests.push(ContentDigest {
            algorithm: DigestAlgorithm::Sha256,
            bytes: hash.finalize().to_vec(),
        });
    }
    if let Some(hash) = blake {
        digests.push(ContentDigest {
            algorithm: DigestAlgorithm::Blake3,
            bytes: hash.finalize().as_bytes().to_vec(),
        });
    }
    Ok(EvidenceHashes {
        stable: evidence.stamp == before && before == after && count == before.size_bytes,
        digests,
        bytes_read: count,
        prefix,
        before,
        after,
    })
}
