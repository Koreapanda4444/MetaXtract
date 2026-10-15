use crate::{CaseWorkspace, Result, codec, db, hex, invalid, now_ms, text};
use rusqlite::{
    Connection, OptionalExtension, Transaction,
    functions::FunctionFlags,
    params, params_from_iter,
    types::{Value, ValueRef},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct AuditAnchor {
    pub case_id: String,
    pub sequence: i64,
    pub hash: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuditEvent {
    pub sequence: i64,
    pub time_ms: i64,
    pub actor: String,
    pub action: String,
    pub payload: serde_json::Value,
    pub previous_hash: String,
    pub hash: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IntegrityReport {
    pub anchor: AuditAnchor,
    pub records: u64,
    pub checked_payloads: u64,
    pub audit_events: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Change {
    table: String,
    key: String,
    previous: Option<String>,
    current: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Payload {
    details: serde_json::Value,
    changes: Vec<Change>,
}
struct Table {
    name: &'static str,
    keys: &'static [&'static str],
    columns: &'static [&'static str],
}
const TABLES: &[Table] = &[
    Table {
        name: "identities",
        keys: &["name"],
        columns: &[
            "name",
            "fingerprint",
            "public_pem",
            "created_ms",
            "updated_ms",
        ],
    },
    Table {
        name: "case_info",
        keys: &["singleton"],
        columns: &[
            "singleton",
            "case_id",
            "title",
            "notes",
            "created_ms",
            "updated_ms",
            "revision",
        ],
    },
    Table {
        name: "scans",
        keys: &["id"],
        columns: &[
            "id",
            "source",
            "origin",
            "status",
            "started_ms",
            "finished_ms",
            "record_count",
            "warning_count",
            "error_count",
            "imported_from",
        ],
    },
    Table {
        name: "records",
        keys: &["id"],
        columns: &[
            "id",
            "scan_id",
            "ordinal",
            "path",
            "status",
            "mime",
            "size_bytes",
            "size_key",
            "sha256",
            "digest",
        ],
    },
    Table {
        name: "raw_fields",
        keys: &["record_id", "ordinal"],
        columns: &[
            "record_id",
            "ordinal",
            "observation_id",
            "namespace",
            "tag",
            "digest",
        ],
    },
    Table {
        name: "normalized_claims",
        keys: &["record_id", "ordinal"],
        columns: &[
            "record_id",
            "ordinal",
            "claim_id",
            "key",
            "value_text",
            "digest",
        ],
    },
    Table {
        name: "conflicts",
        keys: &["record_id", "ordinal"],
        columns: &["record_id", "ordinal", "key", "digest"],
    },
    Table {
        name: "diagnostics",
        keys: &["record_id", "ordinal"],
        columns: &["record_id", "ordinal", "code", "severity", "digest"],
    },
    Table {
        name: "case_notes",
        keys: &["id"],
        columns: &["id", "author", "body", "created_ms", "updated_ms"],
    },
    Table {
        name: "annotations",
        keys: &["record_id"],
        columns: &["record_id", "note", "tags", "updated_ms"],
    },
    Table {
        name: "saved_filters",
        keys: &["name"],
        columns: &["name", "query", "updated_ms"],
    },
    Table {
        name: "imports",
        keys: &["id"],
        columns: &[
            "id",
            "kind",
            "source",
            "snapshot_sha256",
            "imported_ms",
            "details",
        ],
    },
    Table {
        name: "import_artifacts",
        keys: &["import_id", "path"],
        columns: &["import_id", "path", "digest"],
    },
    Table {
        name: "legacy_records",
        keys: &["record_id"],
        columns: &["record_id", "digest"],
    },
    Table {
        name: "scan_receipts",
        keys: &["scan_id", "key"],
        columns: &["scan_id", "key", "digest"],
    },
];
const PAYLOAD_BYTES: usize = 4 * 1024 * 1024;

pub(crate) fn register(conn: &Connection) -> Result<()> {
    conn.create_scalar_function(
        "mx_case_row_hash",
        -1,
        FunctionFlags::SQLITE_UTF8
            | FunctionFlags::SQLITE_DETERMINISTIC
            | FunctionFlags::SQLITE_INNOCUOUS,
        |ctx| {
            let mut hash = Sha256::new();
            hash.update(b"MetaXtract case row v1\0");
            for i in 0..ctx.len() {
                match ctx.get_raw(i) {
                    ValueRef::Null => hash.update([0]),
                    ValueRef::Integer(v) => {
                        hash.update([1]);
                        hash.update(v.to_le_bytes());
                    }
                    ValueRef::Real(v) => {
                        hash.update([2]);
                        hash.update(v.to_bits().to_le_bytes());
                    }
                    ValueRef::Text(v) => {
                        hash.update([3]);
                        field(&mut hash, v);
                    }
                    ValueRef::Blob(v) => {
                        hash.update([4]);
                        field(&mut hash, v);
                    }
                }
            }
            Ok(hash.finalize().to_vec())
        },
    )
    .map_err(db)
}
fn field(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}
fn key(table: &Table, prefix: &str) -> String {
    format!(
        "json_array({})",
        table
            .keys
            .iter()
            .map(|k| format!("{prefix}{k}"))
            .collect::<Vec<_>>()
            .join(",")
    )
}
fn digest(table: &Table, prefix: &str) -> String {
    format!(
        "mx_case_row_hash({})",
        table
            .columns
            .iter()
            .map(|k| format!("{prefix}{k}"))
            .collect::<Vec<_>>()
            .join(",")
    )
}
fn triggers(table: &Table) -> Vec<String> {
    let mut out = Vec::new();
    let old = key(table, "OLD.");
    let new = key(table, "NEW.");
    let hash = digest(table, "OLD.");
    let name = table.name;
    for (suffix, event) in [("bu", "UPDATE"), ("bd", "DELETE")] {
        out.push(format!("CREATE TRIGGER IF NOT EXISTS mx_audit_{name}_{suffix} BEFORE {event} ON {name} WHEN NOT EXISTS(SELECT 1 FROM audit_dirty WHERE table_name='{name}' AND key={old}) BEGIN SELECT CASE WHEN (SELECT digest FROM audit_state WHERE table_name='{name}' AND key={old})={hash} THEN NULL ELSE RAISE(ABORT,'case integrity guard differs') END; END"));
    }
    out.push(format!("CREATE TRIGGER IF NOT EXISTS mx_audit_{name}_ai AFTER INSERT ON {name} BEGIN INSERT OR IGNORE INTO audit_dirty VALUES('{name}',{new}); END"));
    out.push(format!("CREATE TRIGGER IF NOT EXISTS mx_audit_{name}_au AFTER UPDATE ON {name} BEGIN INSERT OR IGNORE INTO audit_dirty VALUES('{name}',{old}); INSERT OR IGNORE INTO audit_dirty VALUES('{name}',{new}); END"));
    out.push(format!("CREATE TRIGGER IF NOT EXISTS mx_audit_{name}_ad AFTER DELETE ON {name} BEGIN INSERT OR IGNORE INTO audit_dirty VALUES('{name}',{old}); END"));
    out
}
pub(crate) fn migrated(tx: &Transaction<'_>, from: u32, to: u32) -> Result<()> {
    for table in TABLES {
        for sql in triggers(table) {
            tx.execute_batch(&sql).map_err(db)?;
        }
    }
    let initialized: bool = tx
        .query_row(
            "SELECT initialized FROM audit_config WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .map_err(db)?;
    if initialized {
        tx.execute(
            "UPDATE case_info SET revision=revision+1,updated_ms=?1 WHERE singleton=1",
            [now_ms()?],
        )
        .map_err(db)?;
        append(
            tx,
            "system",
            "schema.migrate",
            &serde_json::json!({"from":from,"to":to}),
        )?;
    }
    Ok(())
}
pub(crate) fn ensure_anchor(conn: &mut Connection) -> Result<()> {
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(db)?;
    for table in TABLES {
        for sql in triggers(table) {
            tx.execute_batch(&sql).map_err(db)?;
        }
    }
    let initialized: bool = tx
        .query_row(
            "SELECT initialized FROM audit_config WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .map_err(db)?;
    if !initialized {
        let state: u64 = tx
            .query_row("SELECT count(*) FROM audit_state", [], |r| r.get(0))
            .map_err(db)?;
        if state != 0 {
            return Err(invalid("uninitialized audit state is not empty"));
        }
        verify_payloads(&tx)?;
        for table in TABLES {
            tx.execute_batch(&format!(
                "INSERT INTO audit_dirty SELECT '{}',{} FROM {}",
                table.name,
                key(table, ""),
                table.name
            ))
            .map_err(db)?;
        }
        append(
            &tx,
            "system",
            "case.anchor",
            &serde_json::json!({"schema":crate::SCHEMA_REVISION,"prior_history_attested":false}),
        )?;
        tx.execute(
            "UPDATE audit_config SET initialized=1 WHERE singleton=1",
            [],
        )
        .map_err(db)?;
    } else {
        head(&tx)?;
        let dirty: u64 = tx
            .query_row("SELECT count(*) FROM audit_dirty", [], |r| r.get(0))
            .map_err(db)?;
        if dirty != 0 {
            return Err(invalid("case has uncommitted audit changes"));
        }
    }
    tx.commit().map_err(db)
}
fn lookup(conn: &Connection, table: &Table, key: &str) -> Result<Option<Vec<u8>>> {
    let parts: Vec<serde_json::Value> = serde_json::from_str(key).map_err(std::io::Error::other)?;
    if parts.len() != table.keys.len() {
        return Err(invalid("invalid audit row key"));
    }
    let values = parts
        .into_iter()
        .map(|v| match v {
            serde_json::Value::String(s) => Ok(Value::Text(s)),
            serde_json::Value::Number(n) => n
                .as_i64()
                .map(Value::Integer)
                .ok_or_else(|| invalid("invalid audit integer key")),
            _ => Err(invalid("invalid audit key type")),
        })
        .collect::<Result<Vec<_>>>()?;
    conn.query_row(
        &format!(
            "SELECT {} FROM {} WHERE {}",
            digest(table, ""),
            table.name,
            table
                .keys
                .iter()
                .map(|k| format!("{k}=?"))
                .collect::<Vec<_>>()
                .join(" AND ")
        ),
        params_from_iter(values),
        |r| r.get(0),
    )
    .optional()
    .map_err(db)
}
pub(crate) fn append(
    tx: &Transaction<'_>,
    actor: &str,
    action: &str,
    details: &serde_json::Value,
) -> Result<()> {
    text(actor, 1024, false)?;
    text(action, 256, false)?;
    codec::encode(details, 64 * 1024)?;
    let case_id: String = tx
        .query_row("SELECT case_id FROM case_info WHERE singleton=1", [], |r| {
            r.get(0)
        })
        .map_err(db)?;
    loop {
        let dirty: Vec<(String, String)> = {
            let mut q = tx
                .prepare("SELECT table_name,key FROM audit_dirty ORDER BY table_name,key LIMIT 256")
                .map_err(db)?;
            q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(db)?
                .map(|r| r.map_err(db))
                .collect::<Result<_>>()?
        };
        if dirty.is_empty() {
            break;
        }
        let mut changes = Vec::new();
        for (name, key) in &dirty {
            let table = TABLES
                .iter()
                .find(|t| t.name == name)
                .ok_or_else(|| invalid("unknown audited table"))?;
            let previous: Option<Vec<u8>> = tx
                .query_row(
                    "SELECT digest FROM audit_state WHERE table_name=?1 AND key=?2",
                    params![name, key],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db)?;
            let current = lookup(tx, table, key)?;
            changes.push(Change {
                table: name.clone(),
                key: key.clone(),
                previous: previous.as_deref().map(hex),
                current: current.as_deref().map(hex),
            });
            if let Some(current) = current {
                tx.execute("INSERT INTO audit_state VALUES(?1,?2,?3) ON CONFLICT(table_name,key) DO UPDATE SET digest=excluded.digest",params![name,key,current]).map_err(db)?;
            } else {
                tx.execute(
                    "DELETE FROM audit_state WHERE table_name=?1 AND key=?2",
                    params![name, key],
                )
                .map_err(db)?;
            }
            tx.execute(
                "DELETE FROM audit_dirty WHERE table_name=?1 AND key=?2",
                params![name, key],
            )
            .map_err(db)?;
        }
        let previous: Option<(i64, Vec<u8>)> = tx
            .query_row(
                "SELECT sequence,hash FROM audit_log ORDER BY sequence DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db)?;
        let (sequence, previous) = previous
            .map(|(s, h)| (s + 1, h))
            .unwrap_or((1, vec![0; 32]));
        let time = now_ms()?;
        let payload = codec::encode(
            &Payload {
                details: details.clone(),
                changes,
            },
            PAYLOAD_BYTES,
        )?;
        let hash = event_hash(&case_id, sequence, time, actor, action, &payload, &previous);
        tx.execute(
            "INSERT INTO audit_log VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![sequence, time, actor, action, payload, previous, hash],
        )
        .map_err(db)?;
    }
    Ok(())
}
fn event_hash(
    case: &str,
    seq: i64,
    time: i64,
    actor: &str,
    action: &str,
    payload: &[u8],
    previous: &[u8],
) -> Vec<u8> {
    let mut hash = Sha256::new();
    hash.update(b"MetaXtract case audit v1\0");
    field(&mut hash, case.as_bytes());
    hash.update(seq.to_le_bytes());
    hash.update(time.to_le_bytes());
    field(&mut hash, actor.as_bytes());
    field(&mut hash, action.as_bytes());
    field(&mut hash, payload);
    field(&mut hash, previous);
    hash.finalize().to_vec()
}
fn head(conn: &Connection) -> Result<AuditAnchor> {
    conn.query_row("SELECT c.case_id,a.sequence,a.hash FROM case_info c JOIN audit_log a ON a.sequence=(SELECT max(sequence) FROM audit_log) WHERE c.singleton=1",[],|r|Ok(AuditAnchor{case_id:r.get(0)?,sequence:r.get(1)?,hash:hex(&r.get::<_,Vec<u8>>(2)?)})).map_err(db)
}
fn unhex(value: &str) -> Result<Vec<u8>> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid("invalid audit digest"));
    }
    (0..32)
        .map(|i| u8::from_str_radix(&value[i * 2..i * 2 + 2], 16).map_err(std::io::Error::other))
        .collect()
}
impl CaseWorkspace {
    pub fn audit_head(&self) -> Result<AuditAnchor> {
        let conn = self.lock()?;
        head(&conn)
    }
    pub fn record_activity(&self, action: &str, details: &serde_json::Value) -> Result<()> {
        self.write_detailed(action, details, |_| Ok(()))
    }
    pub fn audit_events(&self, after: i64, limit: usize) -> Result<Vec<AuditEvent>> {
        if !(1..=256).contains(&limit) {
            return Err(invalid("audit page must contain 1..256 events"));
        }
        let conn = self.lock()?;
        let mut q=conn.prepare("SELECT sequence,time_ms,actor,action,payload,previous_hash,hash FROM audit_log WHERE sequence>?1 ORDER BY sequence LIMIT ?2").map_err(db)?;
        let mut rows = q.query(params![after, limit as i64]).map_err(db)?;
        let mut out = Vec::new();
        let mut bytes = 0usize;
        while let Some(row) = rows.next().map_err(db)? {
            let data: Vec<u8> = row.get(4).map_err(db)?;
            if data.len() > PAYLOAD_BYTES {
                return Err(invalid("audit entry exceeds byte budget"));
            }
            if bytes + data.len() > 16 * 1024 * 1024 {
                break;
            }
            bytes += data.len();
            out.push(AuditEvent {
                sequence: row.get(0).map_err(db)?,
                time_ms: row.get(1).map_err(db)?,
                actor: row.get(2).map_err(db)?,
                action: row.get(3).map_err(db)?,
                payload: codec::decode(&data, PAYLOAD_BYTES)?,
                previous_hash: hex(&row.get::<_, Vec<u8>>(5).map_err(db)?),
                hash: hex(&row.get::<_, Vec<u8>>(6).map_err(db)?),
            });
        }
        Ok(out)
    }
    pub fn verify_integrity(&self, expected: Option<&AuditAnchor>) -> Result<IntegrityReport> {
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(db)?;
        let check: String = tx
            .query_row("PRAGMA quick_check", [], |r| r.get(0))
            .map_err(db)?;
        if check != "ok" {
            return Err(invalid("case database integrity differs"));
        }
        if tx
            .prepare("PRAGMA foreign_key_check")
            .map_err(db)?
            .query([])
            .map_err(db)?
            .next()
            .map_err(db)?
            .is_some()
        {
            return Err(invalid("case foreign keys differ"));
        }
        let anchor = head(&tx)?;
        if expected.is_some_and(|e| e != &anchor) {
            return Err(invalid("case does not match the independent audit anchor"));
        }
        let temporary = tempfile::NamedTempFile::new()?;
        let mut expected_db = Connection::open(temporary.path()).map_err(db)?;
        expected_db.execute_batch("PRAGMA cache_size=-16384; CREATE TABLE expected(table_name TEXT NOT NULL,key TEXT NOT NULL,digest BLOB NOT NULL,PRIMARY KEY(table_name,key));").map_err(db)?;
        let expected_tx = expected_db.transaction().map_err(db)?;
        let mut q=tx.prepare("SELECT sequence,time_ms,actor,action,payload,previous_hash,hash FROM audit_log ORDER BY sequence").map_err(db)?;
        let mut rows = q.query([]).map_err(db)?;
        let mut previous = vec![0u8; 32];
        let mut sequence = 0i64;
        while let Some(row) = rows.next().map_err(db)? {
            let seq: i64 = row.get(0).map_err(db)?;
            let time: i64 = row.get(1).map_err(db)?;
            let actor: String = row.get(2).map_err(db)?;
            let action: String = row.get(3).map_err(db)?;
            let data: Vec<u8> = row.get(4).map_err(db)?;
            let prev: Vec<u8> = row.get(5).map_err(db)?;
            let hash: Vec<u8> = row.get(6).map_err(db)?;
            if seq != sequence + 1
                || prev != previous
                || hash != event_hash(&anchor.case_id, seq, time, &actor, &action, &data, &prev)
            {
                return Err(invalid("case audit chain differs"));
            }
            let payload: Payload = codec::decode(&data, PAYLOAD_BYTES)?;
            for change in payload.changes {
                if !TABLES.iter().any(|t| t.name == change.table) {
                    return Err(invalid("audit entry references an unknown table"));
                }
                let stored: Option<Vec<u8>> = expected_tx
                    .query_row(
                        "SELECT digest FROM expected WHERE table_name=?1 AND key=?2",
                        params![change.table, change.key],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(db)?;
                if stored.as_deref().map(hex) != change.previous {
                    return Err(invalid("audit row transition differs"));
                }
                if let Some(value) = change.current {
                    expected_tx.execute("INSERT INTO expected VALUES(?1,?2,?3) ON CONFLICT(table_name,key) DO UPDATE SET digest=excluded.digest",params![change.table,change.key,unhex(&value)?]).map_err(db)?;
                } else {
                    expected_tx
                        .execute(
                            "DELETE FROM expected WHERE table_name=?1 AND key=?2",
                            params![change.table, change.key],
                        )
                        .map_err(db)?;
                }
            }
            sequence = seq;
            previous = hash;
        }
        if sequence != anchor.sequence || hex(&previous) != anchor.hash {
            return Err(invalid("audit head differs"));
        }
        for table in TABLES {
            let mut actual = tx
                .prepare(&format!(
                    "SELECT {},{} FROM {} ORDER BY {}",
                    key(table, ""),
                    digest(table, ""),
                    table.name,
                    key(table, "")
                ))
                .map_err(db)?;
            let mut actual = actual.query([]).map_err(db)?;
            let mut want = expected_tx
                .prepare("SELECT key,digest FROM expected WHERE table_name=?1 ORDER BY key")
                .map_err(db)?;
            let mut want = want.query([table.name]).map_err(db)?;
            let mut state = tx
                .prepare("SELECT key,digest FROM audit_state WHERE table_name=?1 ORDER BY key")
                .map_err(db)?;
            let mut state = state.query([table.name]).map_err(db)?;
            loop {
                match (
                    actual.next().map_err(db)?,
                    want.next().map_err(db)?,
                    state.next().map_err(db)?,
                ) {
                    (None, None, None) => break,
                    (Some(a), Some(b), Some(c)) => {
                        let k: String = b.get(0).map_err(db)?;
                        let d: Vec<u8> = b.get(1).map_err(db)?;
                        if a.get::<_, String>(0).map_err(db)? != k
                            || c.get::<_, String>(0).map_err(db)? != k
                            || a.get::<_, Vec<u8>>(1).map_err(db)? != d
                            || c.get::<_, Vec<u8>>(1).map_err(db)? != d
                        {
                            return Err(invalid("case content differs from audited state"));
                        }
                    }
                    _ => return Err(invalid("case audited row inventory differs")),
                }
            }
            for sql in triggers(table) {
                let name = sql
                    .split_whitespace()
                    .nth(5)
                    .ok_or_else(|| invalid("invalid internal audit trigger"))?;
                let stored: String = tx
                    .query_row(
                        "SELECT sql FROM sqlite_master WHERE type='trigger' AND name=?1",
                        [name],
                        |r| r.get(0),
                    )
                    .map_err(db)?;
                let normalize = |s: &str| {
                    s.replace("IF NOT EXISTS ", "")
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                };
                if normalize(&sql) != normalize(&stored) {
                    return Err(invalid("case audit trigger differs"));
                }
            }
        }
        let allowed = TABLES
            .iter()
            .map(|t| t.name)
            .chain(["audit_log", "audit_state", "audit_dirty", "audit_config"])
            .collect::<std::collections::BTreeSet<_>>();
        let mut names = tx
            .prepare(
                "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            )
            .map_err(db)?;
        for name in names.query_map([], |r| r.get::<_, String>(0)).map_err(db)? {
            if !allowed.contains(name.map_err(db)?.as_str()) {
                return Err(invalid("case contains an unrecognized table"));
            }
        }
        let dirty: u64 = tx
            .query_row("SELECT count(*) FROM audit_dirty", [], |r| r.get(0))
            .map_err(db)?;
        if dirty != 0 {
            return Err(invalid("case contains uncommitted audit changes"));
        }
        let checked_payloads = verify_payloads(&tx)?;
        let records = tx
            .query_row("SELECT count(*) FROM records", [], |r| r.get(0))
            .map_err(db)?;
        Ok(IntegrityReport {
            anchor,
            records,
            checked_payloads,
            audit_events: sequence as u64,
        })
    }
}
fn verify_payloads(conn: &Connection) -> Result<u64> {
    let mut checked = 0u64;
    for (table, column) in [
        ("records", "data"),
        ("raw_fields", "data"),
        ("normalized_claims", "data"),
        ("conflicts", "data"),
        ("diagnostics", "data"),
        ("import_artifacts", "data"),
        ("legacy_records", "original"),
        ("scan_receipts", "data"),
    ] {
        let mut q = conn
            .prepare(&format!("SELECT {column},digest FROM {table}"))
            .map_err(db)?;
        let mut rows = q.query([]).map_err(db)?;
        while let Some(row) = rows.next().map_err(db)? {
            let data: Vec<u8> = row.get(0).map_err(db)?;
            let digest: Vec<u8> = row.get(1).map_err(db)?;
            if data.len() > codec::MAX_RECORD_BYTES || Sha256::digest(&data).as_slice() != digest {
                return Err(invalid("case payload checksum differs"));
            }
            if table == "records" {
                let record = codec::decode(&data, codec::MAX_RECORD_BYTES)?;
                crate::records::validate(&record)?;
            }
            checked += 1;
        }
    }
    Ok(checked)
}
