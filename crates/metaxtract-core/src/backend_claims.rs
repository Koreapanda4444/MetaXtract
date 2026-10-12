use metaxtract_schema::{
    AgreementLevel, CanonicalKey, ClaimId, ClaimResolution, Confidence, ConfidenceBasis,
    ExtractionMethod, MetadataObservation, NormalizedClaim, NormalizedValue, RawValue, TagCode,
};

#[derive(Clone, Copy, Debug)]
pub struct BackendClaimLimits {
    pub max_claims: usize,
    pub max_value_nodes: usize,
    pub max_value_depth: usize,
    pub max_text_bytes: usize,
}

impl Default for BackendClaimLimits {
    fn default() -> Self {
        Self {
            max_claims: 100_000,
            max_value_nodes: 500_000,
            max_value_depth: 16,
            max_text_bytes: 16 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendClaimLimit {
    ClaimCount,
    ValueNodes,
    ValueDepth,
    TextBytes,
}

#[derive(Clone, Debug)]
pub struct BackendClaims {
    pub claims: Vec<NormalizedClaim>,
    pub stopped: Option<BackendClaimLimit>,
}

struct Budget {
    limits: BackendClaimLimits,
    nodes: usize,
    text: usize,
}

impl Budget {
    fn value(
        &mut self,
        raw: &RawValue,
        depth: usize,
    ) -> Result<Option<NormalizedValue>, BackendClaimLimit> {
        if depth > self.limits.max_value_depth {
            return Err(BackendClaimLimit::ValueDepth);
        }
        self.nodes = self.nodes.saturating_add(1);
        if self.nodes > self.limits.max_value_nodes {
            return Err(BackendClaimLimit::ValueNodes);
        }
        Ok(Some(match raw {
            RawValue::Null => NormalizedValue::Null,
            RawValue::Bool(value) => NormalizedValue::Bool(*value),
            RawValue::Signed(value) => NormalizedValue::Signed(*value),
            RawValue::Unsigned(value) => NormalizedValue::Unsigned(*value),
            RawValue::Float(value) if value.is_finite() => NormalizedValue::Number(*value),
            RawValue::Text(value) if value.lossless => {
                self.text = self.text.saturating_add(value.text.len());
                if self.text > self.limits.max_text_bytes {
                    return Err(BackendClaimLimit::TextBytes);
                }
                NormalizedValue::Text(value.text.clone())
            }
            RawValue::Sequence(values) => {
                if values.len() > self.limits.max_value_nodes.saturating_sub(self.nodes) {
                    return Err(BackendClaimLimit::ValueNodes);
                }
                let mut normalized = Vec::with_capacity(values.len());
                for value in values {
                    let Some(value) = self.value(value, depth + 1)? else {
                        return Ok(None);
                    };
                    normalized.push(value);
                }
                NormalizedValue::Sequence(normalized)
            }
            _ => return Ok(None),
        }))
    }
}

fn segment(text: &str) -> String {
    let mut value = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_lowercase() || byte.is_ascii_digit() {
            value.push(char::from(byte));
        } else {
            use std::fmt::Write;
            let _ = write!(value, "_{byte:02x}");
        }
    }
    if value.is_empty() {
        value.push_str("_empty");
    }
    value
}

fn key(observation: &MetadataObservation) -> CanonicalKey {
    let name = observation
        .tag
        .name
        .clone()
        .unwrap_or_else(|| match &observation.tag.code {
            TagCode::Numeric(value) => format!("tag_{value:x}"),
            TagCode::FourCc(value) => value.iter().map(|byte| format!("{byte:02x}")).collect(),
            TagCode::Uuid(value) => value.iter().map(|byte| format!("{byte:02x}")).collect(),
            TagCode::Text(value) => value.clone(),
        });
    let namespace = observation
        .tag
        .namespace
        .split('.')
        .map(segment)
        .collect::<Vec<_>>()
        .join(".");
    CanonicalKey::parse(format!("raw.{namespace}.{}", segment(&name)))
        .expect("escaped raw tag keys are valid")
}

pub fn independent_backend_claims(
    observations: &[MetadataObservation],
    limits: BackendClaimLimits,
) -> BackendClaims {
    let mut budget = Budget {
        limits,
        nodes: 0,
        text: 0,
    };
    let mut output = BackendClaims {
        claims: Vec::new(),
        stopped: None,
    };
    for observation in observations {
        if observation.tag.namespace.starts_with("diagnostic.") {
            continue;
        }
        // Unsupported/lossy interpretations stay in observations without a speculative claim.
        let Some(raw) = observation.interpreted_value.as_ref() else {
            continue;
        };
        let value = match budget.value(raw, 0) {
            Ok(Some(value)) => value,
            Ok(None) => continue,
            Err(code) => {
                output.stopped = Some(code);
                break;
            }
        };
        if output.claims.len() >= limits.max_claims {
            output.stopped = Some(BackendClaimLimit::ClaimCount);
            break;
        }
        let key_bytes = observation
            .tag
            .namespace
            .len()
            .saturating_add(observation.tag.name.as_ref().map_or(64, String::len))
            .saturating_mul(3)
            .saturating_add(16);
        budget.text = budget.text.saturating_add(key_bytes);
        if budget.text > limits.max_text_bytes {
            output.stopped = Some(BackendClaimLimit::TextBytes);
            break;
        }
        let (score, basis) = match observation.provenance.method {
            ExtractionMethod::Structure => (850, ConfidenceBasis::StructuralValidation),
            ExtractionMethod::Heuristic => (250, ConfidenceBasis::Heuristic),
            _ => (650, ConfidenceBasis::DirectTag),
        };
        output.claims.push(NormalizedClaim {
            id: ClaimId::new(output.claims.len() as u64),
            key: key(observation),
            value,
            supporting_observations: vec![observation.id],
            contradicting_observations: Vec::new(),
            supporting_parsers: vec![observation.provenance.parser.id.clone()],
            dissenting_parsers: Vec::new(),
            confidence: Confidence::new(score, vec![basis])
                .expect("fixed interpretation scores are within range"),
            agreement: AgreementLevel::SingleParser,
            resolution: ClaimResolution::Unresolved,
        });
    }
    output
}
