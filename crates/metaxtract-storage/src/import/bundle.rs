use super::*;
use crate::{ManifestSignature, verify_manifest};
use std::collections::BTreeMap;

impl CaseWorkspace {
    pub fn import_python_bundle(
        &self,
        path: impl AsRef<Path>,
        trusted_public_pem: Option<&str>,
        limits: &ImportLimits,
    ) -> Result<ImportSummary> {
        limits.validate()?;
        let path = path.as_ref();
        let (snapshot, hash) = transport::snapshot(path, limits.max_input_bytes)?;
        let mut archive = transport::open_zip(File::open(snapshot.path())?, limits.max_members)?;
        let actual = transport::inventory(
            &mut archive,
            limits.max_member_bytes,
            limits.max_total_bytes,
        )?;
        let manifest_bytes =
            transport::member(&mut archive, "manifest.json", limits.max_control_bytes)?;
        let manifest: Value =
            serde_json::from_slice(&manifest_bytes).map_err(std::io::Error::other)?;
        let signature_bytes = if actual.contains_key("signature.json") {
            Some(transport::member(
                &mut archive,
                "signature.json",
                16 * 1024,
            )?)
        } else {
            None
        };
        let signed = signature_bytes.is_some();
        if let Some(signature) = &signature_bytes {
            let key = trusted_public_pem.ok_or_else(|| {
                invalid("signed bundle requires an explicitly trusted public key")
            })?;
            let signature: ManifestSignature = codec::decode(signature, 16 * 1024)?;
            verify_manifest(&manifest_bytes, &signature, key)?;
        } else if trusted_public_pem.is_some() {
            return Err(invalid("expected a signed bundle"));
        }
        let mut expected = BTreeMap::new();
        for entry in manifest
            .get("artifacts")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid("manifest artifact inventory is missing"))?
        {
            let name = entry
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("invalid artifact path"))?;
            let digest = entry
                .get("sha256")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("invalid artifact digest"))?;
            let size = entry
                .get("size_bytes")
                .and_then(Value::as_u64)
                .ok_or_else(|| invalid("invalid artifact size"))?;
            if name == "manifest.json"
                || name == "signature.json"
                || expected
                    .insert(name.to_owned(), (digest.to_owned(), size))
                    .is_some()
            {
                return Err(invalid("invalid or duplicate artifact declaration"));
            }
        }
        let mut inventory = actual.clone();
        inventory.remove("manifest.json");
        inventory.remove("signature.json");
        if inventory != expected {
            return Err(invalid(
                "bundle artifact inventory, size or SHA-256 differs",
            ));
        }
        for required in ["scan.jsonl", "hashes.txt", "reports/report.json"] {
            if !actual.contains_key(required) {
                return Err(invalid("bundle control is missing"));
            }
        }
        let include = manifest
            .get("includes_files")
            .and_then(Value::as_bool)
            .ok_or_else(|| invalid("invalid bundle evidence mode"))?;
        let originals = manifest
            .get("original_files")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid("missing original evidence inventory"))?;
        let mut original_paths = std::collections::BTreeSet::new();
        for original in originals {
            let name = original
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("invalid original evidence path"))?;
            let expected = (
                original
                    .get("sha256")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid("invalid original digest"))?
                    .to_owned(),
                original
                    .get("size_bytes")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| invalid("invalid original size"))?,
            );
            if !include
                || !original_paths.insert(name)
                || actual.get(&format!("files/{name}")) != Some(&expected)
            {
                return Err(invalid("original evidence inventory differs"));
            }
        }
        if actual.keys().filter(|k| k.starts_with("files/")).count() != originals.len() {
            return Err(invalid("undeclared original evidence"));
        }
        if manifest.get("redacted") == Some(&Value::Bool(true)) && include {
            return Err(invalid("redacted bundle cannot include original evidence"));
        }
        self.write(|tx|{
            let scan=new_scan(tx,&path.to_string_lossy(),"import_python_bundle")?;
            let member=archive.by_name("scan.jsonl").map_err(std::io::Error::other)?;
            let (records,hashes)=read_records(tx,&scan,&mut BufReader::new(member),limits,Some(&manifest))?;
            if actual.get("hashes.txt").map(|v|v.0.as_str())!=Some(hashes.as_str()){return Err(invalid("hashes.txt differs from scan records"));}
            for original in originals{
                let name=original.get("path").and_then(Value::as_str).ok_or_else(||invalid("invalid original evidence path"))?;
                let portable=metaxtract_schema::EvidencePath::root(metaxtract_schema::RelativePath::parse(name.to_owned()).map_err(std::io::Error::other)?).to_string();
                let present:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM records WHERE scan_id=?1 AND path=?2)",params![scan,portable],|r|r.get(0)).map_err(db)?;
                if !present{return Err(invalid("original evidence has no scan record"));}
            }
            tx.execute("UPDATE scans SET status='completed',finished_ms=?2 WHERE id=?1",params![scan,now_ms()?]).map_err(db)?;
            let import_id=receipt(tx,"python_bundle",path,&hash,&json!({"signed":signed,"trusted_public_pem":trusted_public_pem,"source_path":native_path(path),"original_evidence_verified":originals.len(),"original_files_copied":false}))?;
            artifact(tx,import_id,"manifest.json",&manifest_bytes)?;
            if let Some(bytes)=&signature_bytes{artifact(tx,import_id,"signature.json",bytes)?;}
            Ok(ImportSummary{import_id,scans:vec![scan],records,signed,snapshot_sha256:hash.clone()})
        })
    }
}
