use crate::{Result, SCHEMA_REVISION, db, invalid};
use rusqlite::{Connection, TransactionBehavior};

pub(crate) fn migrate(conn: &mut Connection) -> Result<()> {
    let current: u32 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(db)?;
    if current > SCHEMA_REVISION {
        return Err(invalid("case was created by a newer schema"));
    }
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db)?;
    if current < 1 {
        tx.execute_batch("CREATE TABLE case_info(singleton INTEGER PRIMARY KEY CHECK(singleton=1),case_id TEXT NOT NULL UNIQUE,title TEXT NOT NULL,notes TEXT NOT NULL,created_ms INTEGER NOT NULL,updated_ms INTEGER NOT NULL,revision INTEGER NOT NULL CHECK(revision>=0));").map_err(db)?;
    }
    if current < 2 {
        tx.execute_batch("CREATE TABLE scans(id TEXT PRIMARY KEY,source TEXT NOT NULL,origin TEXT NOT NULL,status TEXT NOT NULL CHECK(status IN('running','completed','paused','cancelled','failed')),started_ms INTEGER NOT NULL,finished_ms INTEGER,record_count INTEGER NOT NULL DEFAULT 0,warning_count INTEGER NOT NULL DEFAULT 0,error_count INTEGER NOT NULL DEFAULT 0);
        CREATE TABLE records(id INTEGER PRIMARY KEY,scan_id TEXT NOT NULL REFERENCES scans(id),ordinal INTEGER NOT NULL CHECK(ordinal>=0),path TEXT NOT NULL COLLATE BINARY,status TEXT NOT NULL,mime TEXT NOT NULL,size_bytes TEXT NOT NULL,size_key BLOB NOT NULL,sha256 TEXT,data BLOB NOT NULL,digest BLOB NOT NULL,UNIQUE(scan_id,ordinal),UNIQUE(scan_id,path));
        CREATE TABLE raw_fields(record_id INTEGER NOT NULL REFERENCES records(id) ON DELETE CASCADE,ordinal INTEGER NOT NULL,observation_id TEXT NOT NULL,namespace TEXT NOT NULL,tag TEXT NOT NULL,data BLOB NOT NULL,digest BLOB NOT NULL,PRIMARY KEY(record_id,ordinal),UNIQUE(record_id,observation_id));
        CREATE TABLE normalized_claims(record_id INTEGER NOT NULL REFERENCES records(id) ON DELETE CASCADE,ordinal INTEGER NOT NULL,claim_id TEXT NOT NULL,key TEXT NOT NULL,value_text TEXT,data BLOB NOT NULL,digest BLOB NOT NULL,PRIMARY KEY(record_id,ordinal),UNIQUE(record_id,claim_id));
        CREATE TABLE conflicts(record_id INTEGER NOT NULL REFERENCES records(id) ON DELETE CASCADE,ordinal INTEGER NOT NULL,key TEXT NOT NULL,data BLOB NOT NULL,digest BLOB NOT NULL,PRIMARY KEY(record_id,ordinal));
        CREATE TABLE diagnostics(record_id INTEGER NOT NULL REFERENCES records(id) ON DELETE CASCADE,ordinal INTEGER NOT NULL,code TEXT NOT NULL,severity TEXT NOT NULL,data BLOB NOT NULL,digest BLOB NOT NULL,PRIMARY KEY(record_id,ordinal));").map_err(db)?;
    }
    if current < 3 {
        tx.execute_batch(
            "CREATE INDEX records_path_page ON records(scan_id,path,id);
        CREATE INDEX records_size_page ON records(scan_id,size_key,id);
        CREATE INDEX records_status_page ON records(scan_id,status,ordinal);
        CREATE INDEX records_mime_page ON records(scan_id,mime,ordinal);
        CREATE INDEX claims_lookup ON normalized_claims(key,value_text,record_id);
        CREATE INDEX raw_lookup ON raw_fields(namespace,tag,record_id);
        CREATE INDEX diagnostics_lookup ON diagnostics(code,severity,record_id);",
        )
        .map_err(db)?;
    }
    if current < 4 {
        tx.execute_batch("ALTER TABLE scans ADD COLUMN imported_from TEXT;
        CREATE INDEX scans_history ON scans(started_ms DESC,id DESC);
        CREATE TABLE case_notes(id INTEGER PRIMARY KEY,author TEXT NOT NULL,body TEXT NOT NULL,created_ms INTEGER NOT NULL,updated_ms INTEGER NOT NULL);
        CREATE TABLE annotations(record_id INTEGER PRIMARY KEY REFERENCES records(id) ON DELETE CASCADE,note TEXT NOT NULL,tags BLOB NOT NULL,updated_ms INTEGER NOT NULL);
        CREATE TABLE saved_filters(name TEXT PRIMARY KEY COLLATE NOCASE,query BLOB NOT NULL,updated_ms INTEGER NOT NULL);").map_err(db)?;
    }
    tx.pragma_update(None, "user_version", SCHEMA_REVISION)
        .map_err(db)?;
    tx.commit().map_err(db)
}
