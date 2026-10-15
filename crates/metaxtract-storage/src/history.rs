use crate::{CaseWorkspace, RecordQuery, Result, ScanStatus, codec, db, invalid, now_ms, text};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct ScanInfo {
    pub id: String,
    pub source: String,
    pub origin: String,
    pub status: ScanStatus,
    pub started_ms: i64,
    pub finished_ms: Option<i64>,
    pub record_count: u64,
    pub warning_count: u64,
    pub error_count: u64,
    pub imported_from: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScanCursor {
    case_id: String,
    revision: u64,
    started_ms: i64,
    id: String,
}
#[derive(Clone, Debug)]
pub struct ScanHistoryPage {
    pub items: Vec<ScanInfo>,
    pub next: Option<ScanCursor>,
}
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct CaseNote {
    pub id: i64,
    pub author: String,
    pub body: String,
    pub created_ms: i64,
    pub updated_ms: i64,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteHeader {
    pub id: i64,
    pub author: String,
    pub preview: String,
    pub created_ms: i64,
    pub updated_ms: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct Annotation {
    pub record_id: i64,
    pub note: String,
    pub tags: Vec<String>,
    pub updated_ms: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum FilterQuery {
    Records(RecordQuery),
    Legacy { query: String, status: String },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedFilter {
    pub name: String,
    pub query: FilterQuery,
    pub updated_ms: i64,
}

impl CaseWorkspace {
    pub fn scan_history(
        &self,
        cursor: Option<&ScanCursor>,
        limit: usize,
    ) -> Result<ScanHistoryPage> {
        page_limit(limit)?;
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(db)?;
        let (case_id, revision): (String, u64) = tx
            .query_row(
                "SELECT case_id,revision FROM case_info WHERE singleton=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(db)?;
        if cursor.is_some_and(|c| c.case_id != case_id || c.revision != revision) {
            return Err(invalid("case changed; restart scan history pagination"));
        }
        let mut statement=tx.prepare("SELECT id,source,origin,status,started_ms,finished_ms,record_count,warning_count,error_count,imported_from,length(CAST(source AS BLOB))+coalesce(length(CAST(imported_from AS BLOB)),0) FROM scans WHERE ?1 IS NULL OR (started_ms,id)<(?1,?2) ORDER BY started_ms DESC,id DESC LIMIT ?3").map_err(db)?;
        let mut rows = statement
            .query(params![
                cursor.map(|c| c.started_ms),
                cursor.map(|c| c.id.as_str()),
                (limit + 1) as i64
            ])
            .map_err(db)?;
        let mut items = Vec::new();
        let mut bytes = 0usize;
        let mut more = false;
        while let Some(row) = rows.next().map_err(db)? {
            if items.len() == limit {
                more = true;
                break;
            }
            let size: i64 = row.get(10).map_err(db)?;
            let size = usize::try_from(size)
                .map_err(|_| invalid("invalid scan header size"))?
                .saturating_add(2048);
            if size > 16 * 1024 * 1024usize - bytes {
                if items.is_empty() {
                    return Err(invalid("scan header exceeds page budget"));
                }
                more = true;
                break;
            }
            bytes += size;
            items.push(ScanInfo {
                id: row.get(0).map_err(db)?,
                source: row.get(1).map_err(db)?,
                origin: row.get(2).map_err(db)?,
                status: ScanStatus::parse(&row.get::<_, String>(3).map_err(db)?)?,
                started_ms: row.get(4).map_err(db)?,
                finished_ms: row.get(5).map_err(db)?,
                record_count: row.get(6).map_err(db)?,
                warning_count: row.get(7).map_err(db)?,
                error_count: row.get(8).map_err(db)?,
                imported_from: row.get(9).map_err(db)?,
            });
        }
        let next = if more {
            items.last().map(|s| ScanCursor {
                case_id,
                revision,
                started_ms: s.started_ms,
                id: s.id.clone(),
            })
        } else {
            None
        };
        Ok(ScanHistoryPage { items, next })
    }
    pub fn resume_scan(&self, id: &str) -> Result<()> {
        self.write_named("resume_scan",|tx|{if tx.execute("UPDATE scans SET status='running',finished_ms=NULL WHERE id=?1 AND status='paused'",[id]).map_err(db)?!=1{return Err(invalid("scan is not paused"));}Ok(())})
    }
    pub fn set_notes(&self, notes: &str) -> Result<()> {
        text(notes, 1024 * 1024, true)?;
        self.write_named("set_notes", |tx| {
            tx.execute("UPDATE case_info SET notes=?1 WHERE singleton=1", [notes])
                .map_err(db)?;
            Ok(())
        })
    }
    pub fn add_note(&self, author: &str, body: &str) -> Result<i64> {
        text(author, 1024, true)?;
        text(body, 1024 * 1024, false)?;
        self.write_named("add_note", |tx| {
            tx.execute(
                "INSERT INTO case_notes(author,body,created_ms,updated_ms) VALUES(?1,?2,?3,?3)",
                params![author, body, now_ms()?],
            )
            .map_err(db)?;
            Ok(tx.last_insert_rowid())
        })
    }
    pub fn update_note(&self, id: i64, body: &str) -> Result<()> {
        text(body, 1024 * 1024, false)?;
        self.write_named("update_note", |tx| {
            if tx
                .execute(
                    "UPDATE case_notes SET body=?2,updated_ms=?3 WHERE id=?1",
                    params![id, body, now_ms()?],
                )
                .map_err(db)?
                != 1
            {
                return Err(invalid("unknown case note"));
            }
            Ok(())
        })
    }
    pub fn remove_note(&self, id: i64) -> Result<bool> {
        self.write_named("remove_note", |tx| {
            tx.execute("DELETE FROM case_notes WHERE id=?1", [id])
                .map(|n| n != 0)
                .map_err(db)
        })
    }
    pub fn note(&self, id: i64) -> Result<Option<CaseNote>> {
        self.lock()?
            .query_row(
                "SELECT id,author,body,created_ms,updated_ms FROM case_notes WHERE id=?1",
                [id],
                |r| {
                    Ok(CaseNote {
                        id: r.get(0)?,
                        author: r.get(1)?,
                        body: r.get(2)?,
                        created_ms: r.get(3)?,
                        updated_ms: r.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(db)
    }
    pub fn note_headers(&self, after: i64, limit: usize) -> Result<Vec<NoteHeader>> {
        page_limit(limit)?;
        let conn = self.lock()?;
        let mut q=conn.prepare("SELECT id,author,substr(body,1,256),created_ms,updated_ms FROM case_notes WHERE id>?1 ORDER BY id LIMIT ?2").map_err(db)?;
        q.query_map(params![after, limit as i64], |r| {
            Ok(NoteHeader {
                id: r.get(0)?,
                author: r.get(1)?,
                preview: r.get(2)?,
                created_ms: r.get(3)?,
                updated_ms: r.get(4)?,
            })
        })
        .map_err(db)?
        .map(|r| r.map_err(db))
        .collect()
    }
    pub fn annotate(&self, id: i64, note: &str, tags: &[String]) -> Result<()> {
        validate_annotation(note, tags)?;
        let data = codec::encode(&tags, 64 * 1024)?;
        self.write_named("annotate",|tx|{tx.execute("INSERT INTO annotations VALUES(?1,?2,?3,?4) ON CONFLICT(record_id) DO UPDATE SET note=excluded.note,tags=excluded.tags,updated_ms=excluded.updated_ms",params![id,note,data,now_ms()?]).map_err(db)?;Ok(())})
    }
    pub fn annotation(&self, id: i64) -> Result<Option<Annotation>> {
        let row: Option<(String, Vec<u8>, i64)> = self
            .lock()?
            .query_row(
                "SELECT note,tags,updated_ms FROM annotations WHERE record_id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(db)?;
        row.map(|(note, tags, updated_ms)| {
            let tags: Vec<String> = codec::decode(&tags, 64 * 1024)?;
            validate_annotation(&note, &tags)?;
            Ok(Annotation {
                record_id: id,
                note,
                tags,
                updated_ms,
            })
        })
        .transpose()
    }
    pub fn remove_annotation(&self, id: i64) -> Result<bool> {
        self.write_named("remove_annotation", |tx| {
            tx.execute("DELETE FROM annotations WHERE record_id=?1", [id])
                .map(|n| n != 0)
                .map_err(db)
        })
    }
    pub fn save_filter(&self, name: &str, query: &FilterQuery) -> Result<()> {
        text(name, 1024, false)?;
        if name.chars().count() > 100 {
            return Err(invalid("filter name exceeds 100 characters"));
        }
        match query {
            FilterQuery::Records(q) => q.validate()?,
            FilterQuery::Legacy { query, status } => {
                text(query, 16 * 1024, true)?;
                if !["All", "OK", "Warning", "Error"].contains(&status.as_str()) {
                    return Err(invalid("invalid legacy filter status"));
                }
            }
        }
        let query = codec::encode(query, 32 * 1024)?;
        self.write_named("save_filter",|tx|{tx.execute("INSERT INTO saved_filters VALUES(?1,?2,?3) ON CONFLICT(name) DO UPDATE SET query=excluded.query,updated_ms=excluded.updated_ms",params![name.trim(),query,now_ms()?]).map_err(db)?;Ok(())})
    }
    pub fn filters(&self, after: Option<&str>, limit: usize) -> Result<Vec<SavedFilter>> {
        page_limit(limit)?;
        let conn = self.lock()?;
        let mut q=conn.prepare("SELECT name,query,updated_ms FROM saved_filters WHERE ?1 IS NULL OR name>?1 COLLATE NOCASE ORDER BY name COLLATE NOCASE LIMIT ?2").map_err(db)?;
        let rows = q
            .query_map(params![after, limit as i64], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Vec<u8>>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })
            .map_err(db)?;
        rows.map(|row| {
            let (name, data, updated_ms) = row.map_err(db)?;
            Ok(SavedFilter {
                name,
                query: codec::decode(&data, 32 * 1024)?,
                updated_ms,
            })
        })
        .collect()
    }
    pub fn remove_filter(&self, name: &str) -> Result<bool> {
        self.write_named("remove_filter", |tx| {
            tx.execute(
                "DELETE FROM saved_filters WHERE name=?1 COLLATE NOCASE",
                [name],
            )
            .map(|n| n != 0)
            .map_err(db)
        })
    }
}

pub(crate) fn validate_annotation(note: &str, tags: &[String]) -> Result<()> {
    text(note, 1024 * 1024, true)?;
    if tags.len() > 64 {
        return Err(invalid("annotation exceeds 64 tags"));
    }
    let mut seen = BTreeSet::new();
    for tag in tags {
        text(tag, 1024, false)?;
        if tag.chars().count() > 100 || tag.chars().any(char::is_control) || !seen.insert(tag) {
            return Err(invalid("invalid or duplicate annotation tag"));
        }
    }
    Ok(())
}
fn page_limit(limit: usize) -> Result<()> {
    if !(1..=256).contains(&limit) {
        return Err(invalid("page must contain 1..256 rows"));
    }
    Ok(())
}
