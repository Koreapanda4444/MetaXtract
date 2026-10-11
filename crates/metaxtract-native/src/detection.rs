use crate::{
    BoundedInput, NativeError, NativeErrorCode, ParserLimits, build_info, ffi, native_error,
};
use metaxtract_schema::ByteRange;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DetectionState {
    Signature,
    HeaderValidated,
    Truncated,
    Invalid,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentCandidate {
    pub format: String,
    pub mime: String,
    pub container: String,
    pub state: DetectionState,
    pub source_range: ByteRange,
    pub detail: String,
}

pub fn detect_content(
    input: &BoundedInput,
    limits: ParserLimits,
) -> Result<Vec<ContentCandidate>, NativeError> {
    build_info()?;
    let session = input.source.parser_session(limits.max_read_bytes);
    let result = ffi::bridge::ffi_detect(&session, limits.ffi());
    if result.status != 0 {
        return Err(native_error(
            NativeErrorCode::from(result.status),
            result.recoverable,
        ));
    }
    result
        .candidates
        .into_iter()
        .map(|value| {
            let state = match value.state {
                0 => DetectionState::Signature,
                1 => DetectionState::HeaderValidated,
                2 => DetectionState::Truncated,
                3 => DetectionState::Invalid,
                _ => return Err(native_error(NativeErrorCode::Internal, false)),
            };
            let source_range = ByteRange::new(value.offset, value.length)
                .ok_or_else(|| native_error(NativeErrorCode::Internal, false))?;
            Ok(ContentCandidate {
                format: value.format,
                mime: value.mime,
                container: value.container,
                state,
                source_range,
                detail: value.detail,
            })
        })
        .collect()
}
