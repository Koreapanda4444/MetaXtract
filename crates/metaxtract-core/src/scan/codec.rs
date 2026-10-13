use rusqlite::{Connection, limits::Limit};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    io::{self, Write},
    path::Path,
    time::Duration,
};
pub(crate) const MAX_BLOB_BYTES: usize = 1024 * 1024 * 1024;
const APPLICATION_ID: i32 = 0x4d584354;
struct BoundedWriter {
    bytes: Vec<u8>,
    max: usize,
}
impl Write for BoundedWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.len() > self.max.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("serialized metadata limit reached"));
        }
        self.bytes
            .try_reserve(buf.len())
            .map_err(|e| io::Error::other(e.to_string()))?;
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub(crate) fn encode(value: &impl Serialize, max: usize) -> io::Result<Vec<u8>> {
    if max > MAX_BLOB_BYTES {
        return Err(io::Error::other("invalid serialization budget"));
    }
    let mut writer = BoundedWriter {
        bytes: Vec::new(),
        max,
    };
    serde_json::to_writer(&mut writer, value).map_err(io::Error::other)?;
    Ok(writer.bytes)
}
pub(crate) fn decode<T: DeserializeOwned>(bytes: &[u8], max: usize) -> io::Result<T> {
    if bytes.len() > max || max > MAX_BLOB_BYTES {
        return Err(io::Error::other("serialized metadata exceeds read budget"));
    }
    serde_json::from_slice(bytes).map_err(io::Error::other)
}
pub(crate) fn database(path: &Path) -> io::Result<Connection> {
    let conn = Connection::open(path).map_err(io::Error::other)?;
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(io::Error::other)?;
    let app: i32 = conn
        .pragma_query_value(None, "application_id", |r| r.get(0))
        .map_err(io::Error::other)?;
    let tables: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )
        .map_err(io::Error::other)?;
    if app != APPLICATION_ID && (app != 0 || tables != 0) {
        return Err(io::Error::other("database is not a MetaXtract scan store"));
    }
    if app == 0 {
        conn.pragma_update(None, "application_id", APPLICATION_ID)
            .map_err(io::Error::other)?;
    }
    conn.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF; PRAGMA cache_size=-16384;").map_err(io::Error::other)?;
    let _ = conn.set_limit(
        Limit::SQLITE_LIMIT_LENGTH,
        (MAX_BLOB_BYTES + 1024 * 1024) as i32,
    );
    Ok(conn)
}
