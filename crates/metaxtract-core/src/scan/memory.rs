use super::{ScanControl, walk::invalid};
use crate::schema::{EvidencePath, ExtractionRecord, NormalizedValue, RawValue, TagCode};
use std::{
    io,
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};
pub(crate) struct MemoryBudget {
    max: u64,
    used: Mutex<u64>,
    wake: Condvar,
}
pub(crate) struct MemoryLease {
    budget: Arc<MemoryBudget>,
    bytes: u64,
}
impl MemoryBudget {
    pub(crate) fn new(max: u64) -> Arc<Self> {
        Arc::new(Self {
            max,
            used: Mutex::new(0),
            wake: Condvar::new(),
        })
    }
    pub(crate) fn acquire(
        self: &Arc<Self>,
        bytes: u64,
        control: &ScanControl,
    ) -> io::Result<MemoryLease> {
        if bytes > self.max {
            return Err(invalid("per-file reservation exceeds memory limit"));
        }
        let mut used = self
            .used
            .lock()
            .map_err(|_| io::Error::other("memory lock poisoned"))?;
        loop {
            control.checkpoint()?;
            if self.max.saturating_sub(*used) >= bytes {
                *used += bytes;
                return Ok(MemoryLease {
                    budget: Arc::clone(self),
                    bytes,
                });
            }
            used = self
                .wake
                .wait_timeout(used, Duration::from_millis(25))
                .map_err(|_| io::Error::other("memory lock poisoned"))?
                .0;
        }
    }
}
impl Drop for MemoryLease {
    fn drop(&mut self) {
        if let Ok(mut used) = self.budget.used.lock() {
            *used = used.saturating_sub(self.bytes);
            self.budget.wake.notify_all();
        }
    }
}
fn path_bytes(p: &EvidencePath) -> u64 {
    p.nested()
        .iter()
        .fold(p.root_path().as_str().len() as u64 + 64, |n, s| {
            n.saturating_add(s.path.as_str().len() as u64 + 64)
        })
}
fn raw(v: &RawValue, depth: u32) -> u64 {
    if depth > 64 {
        return u64::MAX;
    }
    match v {
        RawValue::Text(t) => 64_u64
            .saturating_add(t.text.len() as u64)
            .saturating_add(t.encoded_bytes.as_ref().map_or(0, |v| v.len() as u64))
            .saturating_add(t.original_encoding.as_ref().map_or(0, |v| v.len() as u64))
            .saturating_add(t.language.as_ref().map_or(0, |v| v.len() as u64)),
        RawValue::Bytes(b) => 64_u64.saturating_add(b.len() as u64),
        RawValue::Sequence(v) => v
            .iter()
            .fold(64_u64, |n, v| n.saturating_add(raw(v, depth + 1))),
        RawValue::Structure(v) => v.iter().fold(64_u64, |n, f| {
            n.saturating_add(f.name.len() as u64 + 64)
                .saturating_add(raw(&f.value, depth + 1))
        }),
        _ => 64,
    }
}
fn normalized(v: &NormalizedValue, depth: u32) -> u64 {
    if depth > 64 {
        return u64::MAX;
    }
    match v {
        NormalizedValue::Text(t) => 64_u64.saturating_add(t.len() as u64),
        NormalizedValue::Timestamp(t) => 64_u64.saturating_add(t.original.len() as u64),
        NormalizedValue::Sequence(v) => v
            .iter()
            .fold(64_u64, |n, v| n.saturating_add(normalized(v, depth + 1))),
        _ => 64,
    }
}
fn observation(o: &crate::schema::MetadataObservation) -> u64 {
    let strings = o.tag.namespace.len()
        + o.tag.name.as_ref().map_or(0, String::len)
        + o.unit.as_ref().map_or(0, String::len)
        + o.provenance.parser.id.len()
        + o.provenance.parser.name.len()
        + o.provenance.parser.version.len()
        + o.provenance.stream_id.as_ref().map_or(0, String::len);
    let tag = match &o.tag.code {
        TagCode::Text(t) => t.len() as u64,
        _ => 32,
    };
    256_u64
        .saturating_add(strings as u64)
        .saturating_add(tag)
        .saturating_add(path_bytes(&o.provenance.source))
        .saturating_add(raw(&o.raw_value, 0))
        .saturating_add(o.interpreted_value.as_ref().map_or(0, |v| raw(v, 0)))
}
pub(crate) fn record_bytes(r: &ExtractionRecord) -> u64 {
    let mut n = 512_u64.saturating_add(path_bytes(&r.source.path));
    for o in &r.observations {
        n = n.saturating_add(observation(o));
    }
    for c in &r.claims {
        n = n
            .saturating_add(256 + c.key.as_str().len() as u64)
            .saturating_add(normalized(&c.value, 0))
            .saturating_add(
                (c.supporting_observations.len() as u64
                    + c.contradicting_observations.len() as u64)
                    * 16,
            );
        for p in c.supporting_parsers.iter().chain(&c.dissenting_parsers) {
            n = n.saturating_add(p.len() as u64 + 32);
        }
    }
    for c in &r.conflicts {
        n = n.saturating_add(
            128 + c.key.as_str().len() as u64
                + c.reason.len() as u64
                + (c.alternatives.len() as u64) * 16,
        );
    }
    for d in &r.diagnostics {
        n = n
            .saturating_add(256 + d.code.len() as u64 + d.message.len() as u64)
            .saturating_add(path_bytes(&d.source));
    }
    for c in &r.containers {
        n = n.saturating_add(c.format.len() as u64 + 128);
        for s in &c.streams {
            n = n.saturating_add(
                128 + s.id.len() as u64
                    + s.label.as_ref().map_or(0, |s| s.len() as u64)
                    + s.media_type.as_ref().map_or(0, |s| s.len() as u64),
            );
        }
    }
    n
}
pub(crate) fn bound_records(records: &mut Vec<ExtractionRecord>, max: u64) -> bool {
    let mut limited = false;
    if let Some(root) = records.first_mut() {
        if record_bytes(root) > max {
            limited = true;
            root.claims.clear();
            root.conflicts.clear();
            root.containers.clear();
            root.diagnostics.clear();
            let mut used = 512_u64.saturating_add(path_bytes(&root.source.path));
            let mut full = false;
            root.observations.retain(|o| {
                let cost = observation(o);
                if full || used.saturating_add(cost) > max {
                    full = true;
                    false
                } else {
                    used += cost;
                    true
                }
            });
        }
    }
    let mut used = 0_u64;
    let mut full = false;
    records.retain(|r| {
        let cost = record_bytes(r);
        if full || used.saturating_add(cost) > max {
            full = true;
            limited = true;
            false
        } else {
            used += cost;
            true
        }
    });
    !limited
}
