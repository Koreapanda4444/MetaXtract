#![forbid(unsafe_code)]

mod codec;
mod history;
mod query;
pub use history::{
    Annotation, CaseNote, FilterQuery, NoteHeader, SavedFilter, ScanCursor, ScanHistoryPage,
    ScanInfo,
};
mod records;
pub use query::{Page, PageCursor, RecordHeader, RecordOrder, RecordQuery};
mod schema;
pub use records::{MetadataCounts, ScanStatus};

use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const SCHEMA_REVISION: u32 = 4;
pub const APPLICATION_ID: i32 = 0x4d584353;
pub(crate) type Result<T> = io::Result<T>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessMode {
    ReadOnly,
    ReadWrite,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct CaseInfo {
    pub case_id: String,
    pub title: String,
    pub notes: String,
    pub created_ms: i64,
    pub updated_ms: i64,
    pub revision: u64,
}

pub struct CaseWorkspace {
    pub(crate) conn: Mutex<Connection>,
    path: PathBuf,
    access: AccessMode,
}

impl CaseWorkspace {
    pub fn create(path: impl AsRef<Path>, title: &str) -> Result<Self> {
        text(title, 4096, false)?;
        let path = path.as_ref();
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let temp = tempfile::NamedTempFile::new_in(parent)?;
        {
            let mut conn = connection(temp.path(), AccessMode::ReadWrite)?;
            durable(&conn)?;
            conn.pragma_update(None, "application_id", APPLICATION_ID)
                .map_err(db)?;
            schema::migrate(&mut conn)?;
            let now = now_ms()?;
            let id = hex(&Sha256::digest(format!(
                "{}:{now}:{}",
                temp.path().display(),
                std::process::id()
            )));
            conn.execute("INSERT INTO case_info(singleton,case_id,title,notes,created_ms,updated_ms,revision) VALUES(1,?1,?2,'',?3,?3,0)",params![id,title.trim(),now]).map_err(db)?;
        }
        temp.as_file().sync_all()?;
        temp.persist_noclobber(path).map_err(|e| e.error)?;
        #[cfg(unix)]
        std::fs::File::open(parent)?.sync_all()?;
        Self::open(path, AccessMode::ReadWrite)
    }

    pub fn open(path: impl AsRef<Path>, access: AccessMode) -> Result<Self> {
        let path = path.as_ref();
        let metadata = std::fs::symlink_metadata(path)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(invalid("case workspace must be a regular file"));
        }
        let mut conn = connection(path, access)?;
        let app: i32 = conn
            .pragma_query_value(None, "application_id", |r| r.get(0))
            .map_err(db)?;
        if app != APPLICATION_ID {
            return Err(invalid(
                "not a Rust MetaXtract case; use the legacy importer for Python cases",
            ));
        }
        let version: u32 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(db)?;
        if version == 0 || version > SCHEMA_REVISION {
            return Err(invalid("unsupported case schema"));
        }
        if access == AccessMode::ReadWrite {
            durable(&conn)?;
        }
        if version < SCHEMA_REVISION {
            if access == AccessMode::ReadOnly {
                return Err(invalid("case needs a writable schema migration"));
            }
            schema::migrate(&mut conn)?;
        }
        let result = Self {
            conn: Mutex::new(conn),
            path: path.canonicalize()?,
            access,
        };
        result.info()?;
        Ok(result)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn access_mode(&self) -> AccessMode {
        self.access
    }

    pub fn info(&self) -> Result<CaseInfo> {
        self.lock()?.query_row("SELECT case_id,title,notes,created_ms,updated_ms,revision FROM case_info WHERE singleton=1",[],|r|Ok(CaseInfo{case_id:r.get(0)?,title:r.get(1)?,notes:r.get(2)?,created_ms:r.get(3)?,updated_ms:r.get(4)?,revision:r.get(5)?})).map_err(db)
    }

    pub fn set_title(&self, title: &str) -> Result<()> {
        text(title, 4096, false)?;
        self.write(|tx| {
            tx.execute(
                "UPDATE case_info SET title=?1 WHERE singleton=1",
                [title.trim()],
            )
            .map_err(db)?;
            Ok(())
        })
    }

    pub(crate) fn lock(&self) -> Result<MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| io::Error::other("case lock poisoned"))
    }

    pub(crate) fn write<T>(&self, action: impl FnOnce(&Transaction<'_>) -> Result<T>) -> Result<T> {
        if self.access == AccessMode::ReadOnly {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "case is read-only",
            ));
        }
        let mut conn = self.lock()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        let result = action(&tx)?;
        tx.execute(
            "UPDATE case_info SET revision=revision+1,updated_ms=?1 WHERE singleton=1",
            [now_ms()?],
        )
        .map_err(db)?;
        tx.commit().map_err(db)?;
        Ok(result)
    }
}

fn connection(path: &Path, access: AccessMode) -> Result<Connection> {
    let flags = match access {
        AccessMode::ReadOnly => OpenFlags::SQLITE_OPEN_READ_ONLY,
        AccessMode::ReadWrite => OpenFlags::SQLITE_OPEN_READ_WRITE,
    } | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(path, flags).map_err(db)?;
    conn.busy_timeout(Duration::from_secs(15)).map_err(db)?;
    conn.execute_batch(
        "PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF; PRAGMA cache_size=-16384;",
    )
    .map_err(db)?;
    Ok(conn)
}

fn durable(conn: &Connection) -> Result<()> {
    conn.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;")
        .map_err(db)
}

pub(crate) fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
pub(crate) fn db(error: rusqlite::Error) -> io::Error {
    io::Error::other(error)
}
pub(crate) fn text(value: &str, max: usize, empty: bool) -> Result<()> {
    if value.len() > max || value.contains('\0') || (!empty && value.trim().is_empty()) {
        return Err(invalid(
            "text is empty, contains NUL, or exceeds its byte limit",
        ));
    }
    Ok(())
}
pub(crate) fn now_ms() -> Result<i64> {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_millis(),
    )
    .map_err(io::Error::other)
}
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
