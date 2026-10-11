#![forbid(unsafe_code)]

mod detection;
mod images;

pub use detection::{ContentAssessment, TypeConflict, TypeConflictKind, assess_content};
pub use images::{
    ImageAnalysis, ImageAnalysisLimits, ImageIssue, ImageIssueKind, ImageNormalizationError,
    reconcile_image_metadata, reconcile_image_metadata_with_limits,
};
pub use metaxtract_schema as schema;

pub const SCHEMA_REVISION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EngineInfo {
    pub schema_revision: u32,
    pub native_api_revision: Option<u32>,
}

impl Default for EngineInfo {
    fn default() -> Self {
        Self {
            schema_revision: SCHEMA_REVISION,
            native_api_revision: None,
        }
    }
}
