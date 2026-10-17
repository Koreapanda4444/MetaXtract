use crate::{
    AccessMode, AuditAnchor, CaseWorkspace, ImportLimits, IntegrityReport, ManifestSignature,
    Result, SigningIdentity, codec, db, hex, invalid, transport, verify_manifest,
};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::File,
    io::{self, Write},
    path::Path,
};
use tempfile::NamedTempFile;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const FORMAT: &str = "metaxtract.case.snapshot";
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct SnapshotSigner {
    pub name: String,
    pub fingerprint: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SnapshotManifest {
    pub format: String,
    pub revision: u32,
    pub case_schema: u32,
    pub case_id: String,
    pub case_size: u64,
    pub case_sha256: String,
    pub record_count: u64,
    pub audit: Option<AuditAnchor>,
    pub created_ms: i64,
    pub signer: Option<SnapshotSigner>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnapshotVerification {
    pub manifest: SnapshotManifest,
    pub signed: bool,
    pub migrated_from: Option<u32>,
    pub integrity: IntegrityReport,
}
struct Prepared {
    file: NamedTempFile,
    verification: SnapshotVerification,
}

impl CaseWorkspace {
    pub fn export_snapshot(
        &self,
        path: impl AsRef<Path>,
        signer: Option<&SigningIdentity>,
        limits: &ImportLimits,
    ) -> Result<SnapshotVerification> {
        limits.validate()?;
        let path = path.as_ref();
        destination(path)?;
        let database = NamedTempFile::new()?;
        {
            let conn = self.lock()?;
            transport::database_snapshot(&conn, database.path(), limits.max_member_bytes)?;
        }
        let case = Self::open(database.path(), AccessMode::ReadOnly)?;
        let integrity = case.verify_integrity(None)?;
        let info = case.info()?;
        drop(case);
        let size = database.as_file().metadata()?.len();
        let hash = transport::hash_file(database.path(), limits.max_member_bytes)?;
        let manifest = SnapshotManifest {
            format: FORMAT.into(),
            revision: 1,
            case_schema: crate::SCHEMA_REVISION,
            case_id: info.case_id,
            case_size: size,
            case_sha256: hash,
            record_count: integrity.records,
            audit: Some(integrity.anchor.clone()),
            created_ms: info.updated_ms,
            signer: signer.map(|s| SnapshotSigner {
                name: s.name().into(),
                fingerprint: s.fingerprint(),
            }),
        };
        let mut bytes = codec::encode(&manifest, limits.max_control_bytes)?;
        bytes.push(b'\n');
        if bytes.len() > limits.max_control_bytes {
            return Err(invalid("snapshot manifest exceeds control byte budget"));
        }
        let signature = signer
            .map(|s| codec::encode(&s.sign_manifest(&bytes), 16 * 1024))
            .transpose()?;
        let total = size
            .checked_add(bytes.len() as u64)
            .and_then(|v| v.checked_add(signature.as_ref().map_or(0, |s| s.len() as u64)))
            .ok_or_else(|| invalid("snapshot size overflow"))?;
        if total > limits.max_total_bytes
            || limits.max_members < if signer.is_some() { 3 } else { 2 }
        {
            return Err(invalid("snapshot exceeds archive resource limits"));
        }
        let parent = parent(path);
        let mut temporary = NamedTempFile::new_in(parent)?;
        {
            let mut archive = ZipWriter::new(temporary.as_file_mut());
            let options = SimpleFileOptions::default()
                .last_modified_time(zip::DateTime::default())
                .compression_method(CompressionMethod::Deflated)
                .unix_permissions(0o600);
            archive
                .start_file("manifest.json", options)
                .map_err(io::Error::other)?;
            archive.write_all(&bytes)?;
            if let Some(signature) = &signature {
                archive
                    .start_file("signature.json", options)
                    .map_err(io::Error::other)?;
                archive.write_all(signature)?;
            }
            archive
                .start_file("case.sqlite", options.large_file(size > u32::MAX as u64))
                .map_err(io::Error::other)?;
            io::copy(&mut File::open(database.path())?, &mut archive)?;
            archive.finish().map_err(io::Error::other)?;
        }
        if temporary.as_file().metadata()?.len() > limits.max_input_bytes {
            return Err(invalid("compressed snapshot exceeds input byte budget"));
        }
        publish(temporary, path)?;
        Ok(SnapshotVerification {
            manifest,
            signed: signer.is_some(),
            migrated_from: None,
            integrity,
        })
    }
    pub fn open_snapshot(
        source: impl AsRef<Path>,
        destination_path: impl AsRef<Path>,
        access: AccessMode,
        trusted_public_pem: Option<&str>,
        limits: &ImportLimits,
    ) -> Result<Self> {
        let destination_path = destination_path.as_ref();
        destination(destination_path)?;
        let prepared = prepare(source.as_ref(), trusted_public_pem, limits)?;
        let mut temporary = NamedTempFile::new_in(parent(destination_path))?;
        io::copy(
            &mut File::open(prepared.file.path())?,
            temporary.as_file_mut(),
        )?;
        publish(temporary, destination_path)?;
        Self::open(destination_path, access)
    }
}
pub fn verify_snapshot(
    path: impl AsRef<Path>,
    trusted_public_pem: Option<&str>,
    limits: &ImportLimits,
) -> Result<SnapshotVerification> {
    Ok(prepare(path.as_ref(), trusted_public_pem, limits)?.verification)
}

fn prepare(
    path: &Path,
    trusted_public_pem: Option<&str>,
    limits: &ImportLimits,
) -> Result<Prepared> {
    limits.validate()?;
    let (archive_copy, _) = transport::snapshot(path, limits.max_input_bytes)?;
    let mut archive = transport::open_zip(File::open(archive_copy.path())?, limits.max_members)?;
    let mut database = NamedTempFile::new()?;
    let inventory = transport::inventory_copy(
        &mut archive,
        limits.max_member_bytes,
        limits.max_total_bytes,
        "case.sqlite",
        database.as_file_mut(),
    )?;
    database.as_file().sync_all()?;
    let bytes = transport::member(&mut archive, "manifest.json", limits.max_control_bytes)?;
    let manifest: SnapshotManifest = codec::decode(&bytes, limits.max_control_bytes)?;
    if manifest.format != FORMAT
        || manifest.revision != 1
        || manifest.case_schema == 0
        || manifest.case_schema > crate::SCHEMA_REVISION
    {
        return Err(invalid("unsupported portable case snapshot"));
    }
    let signed = inventory.contains_key("signature.json");
    let expected: BTreeSet<_> = if signed {
        ["manifest.json", "case.sqlite", "signature.json"]
            .into_iter()
            .collect()
    } else {
        ["manifest.json", "case.sqlite"].into_iter().collect()
    };
    if inventory
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>()
        != expected
    {
        return Err(invalid("snapshot contains missing or unexpected members"));
    }
    if signed {
        let key = trusted_public_pem
            .ok_or_else(|| invalid("signed snapshot requires an explicitly trusted public key"))?;
        let signature: ManifestSignature = codec::decode(
            &transport::member(&mut archive, "signature.json", 16 * 1024)?,
            16 * 1024,
        )?;
        let fingerprint = verify_manifest(&bytes, &signature, key)?;
        if manifest.signer.as_ref().map(|s| s.fingerprint.as_str()) != Some(fingerprint.as_str()) {
            return Err(invalid("snapshot signer declaration differs"));
        }
    } else if trusted_public_pem.is_some() || manifest.signer.is_some() {
        return Err(invalid("expected a signed snapshot"));
    }
    if inventory.get("case.sqlite") != Some(&(manifest.case_sha256.clone(), manifest.case_size)) {
        return Err(invalid("snapshot database hash or size differs"));
    }
    let source = Connection::open_with_flags(
        database.path(),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(db)?;
    source
        .execute_batch("PRAGMA trusted_schema=OFF;")
        .map_err(db)?;
    let app: i32 = source
        .pragma_query_value(None, "application_id", |r| r.get(0))
        .map_err(db)?;
    let schema: u32 = source
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(db)?;
    if app != crate::APPLICATION_ID || schema != manifest.case_schema {
        return Err(invalid("snapshot database format differs from manifest"));
    }
    if schema >= 6 {
        let original=source.query_row("SELECT c.case_id,a.sequence,a.hash FROM case_info c JOIN audit_log a ON a.sequence=(SELECT max(sequence) FROM audit_log) WHERE c.singleton=1",[],|r|Ok(AuditAnchor{case_id:r.get(0)?,sequence:r.get(1)?,hash:hex(&r.get::<_,Vec<u8>>(2)?)})).map_err(db)?;
        if manifest.audit.as_ref() != Some(&original) {
            return Err(invalid("snapshot audit anchor differs"));
        }
    } else if manifest.audit.is_some() {
        return Err(invalid(
            "pre-audit snapshot cannot declare an audit history",
        ));
    }
    drop(source);
    let migrated_from = (schema < crate::SCHEMA_REVISION).then_some(schema);
    let case = CaseWorkspace::open(
        database.path(),
        if migrated_from.is_some() {
            AccessMode::ReadWrite
        } else {
            AccessMode::ReadOnly
        },
    )?;
    let integrity = case.verify_integrity(None)?;
    if case.info()?.case_id != manifest.case_id || integrity.records != manifest.record_count {
        return Err(invalid("snapshot case identity or record count differs"));
    }
    drop(case);
    Ok(Prepared {
        file: database,
        verification: SnapshotVerification {
            manifest,
            signed,
            migrated_from,
            integrity,
        },
    })
}
fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}
fn destination(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "refusing to replace an existing case or snapshot",
        )),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}
fn publish(file: NamedTempFile, path: &Path) -> Result<()> {
    file.as_file().sync_all()?;
    file.persist_noclobber(path).map_err(|e| e.error)?;
    #[cfg(unix)]
    File::open(parent(path))?.sync_all()?;
    Ok(())
}
