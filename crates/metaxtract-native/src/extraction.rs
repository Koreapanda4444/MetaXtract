use crate::{
    BoundedInput, NativeError, NativeErrorCode, ParserLimits, ParserRun, build_info, ffi,
    native_error,
};
use metaxtract_schema::{
    ByteRange, EvidencePath, ExtractionMethod, MetadataObservation, ObservationId,
    ParserDescriptor, Provenance, Rational, RawValue, TagCode, TagIdentity, TextValue,
};

#[derive(Clone, Debug, PartialEq)]
pub struct NativeObservation {
    pub namespace: String,
    pub key: String,
    pub tag_code: Option<u64>,
    pub value: RawValue,
    pub encoded: Vec<u8>,
    pub source_range: Option<ByteRange>,
}

impl NativeObservation {
    pub fn to_canonical(
        &self,
        id: ObservationId,
        source: EvidencePath,
        parser: ParserDescriptor,
    ) -> MetadataObservation {
        MetadataObservation {
            id,
            tag: TagIdentity {
                namespace: self.namespace.clone(),
                code: self
                    .tag_code
                    .map(TagCode::Numeric)
                    .unwrap_or_else(|| TagCode::Text(self.key.clone())),
                name: Some(self.key.clone()),
            },
            raw_value: RawValue::Bytes(self.encoded.clone()),
            interpreted_value: Some(self.value.clone()),
            unit: None,
            provenance: Provenance {
                parser,
                source,
                stream_id: None,
                source_range: self.source_range,
                method: ExtractionMethod::Structure,
                sequence: id.get(),
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExtractionResult {
    pub parser_id: String,
    pub run: ParserRun,
    pub error: Option<NativeError>,
    pub observations: Vec<NativeObservation>,
}

fn rational(value: &str) -> Option<RawValue> {
    let (numerator, denominator) = value.split_once('/')?;
    Some(RawValue::Rational(Rational {
        numerator: numerator.parse().ok()?,
        denominator: denominator.parse().ok()?,
    }))
}

fn decoded(kind: u32, bytes: &[u8], encoded: &[u8], encoding: &str) -> Option<RawValue> {
    if kind == 0 {
        return Some(RawValue::Bytes(encoded.to_vec()));
    }
    let text = String::from_utf8_lossy(bytes);
    if kind == 4 {
        return Some(RawValue::Text(TextValue {
            text: text.into_owned(),
            original_encoding: (!encoding.is_empty()).then(|| encoding.to_owned()),
            language: None,
            encoded_bytes: Some(encoded.to_vec()),
            lossless: std::str::from_utf8(bytes).is_ok()
                && (encoding != "ASCII" || bytes.is_ascii()),
        }));
    }
    let scalar = |kind, value: &str| match kind {
        1 => value.parse().ok().map(RawValue::Unsigned),
        2 => value.parse().ok().map(RawValue::Signed),
        3 => value.parse().ok().map(RawValue::Float),
        5 => rational(value),
        _ => None,
    };
    if matches!(kind, 6..=9) {
        let scalar_kind = match kind {
            6 => 1,
            7 => 2,
            8 => 3,
            _ => 5,
        };
        let values = text
            .split(',')
            .map(|value| scalar(scalar_kind, value))
            .collect::<Option<Vec<_>>>()?;
        Some(RawValue::Sequence(values))
    } else {
        scalar(kind, &text)
    }
}

pub fn extract_parser(
    input: &BoundedInput,
    parser_id: &str,
    limits: ParserLimits,
) -> Result<ExtractionResult, NativeError> {
    build_info()?;
    if parser_id.is_empty() || limits.max_wall_time_ms == 0 {
        return Err(native_error(NativeErrorCode::InvalidRequest, false));
    }
    let session = input.source.parser_session(limits.max_read_bytes);
    let result = ffi::bridge::ffi_extract(&session, parser_id, limits.ffi());
    let error = (result.run.status != 0).then(|| {
        native_error(
            NativeErrorCode::from(result.run.status),
            result.run.recoverable,
        )
    });
    let mut observations = Vec::new();
    for value in result.values {
        let source_range = if value.located {
            Some(
                ByteRange::new(value.offset, value.length)
                    .ok_or_else(|| native_error(NativeErrorCode::Internal, false))?,
            )
        } else {
            None
        };
        let decoded = decoded(value.kind, &value.value, &value.encoded, &value.encoding)
            .ok_or_else(|| native_error(NativeErrorCode::Internal, false))?;
        observations.push(NativeObservation {
            namespace: value.name_space,
            key: value.key,
            tag_code: value.numeric_tag.then_some(value.tag_code),
            value: decoded,
            encoded: value.encoded,
            source_range,
        });
    }
    Ok(ExtractionResult {
        parser_id: parser_id.to_owned(),
        run: ParserRun {
            read_bytes: result.run.read_bytes,
            allocation_bytes: result.run.allocation_bytes,
            decoded_bytes: result.run.decoded_bytes,
            entries: result.run.entries,
            observations: result.run.observations,
        },
        error,
        observations,
    })
}
