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
    tx.pragma_update(None, "user_version", SCHEMA_REVISION)
        .map_err(db)?;
    tx.commit().map_err(db)
}
