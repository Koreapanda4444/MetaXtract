use super::*;
use crate::{FilterQuery, history::validate_annotation};
use metaxtract_schema::{EvidencePath, RelativePath};
use rusqlite::{Connection, OpenFlags};

impl CaseWorkspace {
    pub fn import_python_case(
        &self,
        path: impl AsRef<Path>,
        limits: &ImportLimits,
    ) -> Result<ImportSummary> {
        limits.validate()?;
        let path = path.as_ref();
        if std::fs::metadata(path)?.len() > limits.max_input_bytes {
            return Err(invalid("Python case exceeds import budget"));
        }
        let original = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(db)?;
        original
            .execute_batch("PRAGMA trusted_schema=OFF; PRAGMA busy_timeout=15000;")
            .map_err(db)?;
        let app: i32 = original
            .pragma_query_value(None, "application_id", |r| r.get(0))
            .map_err(db)?;
        let version: u32 = original
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(db)?;
        if app != 0x4d584354 || !(1..=3).contains(&version) {
            return Err(invalid("unsupported Python case schema"));
        }
        let snapshot = tempfile::NamedTempFile::new()?;
        transport::database_snapshot(&original, snapshot.path(), limits.max_input_bytes)?;
        if snapshot.as_file().metadata()?.len() > limits.max_input_bytes {
            return Err(invalid("Python case backup exceeds import budget"));
        }
        let hash = transport::hash_file(snapshot.path(), limits.max_input_bytes)?;
        let source = Connection::open_with_flags(
            snapshot.path(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(db)?;
        source
            .execute_batch(
                "PRAGMA trusted_schema=OFF; PRAGMA foreign_keys=ON; PRAGMA cache_size=-16384;",
            )
            .map_err(db)?;
        source
            .set_limit(
                rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH,
                limits.max_record_bytes as i32,
            )
            .map_err(db)?;
        let check: String = source
            .query_row("PRAGMA quick_check", [], |r| r.get(0))
            .map_err(db)?;
        if check != "ok" {
            return Err(invalid("Python case database integrity differs"));
        }
        if source
            .prepare("PRAGMA foreign_key_check")
            .map_err(db)?
            .query([])
            .map_err(db)?
            .next()
            .map_err(db)?
            .is_some()
        {
            return Err(invalid("Python case foreign keys differ"));
        }
        let info: Value = source
            .query_row("SELECT * FROM case_info WHERE singleton=1", [], sqlite_json)
            .map_err(db)?;
        self.write(|tx|{
            let mut scans=Vec::new();let mut count=0u64;
            let mut runs=source.prepare("SELECT r.*,s.path AS source_path,s.kind AS source_kind FROM scan_runs r JOIN sources s ON s.source_id=r.source_id ORDER BY r.started_at,r.rowid").map_err(db)?;
            let mut rows=runs.query([]).map_err(db)?;
            while let Some(row)=rows.next().map_err(db)?{
                let raw=sqlite_json(row).map_err(db)?;
                let old:String=row.get("scan_id").map_err(db)?;
                let source_path:String=row.get("source_path").map_err(db)?;
                let scan=new_scan(tx,&source_path,"import_python_case")?;
                let mut records=source.prepare("SELECT * FROM records WHERE scan_id=?1 ORDER BY ordinal").map_err(db)?;
                let mut records=records.query([&old]).map_err(db)?;
                while let Some(row)=records.next().map_err(db)?{
                    if count>=limits.max_records{return Err(invalid("Python case exceeds record count budget"));}
                    let metadata:String=row.get("metadata_json").map_err(db)?;
                    let warnings:String=row.get("warnings_json").map_err(db)?;
                    let errors:String=row.get("errors_json").map_err(db)?;
                    let value=json!({"path":row.get::<_,String>("path").map_err(db)?,"mime":row.get::<_,String>("mime").map_err(db)?,"size_bytes":row.get::<_,u64>("size_bytes").map_err(db)?,"sha256":row.get::<_,String>("sha256").map_err(db)?,"metadata":serde_json::from_str::<Value>(&metadata).map_err(std::io::Error::other)?,"warnings":serde_json::from_str::<Value>(&warnings).map_err(std::io::Error::other)?,"errors":serde_json::from_str::<Value>(&errors).map_err(std::io::Error::other)?,"original_sqlite_row":sqlite_json(row).map_err(db)?});
                    let record=legacy::convert(&value)?;let id=insert_record(tx,&scan,&record)?;
                    let data=codec::encode(&value,limits.max_record_bytes)?;
                    tx.execute("INSERT INTO legacy_records VALUES(?1,?2,?3)",params![id,&data,Sha256::digest(&data).to_vec()]).map_err(db)?;count+=1;
                }
                let status:String=row.get("status").map_err(db)?;
                if !["running","completed","cancelled","failed"].contains(&status.as_str()){return Err(invalid("invalid Python scan state"));}
                let started:String=row.get("started_at").map_err(db)?;let completed:Option<String>=row.get("completed_at").map_err(db)?;
                tx.execute("UPDATE scans SET status=?2,started_ms=coalesce(CAST(unixepoch(?3,'subsec')*1000 AS INTEGER),0),finished_ms=CAST(unixepoch(?4,'subsec')*1000 AS INTEGER),imported_from=?5 WHERE id=?1",params![scan,status,started,completed,path.to_string_lossy()]).map_err(db)?;
                let data=codec::encode(&raw,limits.max_control_bytes)?;
                tx.execute("INSERT INTO scan_receipts VALUES(?1,'python_scan',?2,?3)",params![scan,&data,Sha256::digest(&data).to_vec()]).map_err(db)?;
                if has_table(&source,"record_annotations")?{
                    let mut annotations=source.prepare("SELECT a.*,r.path AS actual_path FROM record_annotations a JOIN records r ON r.scan_id=a.scan_id AND r.path=a.record_path COLLATE NOCASE WHERE a.scan_id=?1").map_err(db)?;
                    let mut annotations=annotations.query([&old]).map_err(db)?;
                    while let Some(row)=annotations.next().map_err(db)?{
                        let path:String=row.get("actual_path").map_err(db)?;
                        let path=EvidencePath::root(RelativePath::parse(path).map_err(std::io::Error::other)?).to_string();
                        let id:i64=tx.query_row("SELECT id FROM records WHERE scan_id=?1 AND path=?2",params![scan,path],|r|r.get(0)).map_err(db)?;
                        let note:String=row.get("note").map_err(db)?;let tags:String=row.get("tags_json").map_err(db)?;
                        let tags:Vec<String>=serde_json::from_str(&tags).map_err(std::io::Error::other)?;validate_annotation(&note,&tags)?;
                        let updated:String=row.get("updated_at").map_err(db)?;
                        tx.execute("INSERT INTO annotations VALUES(?1,?2,?3,coalesce(CAST(unixepoch(?4,'subsec')*1000 AS INTEGER),0))",params![id,note,codec::encode(&tags,64*1024)?,updated]).map_err(db)?;
                    }
                }
                scans.push(scan);
            }
            if has_table(&source,"saved_filters")?{
                let mut filters=source.prepare("SELECT name,query,status,updated_at FROM saved_filters ORDER BY name").map_err(db)?;
                let mut rows=filters.query([]).map_err(db)?;
                while let Some(row)=rows.next().map_err(db)?{
                    let name:String=row.get(0).map_err(db)?;crate::text(&name,1024,false)?;
                    let query:String=row.get(1).map_err(db)?;crate::text(&query,16*1024,true)?;
                    let status:String=row.get(2).map_err(db)?;if !["All","OK","Warning","Error"].contains(&status.as_str()){return Err(invalid("invalid Python saved filter status"));}
                    let filter=FilterQuery::Legacy{query,status};let data=codec::encode(&filter,32*1024)?;let updated:String=row.get(3).map_err(db)?;
                    let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM saved_filters WHERE name=?1 COLLATE NOCASE)",[&name],|r|r.get(0)).map_err(db)?;
                    if exists{return Err(invalid("imported filter name collides with an existing filter"));}
                    tx.execute("INSERT INTO saved_filters VALUES(?1,?2,coalesce(CAST(unixepoch(?3,'subsec')*1000 AS INTEGER),0))",params![name,data,updated]).map_err(db)?;
                }
            }
            let import_id=receipt(tx,"python_case",path,&hash,&json!({"source_schema":version,"source_path":native_path(path),"case_info":info}))?;
            artifact(tx,import_id,"python-case-info.json",&codec::encode(&info,limits.max_control_bytes)?)?;
            if let Some(notes)=info.get("notes").and_then(Value::as_str).filter(|v|!v.trim().is_empty()){
                crate::text(notes,1024*1024,true)?;tx.execute("INSERT INTO case_notes(author,body,created_ms,updated_ms) VALUES('Python case import',?1,?2,?2)",params![notes,now_ms()?]).map_err(db)?;
            }
            Ok(ImportSummary{import_id,scans,records:count,signed:false,snapshot_sha256:hash.clone()})
        })
    }
}
fn has_table(conn: &Connection, name: &str) -> Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [name],
        |r| r.get(0),
    )
    .map_err(db)
}
fn sqlite_json(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use rusqlite::types::ValueRef;
    let mut out = serde_json::Map::new();
    for index in 0..row.as_ref().column_count() {
        let value = match row.get_ref(index)? {
            ValueRef::Null => Value::Null,
            ValueRef::Integer(v) => json!(v),
            ValueRef::Real(v) => json!(v),
            ValueRef::Text(v) => match std::str::from_utf8(v) {
                Ok(v) => json!(v),
                Err(_) => json!({"sqlite_text_base64":STANDARD.encode(v)}),
            },
            ValueRef::Blob(v) => json!({"sqlite_blob_base64":STANDARD.encode(v)}),
        };
        out.insert(row.as_ref().column_name(index)?.into(), value);
    }
    Ok(Value::Object(out))
}
