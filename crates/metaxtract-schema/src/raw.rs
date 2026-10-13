use crate::Provenance;

#[derive(
    serde::Serialize, serde::Deserialize, Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd,
)]
pub struct ObservationId(u64);

impl ObservationId {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Eq, Hash, PartialEq)]
pub enum TagCode {
    Numeric(u64),
    FourCc([u8; 4]),
    Uuid([u8; 16]),
    Text(String),
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Eq, Hash, PartialEq)]
pub struct TagIdentity {
    pub namespace: String,
    pub code: TagCode,
    pub name: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct Rational {
    pub numerator: i64,
    pub denominator: i64,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct TextValue {
    pub text: String,
    pub original_encoding: Option<String>,
    pub language: Option<String>,
    pub encoded_bytes: Option<Vec<u8>>,
    pub lossless: bool,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct RawField {
    pub name: String,
    pub value: RawValue,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub enum RawValue {
    Null,
    Bool(bool),
    Signed(i64),
    Unsigned(u64),
    Float(#[serde(with = "crate::float_bits")] f64),
    Rational(Rational),
    Text(TextValue),
    Bytes(Vec<u8>),
    Sequence(Vec<RawValue>),
    Structure(Vec<RawField>),
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct MetadataObservation {
    pub id: ObservationId,
    pub tag: TagIdentity,
    pub raw_value: RawValue,
    pub interpreted_value: Option<RawValue>,
    pub unit: Option<String>,
    pub provenance: Provenance,
}
