use crate::{
    CaseWorkspace, Page, PageCursor, RecordHeader, RecordQuery, Result, codec, db, invalid, text,
};
use metaxtract_schema::{CanonicalKey, NormalizedClaim, NormalizedValue};
use rusqlite::{
    Connection,
    functions::FunctionFlags,
    params_from_iter,
    types::{Value, ValueRef},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchQuery {
    pub records: RecordQuery,
    pub expression: SearchExpression,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SearchExpression {
    All(Vec<SearchExpression>),
    Any(Vec<SearchExpression>),
    Not(Box<SearchExpression>),
    Predicate(SearchPredicate),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchPredicate {
    pub field: SearchField,
    pub operator: SearchOperator,
    pub value: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SearchField {
    Path,
    Mime,
    Status,
    Sha256,
    Size,
    Claim(CanonicalKey),
    Note,
    Tag,
    Namespace,
    Conflicts,
    Text,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum SearchOperator {
    Equal,
    NotEqual,
    Contains,
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
    Exists,
}
impl SearchOperator {
    fn sql(self) -> Result<&'static str> {
        match self {
            Self::Equal => Ok("="),
            Self::NotEqual => Ok("<>"),
            Self::Greater => Ok(">"),
            Self::GreaterEqual => Ok(">="),
            Self::Less => Ok("<"),
            Self::LessEqual => Ok("<="),
            _ => Err(invalid("operator requires a supported field")),
        }
    }
}
impl SearchQuery {
    pub fn new(scan_id: impl Into<String>, expression: SearchExpression) -> Self {
        Self {
            records: RecordQuery::new(scan_id),
            expression,
        }
    }
    pub fn validate(&self) -> Result<()> {
        self.records.validate()?;
        let mut stack = vec![(&self.expression, 0usize)];
        let mut nodes = 0;
        while let Some((expression, depth)) = stack.pop() {
            nodes += 1;
            if nodes > 128 || depth > 8 {
                return Err(invalid("search expression exceeds node or depth limit"));
            }
            match expression {
                SearchExpression::All(v) | SearchExpression::Any(v) => {
                    if v.len() > 64 {
                        return Err(invalid("search group exceeds 64 terms"));
                    }
                    stack.extend(v.iter().map(|v| (v, depth + 1)));
                }
                SearchExpression::Not(v) => stack.push((v, depth + 1)),
                SearchExpression::Predicate(p) => text(&p.value, 4096, true)?,
            }
        }
        let mut sql = String::new();
        let mut values = vec![];
        append_expression(&self.expression, &mut sql, &mut values)?;
        Ok(())
    }
}
pub(crate) fn append_expression(
    expression: &SearchExpression,
    sql: &mut String,
    values: &mut Vec<Value>,
) -> Result<()> {
    sql.push_str(" AND ");
    compile(expression, sql, values)
}
fn compile(expression: &SearchExpression, sql: &mut String, values: &mut Vec<Value>) -> Result<()> {
    match expression {
        SearchExpression::All(v) | SearchExpression::Any(v) => {
            sql.push('(');
            if v.is_empty() {
                sql.push_str(if matches!(expression, SearchExpression::All(_)) {
                    "1"
                } else {
                    "0"
                });
            }
            for (i, e) in v.iter().enumerate() {
                if i > 0 {
                    sql.push_str(if matches!(expression, SearchExpression::All(_)) {
                        " AND "
                    } else {
                        " OR "
                    });
                }
                compile(e, sql, values)?;
            }
            sql.push(')');
        }
        SearchExpression::Not(e) => {
            sql.push_str("NOT (");
            compile(e, sql, values)?;
            sql.push(')');
        }
        SearchExpression::Predicate(p) => {
            let string_predicate =
                |column: &str, sql: &mut String, values: &mut Vec<Value>| -> Result<()> {
                    if matches!(p.operator, SearchOperator::Exists) {
                        sql.push_str(&format!("{column} IS NOT NULL"));
                    } else if matches!(p.operator, SearchOperator::Contains) {
                        sql.push_str(&format!("instr({column},?)>0"));
                        values.push(Value::Text(p.value.clone()));
                    } else {
                        sql.push_str(&format!("{column} {} ?", p.operator.sql()?));
                        values.push(Value::Text(p.value.clone()));
                    }
                    Ok(())
                };
            match &p.field {
                SearchField::Path => string_predicate("r.path", sql, values)?,
                SearchField::Mime => string_predicate("r.mime", sql, values)?,
                SearchField::Status => string_predicate("r.status", sql, values)?,
                SearchField::Sha256 => string_predicate("r.sha256", sql, values)?,
                SearchField::Size => {
                    let size: u64 = p
                        .value
                        .parse()
                        .map_err(|_| invalid("size query requires a u64 integer"))?;
                    sql.push_str(&format!("r.size_key {} ?", p.operator.sql()?));
                    values.push(Value::Blob(size.to_be_bytes().to_vec()));
                }
                SearchField::Claim(key) => {
                    if matches!(p.operator, SearchOperator::NotEqual) {
                        sql.push_str("NOT ");
                    }
                    sql.push_str("EXISTS(SELECT 1 FROM normalized_claims c WHERE c.record_id=r.id AND c.key=?");
                    values.push(Value::Text(key.to_string()));
                    if !matches!(p.operator, SearchOperator::Exists) {
                        sql.push_str(" AND mx_case_claim_match(c.data,c.digest,?,?)=1");
                        values.push(Value::Text(
                            if matches!(p.operator, SearchOperator::NotEqual) {
                                "Equal".into()
                            } else {
                                format!("{:?}", p.operator)
                            },
                        ));
                        values.push(Value::Text(p.value.clone()));
                    }
                    sql.push(')');
                }
                SearchField::Note => {
                    sql.push_str("EXISTS(SELECT 1 FROM annotations a WHERE a.record_id=r.id AND ");
                    string_predicate("a.note", sql, values)?;
                    sql.push(')');
                }
                SearchField::Tag => {
                    sql.push_str("EXISTS(SELECT 1 FROM annotations a,json_each(CAST(a.tags AS TEXT)) j WHERE a.record_id=r.id AND ");
                    string_predicate("j.value", sql, values)?;
                    sql.push(')');
                }
                SearchField::Namespace => {
                    sql.push_str("EXISTS(SELECT 1 FROM raw_fields f WHERE f.record_id=r.id AND ");
                    string_predicate("f.namespace", sql, values)?;
                    sql.push(')');
                }
                SearchField::Conflicts => {
                    if !matches!(p.operator, SearchOperator::Equal | SearchOperator::NotEqual) {
                        return Err(invalid("conflicts requires equality to true/false"));
                    }
                    let expected: bool = p
                        .value
                        .parse()
                        .map_err(|_| invalid("conflicts requires true/false"))?;
                    if expected == matches!(p.operator, SearchOperator::NotEqual) {
                        sql.push_str("NOT ");
                    }
                    sql.push_str("EXISTS(SELECT 1 FROM conflicts c WHERE c.record_id=r.id)");
                }
                SearchField::Text => {
                    if !matches!(p.operator, SearchOperator::Contains | SearchOperator::Equal) {
                        return Err(invalid("free text supports contains or equality"));
                    }
                    sql.push_str("(instr(r.path,?)>0 OR instr(r.mime,?)>0 OR EXISTS(SELECT 1 FROM normalized_claims c WHERE c.record_id=r.id AND mx_case_claim_match(c.data,c.digest,'Contains',?)=1) OR EXISTS(SELECT 1 FROM annotations a WHERE a.record_id=r.id AND instr(a.note,?)>0))");
                    for _ in 0..4 {
                        values.push(Value::Text(p.value.clone()));
                    }
                }
            }
        }
    }
    Ok(())
}
fn compare_order<T: PartialOrd + PartialEq>(a: T, b: T, op: &str) -> bool {
    match op {
        "Equal" => a == b,
        "NotEqual" => a != b,
        "Greater" => a > b,
        "GreaterEqual" => a >= b,
        "Less" => a < b,
        "LessEqual" => a <= b,
        _ => false,
    }
}
fn matches_value(value: &NormalizedValue, op: &str, expected: &str) -> Result<bool> {
    let mut stack = vec![(value, 0usize)];
    let mut nodes = 0;
    while let Some((value, depth)) = stack.pop() {
        nodes += 1;
        if nodes > 100_000 || depth > 64 {
            return Err(invalid("claim search value exceeds structural limit"));
        }
        let matches = match value {
            NormalizedValue::Sequence(v) => {
                stack.extend(v.iter().map(|v| (v, depth + 1)));
                false
            }
            NormalizedValue::Text(v) => {
                if op == "Contains" {
                    v.contains(expected)
                } else {
                    compare_order(v.as_str(), expected, op)
                }
            }
            NormalizedValue::Signed(v) => expected
                .parse::<i128>()
                .is_ok_and(|n| compare_order(i128::from(*v), n, op)),
            NormalizedValue::Unsigned(v) => expected
                .parse::<i128>()
                .is_ok_and(|n| compare_order(i128::from(*v), n, op)),
            NormalizedValue::DurationNanoseconds(v) => expected
                .parse::<i128>()
                .is_ok_and(|n| compare_order(*v, n, op)),
            NormalizedValue::Number(v) => {
                v.is_finite()
                    && expected
                        .parse::<f64>()
                        .is_ok_and(|n| n.is_finite() && compare_order(*v, n, op))
            }
            NormalizedValue::Bool(v) => expected
                .parse::<bool>()
                .is_ok_and(|n| compare_order(*v, n, op)),
            NormalizedValue::Null => compare_order(expected == "null", true, op),
            NormalizedValue::Timestamp(t) => {
                if let Some(epoch) = expected.strip_prefix("unix:") {
                    epoch
                        .parse::<i64>()
                        .ok()
                        .zip(t.unix_seconds)
                        .is_some_and(|(b, a)| compare_order(a, b, op))
                } else if op == "Contains" {
                    t.original.contains(expected)
                } else if matches!(op, "Equal" | "NotEqual") {
                    compare_order(t.original.as_str(), expected, op)
                } else {
                    return Err(invalid("timestamp ranges require unix:<seconds>"));
                }
            }
            NormalizedValue::GeoPoint(_) => false,
        };
        if matches {
            return Ok(true);
        }
    }
    Ok(false)
}
pub(crate) fn register(conn: &Connection) -> Result<()> {
    conn.create_scalar_function(
        "mx_case_claim_match",
        4,
        FunctionFlags::SQLITE_UTF8
            | FunctionFlags::SQLITE_DETERMINISTIC
            | FunctionFlags::SQLITE_INNOCUOUS,
        |ctx| {
            let error =
                |message: &str| rusqlite::Error::UserFunctionError(Box::new(invalid(message)));
            let bytes = match ctx.get_raw(0) {
                ValueRef::Blob(v) => v,
                _ => return Err(error("invalid claim data")),
            };
            let digest = match ctx.get_raw(1) {
                ValueRef::Blob(v) => v,
                _ => return Err(error("invalid claim digest")),
            };
            if bytes.len() > codec::MAX_RECORD_BYTES || Sha256::digest(bytes).as_slice() != digest {
                return Err(error("claim checksum mismatch or size limit exceeded"));
            }
            let c: NormalizedClaim = codec::decode(bytes, codec::MAX_RECORD_BYTES)
                .map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))?;
            let op: String = ctx.get(2)?;
            let expected: String = ctx.get(3)?;
            matches_value(&c.value, &op, &expected)
                .map(i32::from)
                .map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))
        },
    )
    .map_err(db)
}
#[derive(Clone, Copy, Debug)]
pub enum FacetField {
    Mime,
    Status,
    ClaimKey,
    Tag,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FacetValue {
    pub value: String,
    pub records: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FacetPage {
    pub values: Vec<FacetValue>,
    pub more: bool,
}
impl CaseWorkspace {
    pub fn search_records(
        &self,
        query: &SearchQuery,
        cursor: Option<&PageCursor>,
        limit: usize,
    ) -> Result<Page<RecordHeader>> {
        query.validate()?;
        self.query_records_with(&query.records, Some(query), cursor, limit)
    }
    pub fn count_search(&self, query: &SearchQuery) -> Result<u64> {
        query.validate()?;
        let (mut sql, mut values) = crate::query::selection(&query.records);
        append_expression(&query.expression, &mut sql, &mut values)?;
        let (_, predicate) = sql
            .split_once(" FROM records r WHERE ")
            .ok_or_else(|| invalid("invalid internal search"))?;
        self.lock()?
            .query_row(
                &format!("SELECT count(*) FROM records r WHERE {predicate}"),
                params_from_iter(values),
                |r| r.get(0),
            )
            .map_err(db)
    }
    pub fn search_facets(
        &self,
        query: &SearchQuery,
        field: FacetField,
        limit: usize,
    ) -> Result<FacetPage> {
        query.validate()?;
        if !(1..=256).contains(&limit) {
            return Err(invalid("facet page must contain 1..256 values"));
        }
        let (mut sql, mut values) = crate::query::selection(&query.records);
        append_expression(&query.expression, &mut sql, &mut values)?;
        let (_, predicate) = sql
            .split_once(" FROM records r WHERE ")
            .ok_or_else(|| invalid("invalid internal facet"))?;
        let (column, join) = match field {
            FacetField::Mime => ("r.mime", ""),
            FacetField::Status => ("r.status", ""),
            FacetField::ClaimKey => ("c.key", " JOIN normalized_claims c ON c.record_id=r.id"),
            FacetField::Tag => (
                "j.value",
                " JOIN annotations a ON a.record_id=r.id JOIN json_each(CAST(a.tags AS TEXT)) j",
            ),
        };
        let conn = self.lock()?;
        let mut q=conn.prepare(&format!("SELECT {column},count(DISTINCT r.id) FROM records r{join} WHERE {predicate} GROUP BY {column} ORDER BY count(DISTINCT r.id) DESC,{column} LIMIT ?")).map_err(db)?;
        values.push(Value::Integer((limit + 1) as i64));
        let mut rows = q.query(params_from_iter(values)).map_err(db)?;
        let mut items = Vec::new();
        let mut bytes = 0usize;
        while let Some(row) = rows.next().map_err(db)? {
            let value: String = row.get(0).map_err(db)?;
            bytes = bytes.saturating_add(value.len());
            if bytes > 16 * 1024 * 1024 {
                return Err(invalid("facet byte budget exceeded"));
            }
            items.push(FacetValue {
                value,
                records: row.get(1).map_err(db)?,
            });
        }
        let more = items.len() > limit;
        items.truncate(limit);
        Ok(FacetPage {
            values: items,
            more,
        })
    }
}
