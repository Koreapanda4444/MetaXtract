#![forbid(unsafe_code)]

pub mod findings;
pub mod normalize;
pub mod provenance;
pub mod timestamp;
pub mod validation;

pub use metaxtract_schema as schema;
use std::io;

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct AnalysisLimits {
    pub max_records: usize,
    pub max_fields: usize,
    pub max_results: usize,
    pub max_comparisons: usize,
    pub max_bytes: usize,
}
impl Default for AnalysisLimits {
    fn default() -> Self {
        Self {
            max_records: 100_000,
            max_fields: 100_000,
            max_results: 100_000,
            max_comparisons: 5_000_000,
            max_bytes: 256 * 1024 * 1024,
        }
    }
}
pub(crate) fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|v| format!("{v:02x}")).collect()
}
pub(crate) fn check_record(
    record: &schema::ExtractionRecord,
    limits: AnalysisLimits,
) -> io::Result<()> {
    if record
        .observations
        .len()
        .saturating_add(record.claims.len())
        > limits.max_fields
    {
        return Err(invalid("analysis field budget exceeded"));
    }
    let mut stack: Vec<_> = record
        .observations
        .iter()
        .flat_map(|o| {
            std::iter::once((&o.raw_value, 0usize))
                .chain(o.interpreted_value.as_ref().map(|v| (v, 0)))
        })
        .collect();
    let mut fields = 0usize;
    while let Some((value, depth)) = stack.pop() {
        fields = fields.saturating_add(1);
        if depth > 64 || fields > limits.max_fields {
            return Err(invalid("raw metadata depth or field budget exceeded"));
        }
        match value {
            schema::RawValue::Sequence(v) => stack.extend(v.iter().map(|v| (v, depth + 1))),
            schema::RawValue::Structure(v) => stack.extend(v.iter().map(|v| (&v.value, depth + 1))),
            _ => {}
        }
    }
    let mut stack: Vec<_> = record.claims.iter().map(|c| (&c.value, 0usize)).collect();
    while let Some((value, depth)) = stack.pop() {
        fields = fields.saturating_add(1);
        if depth > 64 || fields > limits.max_fields {
            return Err(invalid(
                "normalized metadata depth or field budget exceeded",
            ));
        }
        if let schema::NormalizedValue::Sequence(v) = value {
            stack.extend(v.iter().map(|v| (v, depth + 1)));
        }
    }
    serde_json::to_writer(LimitedWriter::new(io::sink(), limits.max_bytes), record)
        .map_err(io::Error::other)?;
    let mut ids = std::collections::BTreeSet::new();
    for observation in &record.observations {
        if !ids.insert(observation.id) || observation.provenance.source != record.source.path {
            return Err(invalid("duplicate observation ID or mismatched source"));
        }
    }
    let mut claims = std::collections::BTreeSet::new();
    for claim in &record.claims {
        if !claims.insert(claim.id)
            || claim
                .supporting_observations
                .iter()
                .chain(&claim.contradicting_observations)
                .any(|id| !ids.contains(id))
        {
            return Err(invalid("duplicate claim ID or dangling observation link"));
        }
    }
    Ok(())
}

pub(crate) struct LimitedWriter<W> {
    inner: W,
    remaining: usize,
}
impl<W> LimitedWriter<W> {
    pub(crate) fn new(inner: W, limit: usize) -> Self {
        Self {
            inner,
            remaining: limit,
        }
    }
}
impl<W: io::Write> io::Write for LimitedWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.remaining {
            return Err(invalid("analysis byte budget exceeded"));
        }
        let n = self.inner.write(bytes)?;
        self.remaining -= n;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
