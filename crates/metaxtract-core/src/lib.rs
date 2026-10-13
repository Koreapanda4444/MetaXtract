#![forbid(unsafe_code)]

mod backend_claims;
mod containers;
mod detection;
mod evidence;
mod images;
pub mod scan;

pub use backend_claims::{
    BackendClaimLimit, BackendClaimLimits, BackendClaims, independent_backend_claims,
};
pub use containers::{
    NestedIdentity, RecursiveExtraction, RecursiveLimits, RecursiveNode, extract_file_recursive,
    extract_file_recursive_controlled, extract_recursive,
};

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
