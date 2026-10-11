use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::ObservationId;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CanonicalKey(String);

impl CanonicalKey {
    pub fn parse(value: impl Into<String>) -> Result<Self, KeyError> {
        let value = value.into();
        if value.is_empty() {
            return Err(KeyError::Empty);
        }
        for segment in value.split('.') {
            if segment.is_empty() {
                return Err(KeyError::EmptySegment);
            }
            if !segment
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
            {
                return Err(KeyError::InvalidCharacter);
            }
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for CanonicalKey {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyError {
    Empty,
    EmptySegment,
    InvalidCharacter,
}

impl Display for KeyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Empty => "canonical key is empty",
            Self::EmptySegment => "canonical key contains an empty segment",
            Self::InvalidCharacter => "canonical key contains an invalid character",
        };
        formatter.write_str(message)
    }
}

impl Error for KeyError {}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ClaimId(u64);

impl ClaimId {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TimestampPrecision {
    Year,
    Month,
    Day,
    Minute,
    Second,
    FractionalSecond,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimestampValue {
    pub original: String,
    pub unix_seconds: Option<i64>,
    pub nanosecond: Option<u32>,
    pub utc_offset_minutes: Option<i16>,
    pub precision: TimestampPrecision,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeoPoint {
    pub latitude: f64,
    pub longitude: f64,
    pub altitude_meters: Option<f64>,
    pub horizontal_accuracy_meters: Option<f64>,
}

impl GeoPoint {
    pub fn is_valid(self) -> bool {
        self.latitude.is_finite()
            && self.longitude.is_finite()
            && (-90.0..=90.0).contains(&self.latitude)
            && (-180.0..=180.0).contains(&self.longitude)
            && self.altitude_meters.is_none_or(f64::is_finite)
            && self
                .horizontal_accuracy_meters
                .is_none_or(|value| value.is_finite() && value >= 0.0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum NormalizedValue {
    Null,
    Bool(bool),
    Signed(i64),
    Unsigned(u64),
    Number(f64),
    Text(String),
    Timestamp(TimestampValue),
    DurationNanoseconds(i128),
    GeoPoint(GeoPoint),
    Sequence(Vec<NormalizedValue>),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ConfidenceBasis {
    DirectTag,
    StructuralValidation,
    IndependentParserAgreement,
    CrossFieldConsistency,
    Heuristic,
    UserOverride,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Confidence {
    score_millis: u16,
    pub basis: Vec<ConfidenceBasis>,
}

impl Confidence {
    pub fn new(score_millis: u16, basis: Vec<ConfidenceBasis>) -> Option<Self> {
        if score_millis > 1_000 {
            return None;
        }
        Some(Self {
            score_millis,
            basis,
        })
    }

    pub fn score_millis(&self) -> u16 {
        self.score_millis
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AgreementLevel {
    SingleParser,
    Corroborated,
    Disputed,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ClaimResolution {
    Accepted,
    Unresolved,
    Rejected,
    UserSelected,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NormalizedClaim {
    pub id: ClaimId,
    pub key: CanonicalKey,
    pub value: NormalizedValue,
    pub supporting_observations: Vec<ObservationId>,
    pub contradicting_observations: Vec<ObservationId>,
    pub supporting_parsers: Vec<String>,
    pub dissenting_parsers: Vec<String>,
    pub confidence: Confidence,
    pub agreement: AgreementLevel,
    pub resolution: ClaimResolution,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaimConflict {
    pub key: CanonicalKey,
    pub alternatives: Vec<ClaimId>,
    pub reason: String,
}
