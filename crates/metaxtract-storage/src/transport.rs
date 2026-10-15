use crate::{Result, hex, invalid};
use metaxtract_schema::RelativePath;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};
use tempfile::NamedTempFile;
use zip::ZipArchive;

pub(crate) fn hash_file(path: &Path, max: u64) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut count = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        count += n as u64;
        if count > max {
            return Err(invalid("file exceeds hash budget"));
        }
        hash.update(&buffer[..n]);
    }
    Ok(hex(&hash.finalize()))
}
pub(crate) fn database_snapshot(
    source: &rusqlite::Connection,
    path: &Path,
    max: u64,
) -> Result<()> {
    use rusqlite::backup::{Backup, StepResult};
    use std::time::{Duration, Instant};
    let page_size: u64 = source
        .pragma_query_value(None, "page_size", |r| r.get(0))
        .map_err(crate::db)?;
    if page_size == 0 || page_size > 65536 || max < page_size {
        return Err(invalid("invalid database page size or snapshot budget"));
    }
    let mut target = rusqlite::Connection::open(path).map_err(crate::db)?;
    target
        .pragma_update(None, "page_size", page_size)
        .map_err(crate::db)?;
    target
        .pragma_update(
            None,
            "max_page_count",
            (max / page_size).min(u32::MAX as u64),
        )
        .map_err(crate::db)?;
    let backup = Backup::new(source, &mut target).map_err(crate::db)?;
    let started = Instant::now();
    loop {
        let result = backup.step(128).map_err(crate::db)?;
        if backup.progress().pagecount < 0 || backup.progress().pagecount as u64 > max / page_size {
            return Err(invalid("database snapshot exceeds byte budget"));
        }
        if result == StepResult::Done {
            break;
        }
        if started.elapsed() > Duration::from_secs(120) {
            return Err(invalid("database snapshot deadline reached"));
        }
        if matches!(result, StepResult::Busy | StepResult::Locked) {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    Ok(())
}

pub(crate) fn snapshot(path: &Path, max: u64) -> Result<(NamedTempFile, String)> {
    let mut source = File::open(path)?;
    let before = source.metadata()?;
    if !before.is_file() || before.len() > max {
        return Err(invalid(
            "input exceeds snapshot budget or is not a regular file",
        ));
    }
    let mut target = NamedTempFile::new()?;
    let mut hash = Sha256::new();
    let mut size = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = source.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        size = size
            .checked_add(n as u64)
            .ok_or_else(|| invalid("input size overflow"))?;
        if size > max {
            return Err(invalid("input exceeds snapshot budget"));
        }
        target.write_all(&buffer[..n])?;
        hash.update(&buffer[..n]);
    }
    let after = source.metadata()?;
    if size != before.len()
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err(invalid("input changed while creating its import snapshot"));
    }
    target.as_file().sync_all()?;
    Ok((target, hex(&hash.finalize())))
}
pub(crate) fn open_zip(file: File, max_members: usize) -> Result<ZipArchive<File>> {
    // Check the declared directory count before ZipArchive allocates its member map.
    let mut file = file;
    let len = file.metadata()?.len();
    let tail_len = len.min(65_557) as usize;
    file.seek(SeekFrom::End(-(tail_len as i64)))?;
    let mut tail = vec![0u8; tail_len];
    file.read_exact(&mut tail)?;
    let pos = (0..tail.len().saturating_sub(21))
        .rev()
        .find(|&i| {
            tail[i..].starts_with(b"PK\x05\x06")
                && i + 22 + u16::from_le_bytes([tail[i + 20], tail[i + 21]]) as usize == tail.len()
        })
        .ok_or_else(|| invalid("ZIP end directory is missing"))?;
    let e = &tail[pos..];
    let declared = u16::from_le_bytes([e[10], e[11]]) as u64;
    let mut count = declared;
    if e[4..8] != [0, 0, 0, 0] || u16::from_le_bytes([e[8], e[9]]) as u64 != count {
        return Err(invalid("split case archive"));
    }
    let end_offset = len - tail_len as u64 + pos as u64;
    let mut zip64 = false;
    if end_offset >= 20 {
        file.seek(SeekFrom::Start(end_offset - 20))?;
        let mut locator = [0u8; 20];
        file.read_exact(&mut locator)?;
        if locator.starts_with(b"PK\x06\x07") {
            zip64 = true;
            if locator[4..8] != [0, 0, 0, 0] || locator[16..20] != [1, 0, 0, 0] {
                return Err(invalid("split ZIP64 case archive"));
            }
            let offset = u64::from_le_bytes(
                locator[8..16]
                    .try_into()
                    .map_err(|_| invalid("invalid ZIP64 locator"))?,
            );
            if offset.checked_add(56).is_none_or(|v| v > end_offset - 20) {
                return Err(invalid("invalid ZIP64 directory offset"));
            }
            file.seek(SeekFrom::Start(offset))?;
            let mut header = [0u8; 56];
            file.read_exact(&mut header)?;
            if !header.starts_with(b"PK\x06\x06") || header[16..24] != [0; 8] {
                return Err(invalid("invalid ZIP64 directory"));
            }
            let total = u64::from_le_bytes(
                header[32..40]
                    .try_into()
                    .map_err(|_| invalid("invalid ZIP64 count"))?,
            );
            let disk = u64::from_le_bytes(
                header[24..32]
                    .try_into()
                    .map_err(|_| invalid("invalid ZIP64 count"))?,
            );
            if disk != total || (declared != 65535 && declared != total) {
                return Err(invalid("ZIP64 member counts differ"));
            }
            count = total;
        }
    }
    if (declared == 65535 && !zip64) || count > max_members as u64 {
        return Err(invalid("case archive exceeds member budget"));
    }
    file.seek(SeekFrom::Start(0))?;
    let archive = ZipArchive::new(file).map_err(std::io::Error::other)?;
    if archive.len() as u64 != count || archive.len() > max_members {
        return Err(invalid("case archive member count differs"));
    }
    Ok(archive)
}
pub(crate) fn inventory(
    archive: &mut ZipArchive<File>,
    max_member: u64,
    max_total: u64,
) -> Result<BTreeMap<String, (String, u64)>> {
    let mut out = BTreeMap::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut member = archive.by_index(index).map_err(std::io::Error::other)?;
        let name = std::str::from_utf8(member.name_raw())
            .map_err(|_| invalid("case member name is not UTF-8"))?
            .to_owned();
        let path = RelativePath::parse(name.clone()).map_err(std::io::Error::other)?;
        if path.as_str() != name
            || member.is_dir()
            || member.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000)
            || out.contains_key(&name)
            || member.size() > max_member
        {
            return Err(invalid("unsafe, duplicate or oversized case member"));
        }
        total = total
            .checked_add(member.size())
            .ok_or_else(|| invalid("case archive size overflow"))?;
        if total > max_total {
            return Err(invalid("case archive exceeds total expansion budget"));
        }
        let mut hash = Sha256::new();
        let mut size = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let n = member.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            size += n as u64;
            if size > max_member || size > member.size() {
                return Err(invalid("case member exceeds declared size"));
            }
            hash.update(&buffer[..n]);
        }
        if size != member.size() {
            return Err(invalid("case member size differs"));
        }
        out.insert(name, (hex(&hash.finalize()), size));
    }
    Ok(out)
}
pub(crate) fn member(archive: &mut ZipArchive<File>, name: &str, max: usize) -> Result<Vec<u8>> {
    let mut source = archive.by_name(name).map_err(std::io::Error::other)?;
    if source.size() > max as u64 {
        return Err(invalid("case control exceeds byte budget"));
    }
    let mut data = Vec::new();
    source
        .by_ref()
        .take(max as u64 + 1)
        .read_to_end(&mut data)?;
    if data.len() > max {
        return Err(invalid("case control exceeds byte budget"));
    }
    Ok(data)
}
