use crate::{CaseWorkspace, Result, codec, db, hex, invalid, text};
use metaxtract_schema::{CanonicalKey, ExtractionStatus};
use rusqlite::{params_from_iter, types::Value};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const PAGE_BYTES: usize = 16 * 1024 * 1024;
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, Eq, PartialEq)]
pub enum RecordOrder {
    #[default]
    Insertion,
    Path,
    Size,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecordQuery {
    pub scan_id: String,
    pub status: Option<ExtractionStatus>,
    pub mime: Option<String>,
    pub path_prefix: Option<String>,
    pub claim_key: Option<CanonicalKey>,
    pub claim_text: Option<String>,
    pub has_conflicts: Option<bool>,
    pub min_size: Option<u64>,
    pub max_size: Option<u64>,
    pub order: RecordOrder,
}
impl RecordQuery {
    pub fn new(scan_id: impl Into<String>) -> Self {
        Self {
            scan_id: scan_id.into(),
            status: None,
            mime: None,
            path_prefix: None,
            claim_key: None,
            claim_text: None,
            has_conflicts: None,
            min_size: None,
            max_size: None,
            order: RecordOrder::Insertion,
        }
    }
    pub(crate) fn validate(&self) -> Result<()> {
        text(&self.scan_id, 1024, false)?;
        for value in [&self.mime, &self.path_prefix, &self.claim_text]
            .into_iter()
            .flatten()
        {
            text(value, 1024 * 1024, true)?;
        }
        if self.claim_text.is_some() && self.claim_key.is_none() {
            return Err(invalid("claim text requires a canonical key"));
        }
        if matches!((self.min_size,self.max_size),(Some(a),Some(b)) if a>b) {
            return Err(invalid("invalid size range"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
enum Position {
    Insertion(i64),
    Path(String, i64),
    Size(Vec<u8>, i64),
}
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct PageCursor {
    case_id: String,
    revision: u64,
    query: String,
    position: Position,
}
#[derive(Clone, Debug)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next: Option<PageCursor>,
}
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct RecordHeader {
    pub id: i64,
    pub ordinal: u64,
    pub path: String,
    pub status: ExtractionStatus,
    pub mime: String,
    pub size_bytes: u64,
    pub sha256: Option<String>,
}

impl CaseWorkspace {
    pub fn query_records(
        &self,
        query: &RecordQuery,
        cursor: Option<&PageCursor>,
        limit: usize,
    ) -> Result<Page<RecordHeader>> {
        query.validate()?;
        if !(1..=256).contains(&limit) {
            return Err(invalid("record page must contain 1..256 items"));
        }
        let fingerprint = hex(&Sha256::digest(codec::encode(query, 4 * 1024 * 1024)?));
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(db)?;
        let (case_id, revision): (String, u64) = tx
            .query_row(
                "SELECT case_id,revision FROM case_info WHERE singleton=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(db)?;
        if let Some(cursor) = cursor {
            if cursor.case_id != case_id
                || cursor.revision != revision
                || cursor.query != fingerprint
            {
                return Err(invalid("case or query changed; restart pagination"));
            }
        }
        let (mut sql, mut values) = selection(query);
        if let Some(cursor) = cursor {
            match (&cursor.position, query.order) {
                (Position::Insertion(n), RecordOrder::Insertion) if *n >= 0 => {
                    sql.push_str(" AND r.ordinal>?");
                    values.push(Value::Integer(*n));
                }
                (Position::Path(path, id), RecordOrder::Path)
                    if *id > 0 && path.len() <= PAGE_BYTES =>
                {
                    sql.push_str(" AND (r.path,r.id)>(?,?)");
                    values.extend([Value::Text(path.clone()), Value::Integer(*id)]);
                }
                (Position::Size(bytes, id), RecordOrder::Size) if bytes.len() == 8 && *id > 0 => {
                    sql.push_str(" AND (r.size_key,r.id)>(?,?)");
                    values.extend([Value::Blob(bytes.clone()), Value::Integer(*id)]);
                }
                _ => return Err(invalid("invalid page cursor")),
            }
        }
        sql.push_str(match query.order {
            RecordOrder::Insertion => " ORDER BY r.ordinal",
            RecordOrder::Path => " ORDER BY r.path,r.id",
            RecordOrder::Size => " ORDER BY r.size_key,r.id",
        });
        sql.push_str(" LIMIT ?");
        values.push(Value::Integer((limit + 1) as i64));
        let mut statement = tx.prepare(&sql).map_err(db)?;
        let mut rows = statement.query(params_from_iter(values)).map_err(db)?;
        let mut items = Vec::new();
        let mut bytes = 0usize;
        let mut has_more = false;
        while let Some(row) = rows.next().map_err(db)? {
            if items.len() == limit {
                has_more = true;
                break;
            }
            let strings: i64 = row.get(7).map_err(db)?;
            let needed = usize::try_from(strings)
                .map_err(|_| invalid("invalid record header length"))?
                .saturating_add(256);
            if needed > PAGE_BYTES.saturating_sub(bytes) {
                if items.is_empty() {
                    return Err(invalid("record header exceeds page byte budget"));
                }
                has_more = true;
                break;
            }
            bytes += needed;
            let status: String = row.get(3).map_err(db)?;
            let status = match status.as_str() {
                "Complete" => ExtractionStatus::Complete,
                "Partial" => ExtractionStatus::Partial,
                "Failed" => ExtractionStatus::Failed,
                "Unsupported" => ExtractionStatus::Unsupported,
                _ => return Err(invalid("invalid stored extraction status")),
            };
            let size: String = row.get(5).map_err(db)?;
            items.push(RecordHeader {
                id: row.get(0).map_err(db)?,
                ordinal: row.get(1).map_err(db)?,
                path: row.get(2).map_err(db)?,
                status,
                mime: row.get(4).map_err(db)?,
                size_bytes: size.parse().map_err(|_| invalid("invalid stored size"))?,
                sha256: row.get(6).map_err(db)?,
            });
        }
        let next = if has_more {
            items.last().map(|last| PageCursor {
                case_id,
                revision,
                query: fingerprint,
                position: match query.order {
                    RecordOrder::Insertion => Position::Insertion(last.ordinal as i64),
                    RecordOrder::Path => Position::Path(last.path.clone(), last.id),
                    RecordOrder::Size => {
                        Position::Size(last.size_bytes.to_be_bytes().to_vec(), last.id)
                    }
                },
            })
        } else {
            None
        };
        Ok(Page { items, next })
    }
    pub fn count_records(&self, query: &RecordQuery) -> Result<u64> {
        query.validate()?;
        let (sql, values) = selection(query);
        let predicate = sql
            .split_once(" FROM records r WHERE ")
            .ok_or_else(|| invalid("invalid internal record query"))?
            .1;
        self.lock()?
            .query_row(
                &format!("SELECT count(*) FROM records r WHERE {predicate}"),
                params_from_iter(values),
                |r| r.get(0),
            )
            .map_err(db)
    }
}

fn selection(query: &RecordQuery) -> (String, Vec<Value>) {
    let mut sql = String::from(
        "SELECT r.id,r.ordinal,r.path,r.status,r.mime,r.size_bytes,r.sha256,length(CAST(r.path AS BLOB))+length(CAST(r.mime AS BLOB))+coalesce(length(r.sha256),0) FROM records r WHERE r.scan_id=?",
    );
    let mut values = vec![Value::Text(query.scan_id.clone())];
    if let Some(status) = query.status {
        sql.push_str(" AND r.status=?");
        values.push(Value::Text(format!("{status:?}")));
    }
    if let Some(mime) = &query.mime {
        sql.push_str(" AND r.mime=?");
        values.push(Value::Text(mime.clone()));
    }
    if let Some(prefix) = &query.path_prefix {
        sql.push_str(" AND r.path>=?");
        values.push(Value::Text(prefix.clone()));
        if let Some(end) = prefix_end(prefix) {
            sql.push_str(" AND r.path<?");
            values.push(Value::Text(end));
        }
    }
    if let Some(key) = &query.claim_key {
        sql.push_str(
            " AND EXISTS(SELECT 1 FROM normalized_claims c WHERE c.record_id=r.id AND c.key=?",
        );
        values.push(Value::Text(key.as_str().into()));
        if let Some(value) = &query.claim_text {
            sql.push_str(" AND c.value_text=?");
            values.push(Value::Text(value.clone()));
        }
        sql.push(')');
    }
    if let Some(conflicts) = query.has_conflicts {
        sql.push_str(if conflicts {
            " AND EXISTS(SELECT 1 FROM conflicts c WHERE c.record_id=r.id)"
        } else {
            " AND NOT EXISTS(SELECT 1 FROM conflicts c WHERE c.record_id=r.id)"
        });
    }
    if let Some(min) = query.min_size {
        sql.push_str(" AND r.size_key>=?");
        values.push(Value::Blob(min.to_be_bytes().to_vec()));
    }
    if let Some(max) = query.max_size {
        sql.push_str(" AND r.size_key<=?");
        values.push(Value::Blob(max.to_be_bytes().to_vec()));
    }
    (sql, values)
}
fn prefix_end(value: &str) -> Option<String> {
    let mut chars: Vec<char> = value.chars().collect();
    while let Some(last) = chars.pop() {
        let next = last as u32 + 1;
        let next = if next == 0xd800 { 0xe000 } else { next };
        if let Some(next) = char::from_u32(next) {
            chars.push(next);
            return Some(chars.into_iter().collect());
        }
    }
    None
}
