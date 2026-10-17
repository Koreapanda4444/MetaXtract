use crate::{
    AnalysisLimits, check_record, hex, invalid,
    normalize::{field, number, raw, raw_text},
    schema::*,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Read},
};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ExactGroup {
    pub algorithm: DigestAlgorithm,
    pub digest: String,
    pub size_bytes: u64,
    pub paths: Vec<EvidencePath>,
}
pub fn exact(records: &[ExtractionRecord], limits: AnalysisLimits) -> io::Result<Vec<ExactGroup>> {
    if records.len() > limits.max_records {
        return Err(invalid("duplicate record budget exceeded"));
    }
    type DigestGroups = BTreeMap<(String, Vec<u8>), (DigestAlgorithm, u64, Vec<EvidencePath>)>;
    let mut groups = DigestGroups::new();
    let mut paths = BTreeSet::new();
    for record in records {
        check_record(record, limits)?;
        if !paths.insert(record.source.path.to_string()) {
            return Err(invalid(
                "duplicate evidence paths; compare one scan at a time",
            ));
        }
        for digest in &record.digests {
            if !matches!(
                digest.algorithm,
                DigestAlgorithm::Sha256 | DigestAlgorithm::Blake3
            ) {
                continue;
            }
            if digest.bytes.len() != 32 {
                return Err(invalid("invalid content digest length"));
            }
            let entry = groups
                .entry((format!("{:?}", digest.algorithm), digest.bytes.clone()))
                .or_insert_with(|| {
                    (
                        digest.algorithm.clone(),
                        record.source.identity.size_bytes,
                        vec![],
                    )
                });
            if entry.1 != record.source.identity.size_bytes {
                return Err(invalid("same digest has inconsistent evidence sizes"));
            }
            if !entry.2.contains(&record.source.path) {
                entry.2.push(record.source.path.clone());
            }
        }
    }
    let mut out = Vec::new();
    for ((_, bytes), (algorithm, size_bytes, mut paths)) in groups {
        if paths.len() > 1 {
            paths.sort_by_key(ToString::to_string);
            out.push(ExactGroup {
                algorithm,
                digest: hex(&bytes),
                size_bytes,
                paths,
            });
        }
    }
    if out.len() > limits.max_results {
        return Err(invalid("duplicate result budget exceeded"));
    }
    Ok(out)
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct ContentFingerprint {
    path: EvidencePath,
    sha256: Vec<u8>,
    size_bytes: u64,
    chunks: BTreeSet<[u8; 32]>,
}
impl ContentFingerprint {
    pub fn path(&self) -> &EvidencePath {
        &self.path
    }
    pub(crate) fn matches_record(&self, record: &ExtractionRecord) -> bool {
        self.path == record.source.path
            && self.size_bytes == record.source.identity.size_bytes
            && record
                .digests
                .iter()
                .any(|d| d.algorithm == DigestAlgorithm::Sha256 && d.bytes == self.sha256)
    }
}
/// Rust hashes caller-opened evidence; it does not decode or invoke an external program.
pub fn fingerprint_content(
    mut source: impl Read,
    record: &ExtractionRecord,
    limits: AnalysisLimits,
) -> io::Result<ContentFingerprint> {
    check_record(record, limits)?;
    let expected = record
        .digests
        .iter()
        .find(|d| d.algorithm == DigestAlgorithm::Sha256 && d.bytes.len() == 32)
        .ok_or_else(|| invalid("content similarity requires a recorded SHA-256"))?;
    let mut hasher = Sha256::new();
    let mut chunks = BTreeSet::new();
    let mut chunk = Sha256::new();
    let mut rolling = 0u64;
    let mut length = 0usize;
    let mut total = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let n = source.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        total = total
            .checked_add(n as u64)
            .ok_or_else(|| invalid("evidence size overflow"))?;
        if total > record.source.identity.size_bytes || total > limits.max_bytes as u64 {
            return Err(invalid(
                "content fingerprint byte budget or recorded size exceeded",
            ));
        }
        hasher.update(&buffer[..n]);
        let mut begin = 0;
        for (index, byte) in buffer[..n].iter().enumerate() {
            length += 1;
            rolling = rolling
                .rotate_left(1)
                .wrapping_add(u64::from(*byte).wrapping_mul(0x9e3779b185ebca87));
            if length >= 64 && (rolling & 511 == 0 || length >= 4096) {
                chunk.update(&buffer[begin..=index]);
                begin = index + 1;
                chunks.insert(chunk.finalize_reset().into());
                length = 0;
                rolling = 0;
                if chunks.len() > limits.max_fields {
                    return Err(invalid("content fingerprint chunk budget exceeded"));
                }
            }
        }
        chunk.update(&buffer[begin..n]);
    }
    if length > 0 {
        chunks.insert(chunk.finalize().into());
    }
    let sha256 = hasher.finalize().to_vec();
    if total != record.source.identity.size_bytes || sha256 != expected.bytes {
        return Err(invalid(
            "source changed or does not match recorded evidence",
        ));
    }
    Ok(ContentFingerprint {
        path: record.source.path.clone(),
        sha256,
        size_bytes: total,
        chunks,
    })
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SimilarityMatch {
    pub first: EvidencePath,
    pub second: EvidencePath,
    pub score_millis: u16,
    pub method: String,
    pub heuristic: bool,
}
pub fn near(
    fingerprints: &[ContentFingerprint],
    threshold_millis: u16,
    limits: AnalysisLimits,
) -> io::Result<Vec<SimilarityMatch>> {
    if threshold_millis > 1000 || fingerprints.len() > limits.max_records {
        return Err(invalid("invalid near-duplicate threshold or input limit"));
    }
    let mut out = Vec::new();
    let mut comparisons = 0usize;
    for (i, a) in fingerprints.iter().enumerate() {
        for b in &fingerprints[i + 1..] {
            comparisons =
                comparisons.saturating_add(a.chunks.len().saturating_add(b.chunks.len()).max(1));
            if comparisons > limits.max_comparisons {
                return Err(invalid("content similarity comparison budget exceeded"));
            }
            if a.sha256 == b.sha256 || a.path == b.path {
                continue;
            }
            let intersection = a.chunks.intersection(&b.chunks).count();
            let union = a.chunks.len() + b.chunks.len() - intersection;
            let score = if union == 0 {
                0
            } else {
                (intersection * 1000 / union) as u16
            };
            if score >= threshold_millis {
                out.push(SimilarityMatch {
                    first: a.path.clone(),
                    second: b.path.clone(),
                    score_millis: score,
                    method: "content-defined-sha256-chunks-v1-jaccard".into(),
                    heuristic: true,
                });
            }
            if out.len() > limits.max_results {
                return Err(invalid("similarity result budget exceeded"));
            }
        }
    }
    out.sort_by(|a, b| {
        b.score_millis
            .cmp(&a.score_millis)
            .then_with(|| a.first.to_string().cmp(&b.first.to_string()))
    });
    Ok(out)
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct VisualFingerprint {
    pub path: EvidencePath,
    pub stream: Option<String>,
    pub observation: ObservationId,
    pub dhash: u64,
    pub orientation: u8,
}
/// Consume bounded RGB24 previews already decoded by C++. Orientation is an explicit caller policy.
pub fn visual_fingerprints(
    record: &ExtractionRecord,
    orientation: u8,
    limits: AnalysisLimits,
) -> io::Result<Vec<VisualFingerprint>> {
    check_record(record, limits)?;
    if !(1..=8).contains(&orientation) {
        return Err(invalid("orientation must be 1..8"));
    }
    let mut groups: BTreeMap<_, Vec<_>> = BTreeMap::new();
    for o in &record.observations {
        if field(o).starts_with("preview_") {
            groups
                .entry((
                    &o.provenance.parser.id,
                    &o.tag.namespace,
                    &o.provenance.stream_id,
                ))
                .or_default()
                .push(o);
        }
    }
    let mut out = Vec::new();
    for observations in groups.values() {
        let find = |name: &str| observations.iter().copied().find(|o| field(o) == name);
        let Some(pixels) = find("preview_pixels") else {
            continue;
        };
        let Some(RawValue::Bytes(bytes)) = Some(raw(pixels)) else {
            return Err(invalid("preview pixels are not bytes"));
        };
        if find("preview_format").and_then(|o| raw_text(raw(o))) != Some("RGB24") {
            continue;
        }
        let dim = |name: &str| -> io::Result<usize> {
            let n = find(name)
                .and_then(|o| number(raw(o)))
                .ok_or_else(|| invalid("missing preview dimension"))?;
            if !(1.0..=16384.0).contains(&n) || n.fract() != 0.0 {
                return Err(invalid("invalid preview dimension"));
            }
            Ok(n as usize)
        };
        let w = dim("preview_width")?;
        let h = dim("preview_height")?;
        if w.checked_mul(h).and_then(|n| n.checked_mul(3)) != Some(bytes.len())
            || bytes.len() > limits.max_bytes
        {
            return Err(invalid("preview length or byte budget mismatch"));
        }
        let (dw, dh) = if orientation >= 5 { (h, w) } else { (w, h) };
        let luminance = |x: usize, y: usize| -> u32 {
            let x = (x * dw / 9).min(dw - 1);
            let y = (y * dh / 8).min(dh - 1);
            let (x, y) = match orientation {
                2 => (w - 1 - x, y),
                3 => (w - 1 - x, h - 1 - y),
                4 => (x, h - 1 - y),
                5 => (y, x),
                6 => (y, h - 1 - x),
                7 => (w - 1 - y, h - 1 - x),
                8 => (w - 1 - y, x),
                _ => (x, y),
            };
            let at = (y * w + x) * 3;
            299 * u32::from(bytes[at])
                + 587 * u32::from(bytes[at + 1])
                + 114 * u32::from(bytes[at + 2])
        };
        let mut hash = 0;
        for y in 0..8 {
            for x in 0..8 {
                hash = (hash << 1) | u64::from(luminance(x, y) > luminance(x + 1, y));
            }
        }
        out.push(VisualFingerprint {
            path: record.source.path.clone(),
            stream: pixels.provenance.stream_id.clone(),
            observation: pixels.id,
            dhash: hash,
            orientation,
        });
        if out.len() > limits.max_results {
            return Err(invalid("visual fingerprint budget exceeded"));
        }
    }
    Ok(out)
}
/// Apply only an unambiguous declared orientation; unresolved transforms remain a coverage gap.
pub fn record_visual_fingerprints(
    record: &ExtractionRecord,
    limits: AnalysisLimits,
) -> io::Result<Vec<VisualFingerprint>> {
    let mut orientation = None;
    for claim in &record.claims {
        if claim.key.as_str() != "image.orientation"
            || claim.resolution == ClaimResolution::Rejected
        {
            continue;
        }
        let value = match claim.value {
            NormalizedValue::Unsigned(n) => u8::try_from(n).ok(),
            NormalizedValue::Signed(n) => u8::try_from(n).ok(),
            _ => None,
        };
        let Some(value) = value.filter(|n| (1..=8).contains(n)) else {
            return Ok(vec![]);
        };
        if orientation.is_some_and(|previous| previous != value) {
            return Ok(vec![]);
        }
        orientation = Some(value);
    }
    visual_fingerprints(record, orientation.unwrap_or(1), limits)
}
pub fn visually_similar(
    fingerprints: &[VisualFingerprint],
    threshold: u8,
    limits: AnalysisLimits,
) -> io::Result<Vec<SimilarityMatch>> {
    if threshold > 64 || fingerprints.len() > limits.max_records {
        return Err(invalid("invalid visual threshold or input limit"));
    }
    let mut out = Vec::new();
    let mut comparisons = 0usize;
    for (i, a) in fingerprints.iter().enumerate() {
        for b in &fingerprints[i + 1..] {
            comparisons += 1;
            if comparisons > limits.max_comparisons {
                return Err(invalid("visual comparison budget exceeded"));
            }
            if a.path == b.path {
                continue;
            }
            let distance = (a.dhash ^ b.dhash).count_ones();
            if distance <= u32::from(threshold) {
                out.push(SimilarityMatch {
                    first: a.path.clone(),
                    second: b.path.clone(),
                    score_millis: ((64 - distance) * 1000 / 64) as u16,
                    method: "rgb24-nearest-oriented-dhash64-v1".into(),
                    heuristic: true,
                });
            }
            if out.len() > limits.max_results {
                return Err(invalid("visual match budget exceeded"));
            }
        }
    }
    out.sort_by(|a, b| b.score_millis.cmp(&a.score_millis));
    Ok(out)
}
