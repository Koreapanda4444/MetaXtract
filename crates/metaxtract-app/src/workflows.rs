//! Shared, bounded application operations for the CLI and desktop.
use crate::{analysis, storage};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    io::{self, Write},
    path::Path,
};

pub fn read_json<T: DeserializeOwned>(path: &Path) -> io::Result<T> {
    serde_json::from_slice(&crate::read_bytes(path, 16 * 1024 * 1024)?).map_err(io::Error::other)
}
pub fn read_text(path: &Path, max: usize) -> io::Result<String> {
    String::from_utf8(crate::read_bytes(path, max)?).map_err(io::Error::other)
}
/// Publish a complete output without replacing an existing destination.
pub fn write_bytes(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut out = tempfile::NamedTempFile::new_in(parent)?;
    out.write_all(bytes)?;
    out.as_file().sync_all()?;
    out.persist_noclobber(path).map_err(|e| e.error)?;
    #[cfg(unix)]
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}
pub fn write_json(path: &Path, value: &impl Serialize) -> io::Result<()> {
    write_bytes(
        path,
        &serde_json::to_vec_pretty(value).map_err(io::Error::other)?,
    )
}
pub fn profile(
    path: Option<&Path>,
    privacy: bool,
) -> io::Result<Option<analysis::redaction::RedactionProfile>> {
    let profile = if let Some(path) = path {
        Some(read_json::<analysis::redaction::RedactionProfile>(path)?)
    } else if privacy {
        Some(analysis::redaction::RedactionProfile::privacy())
    } else {
        None
    };
    if let Some(p) = &profile {
        p.validate()?;
    }
    Ok(profile)
}
pub fn report_options(
    title: &str,
    analyst: &str,
) -> io::Result<analysis::reporting::ReportOptions> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(io::Error::other)?;
    Ok(analysis::reporting::ReportOptions::new(
        title,
        analyst,
        i64::try_from(now.as_secs()).map_err(io::Error::other)?,
    ))
}
pub fn identity(path: &Path, name: &str, password: &str) -> io::Result<storage::SigningIdentity> {
    storage::SigningIdentity::load_encrypted(name, path, password, &Default::default())
}
pub fn selected_query(
    scan: &str,
    ids: &std::collections::BTreeSet<i64>,
) -> io::Result<storage::SearchQuery> {
    if ids.is_empty() || ids.len() > 64 || ids.iter().any(|id| *id <= 0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "select 1..64 records",
        ));
    }
    let query = storage::SearchQuery::new(
        scan,
        storage::SearchExpression::Any(
            ids.iter()
                .map(|id| {
                    storage::SearchExpression::Predicate(storage::SearchPredicate {
                        field: storage::SearchField::RecordId,
                        operator: storage::SearchOperator::Equal,
                        value: id.to_string(),
                    })
                })
                .collect(),
        ),
    );
    query.validate()?;
    Ok(query)
}
