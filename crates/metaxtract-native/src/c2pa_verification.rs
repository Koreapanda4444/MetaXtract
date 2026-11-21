//! Offline verification of a deliberately bounded C2PA subset, inside the bundled worker.
use crate::{BoundedInput, ExtractionResult, NativeObservation, ParserLimits, read_input};
use ciborium::Value;
use metaxtract_schema::{ExtractionMethod, RawValue, TextValue};
use openssl::{
    bn::BigNum,
    ecdsa::EcdsaSig,
    hash::{Hasher, MessageDigest, hash},
    nid::Nid,
    pkey::Id,
    rsa::Padding,
    sign::{RsaPssSaltlen, Verifier},
    x509::X509,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Cursor,
};
type Check<T> = Result<T, &'static str>;
fn text(v: &RawValue) -> Option<&str> {
    if let RawValue::Text(v) = v {
        v.lossless.then_some(v.text.as_str())
    } else {
        None
    }
}
fn number(v: &RawValue) -> Option<u64> {
    match v {
        RawValue::Unsigned(v) => Some(*v),
        RawValue::Signed(v) => (*v).try_into().ok(),
        _ => None,
    }
}
fn txt(v: &Value) -> Check<&str> {
    if let Value::Text(v) = v {
        Ok(v)
    } else {
        Err("malformed_text_field")
    }
}
fn uint(v: &Value) -> Check<u64> {
    if let Value::Integer(v) = v {
        u64::try_from(*v).map_err(|_| "malformed_integer_field")
    } else {
        Err("malformed_integer_field")
    }
}
fn bytes(v: &Value) -> Check<&[u8]> {
    if let Value::Bytes(v) = v {
        Ok(v)
    } else {
        Err("malformed_byte_string")
    }
}
fn array(v: &Value) -> Check<&[Value]> {
    if let Value::Array(v) = v {
        Ok(v)
    } else {
        Err("malformed_array")
    }
}
fn map(v: &Value) -> Check<&[(Value, Value)]> {
    if let Value::Map(v) = v {
        Ok(v)
    } else {
        Err("malformed_map")
    }
}
fn field<'a>(v: &'a Value, key: &str) -> Check<Option<&'a Value>> {
    Ok(map(v)?
        .iter()
        .find(|(k, _)| matches!(k,Value::Text(t) if t==key))
        .map(|(_, v)| v))
}
fn required<'a>(v: &'a Value, key: &str) -> Check<&'a Value> {
    field(v, key)?.ok_or("missing_required_field")
}
fn header(v: &Value, key: i64) -> Check<Option<&Value>> {
    Ok(map(v)?
        .iter()
        .find(|(k, _)| matches!(k,Value::Integer(i) if i128::from(*i)==i128::from(key)))
        .map(|(_, v)| v))
}
fn cbor(data: &[u8]) -> Check<Value> {
    if data.len() > 2 * 1024 * 1024 {
        return Err("resource_limit");
    }
    let mut cursor = Cursor::new(data);
    let value: Value = ciborium::de::from_reader_with_recursion_limit(&mut cursor, 32)
        .map_err(|_| "malformed_or_overdeep_cbor")?;
    if cursor.position() != data.len() as u64 {
        return Err("trailing_cbor_bytes");
    }
    let mut pending = vec![&value];
    let mut count = 0;
    while let Some(value) = pending.pop() {
        count += 1;
        if count > 50000 {
            return Err("resource_limit");
        }
        match value {
            Value::Array(a) => pending.extend(a),
            Value::Tag(_, v) => pending.push(v),
            Value::Map(m) => {
                let mut keys = BTreeSet::new();
                for (key, value) in m {
                    let key = match key {
                        Value::Text(v) => format!("s:{v}"),
                        Value::Integer(v) => format!("i:{}", i128::from(*v)),
                        _ => return Err("unsupported_cbor_map_key"),
                    };
                    if !keys.insert(key) {
                        return Err("duplicate_cbor_map_key");
                    }
                    pending.push(value);
                }
            }
            _ => {}
        }
    }
    Ok(value)
}
fn digest(name: &str) -> Check<MessageDigest> {
    match name {
        "sha256" => Ok(MessageDigest::sha256()),
        "sha384" => Ok(MessageDigest::sha384()),
        "sha512" => Ok(MessageDigest::sha512()),
        _ => Err("unsupported_hash_algorithm"),
    }
}
fn hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}
struct CoseResult {
    algorithm: String,
    certificate: String,
}
fn cose(claim: &[u8], signature: &[u8]) -> Check<CoseResult> {
    let value = cbor(signature)?;
    let value = match &value {
        Value::Tag(18, v) => &**v,
        Value::Tag(_, _) => return Err("unsupported_cose_tag"),
        v => v,
    };
    let parts = array(value)?;
    if parts.len() != 4 {
        return Err("malformed_cose_sign1");
    }
    let protected_bytes = bytes(&parts[0])?;
    let protected = cbor(protected_bytes)?;
    let unsigned = &parts[1];
    map(unsigned)?;
    for (key, _) in map(&protected)? {
        if map(unsigned)?.iter().any(|(other, _)| other == key) {
            return Err("duplicate_cose_header");
        }
    }
    if let Some(critical) = header(&protected, 2)? {
        if array(critical)?.is_empty() {
            return Err("malformed_critical_cose_header");
        }
        let mut seen = BTreeSet::new();
        for key in array(critical)? {
            let allowed = matches!(key,Value::Integer(v) if matches!(i128::from(*v),1|33) && seen.insert(i128::from(*v)));
            if !allowed || !map(&protected)?.iter().any(|(present, _)| present == key) {
                return Err("unsupported_critical_cose_header");
            }
        }
    }
    let algorithm = match header(&protected, 1)?.ok_or("missing_protected_algorithm")? {
        Value::Integer(v) => i64::try_from(*v).map_err(|_| "unsupported_signature_algorithm")?,
        _ => return Err("malformed_signature_algorithm"),
    };
    if !matches!(parts[2], Value::Null) && bytes(&parts[2])? != claim {
        return Err("cose_payload_does_not_match_claim");
    }
    let certificates = header(&protected, 33)?
        .or(header(unsigned, 33)?)
        .ok_or("missing_signer_certificate")?;
    let certificate = match certificates {
        Value::Bytes(v) => v.as_slice(),
        Value::Array(v) if !v.is_empty() && v.len() <= 16 => {
            for certificate in v {
                if bytes(certificate)?.len() > 64 * 1024 {
                    return Err("resource_limit");
                }
            }
            bytes(&v[0])?
        }
        _ => return Err("malformed_certificate_chain"),
    };
    if certificate.len() > 64 * 1024 {
        return Err("resource_limit");
    }
    let cert = X509::from_der(certificate).map_err(|_| "malformed_signer_certificate")?;
    if cert.to_der().map_err(|_| "malformed_signer_certificate")? != certificate {
        return Err("non_der_or_trailing_certificate_bytes");
    }
    let key = cert.public_key().map_err(|_| "unsupported_signer_key")?;
    let structure = Value::Array(vec![
        Value::Text("Signature1".into()),
        Value::Bytes(protected_bytes.to_vec()),
        Value::Bytes(Vec::new()),
        Value::Bytes(claim.to_vec()),
    ]);
    let mut data = Vec::new();
    ciborium::ser::into_writer(&structure, &mut data).map_err(|_| "cose_encoding_failed")?;
    let signature = bytes(&parts[3])?;
    let verified = match algorithm {
        -7 | -35 | -36 => {
            let (curve, length, hash) = match algorithm {
                -7 => (Nid::X9_62_PRIME256V1, 32, MessageDigest::sha256()),
                -35 => (Nid::SECP384R1, 48, MessageDigest::sha384()),
                _ => (Nid::SECP521R1, 66, MessageDigest::sha512()),
            };
            let ec = key.ec_key().map_err(|_| "signature_key_type_mismatch")?;
            if ec.group().curve_name() != Some(curve) || signature.len() != length * 2 {
                return Err("signature_key_or_length_mismatch");
            }
            let sig = EcdsaSig::from_private_components(
                BigNum::from_slice(&signature[..length])
                    .map_err(|_| "malformed_ecdsa_signature")?,
                BigNum::from_slice(&signature[length..])
                    .map_err(|_| "malformed_ecdsa_signature")?,
            )
            .map_err(|_| "malformed_ecdsa_signature")?
            .to_der()
            .map_err(|_| "malformed_ecdsa_signature")?;
            let mut verifier =
                Verifier::new(hash, &key).map_err(|_| "unsupported_signature_algorithm")?;
            verifier
                .verify_oneshot(&sig, &data)
                .map_err(|_| "signature_verification_error")?
        }
        -39..=-37 => {
            if !matches!(key.id(), Id::RSA | Id::RSA_PSS)
                || !(2048..=8192).contains(&key.bits())
                || signature.len() != key.size()
            {
                return Err("signature_key_type_or_strength_mismatch");
            }
            let hash = match algorithm {
                -37 => MessageDigest::sha256(),
                -38 => MessageDigest::sha384(),
                _ => MessageDigest::sha512(),
            };
            let mut verifier =
                Verifier::new(hash, &key).map_err(|_| "unsupported_signature_algorithm")?;
            verifier
                .set_rsa_padding(Padding::PKCS1_PSS)
                .map_err(|_| "unsupported_pss_parameters")?;
            verifier
                .set_rsa_mgf1_md(hash)
                .map_err(|_| "unsupported_pss_parameters")?;
            verifier
                .set_rsa_pss_saltlen(RsaPssSaltlen::DIGEST_LENGTH)
                .map_err(|_| "unsupported_pss_parameters")?;
            verifier
                .verify_oneshot(signature, &data)
                .map_err(|_| "signature_verification_error")?
        }
        -8 => {
            if key.id() != Id::ED25519 || signature.len() != 64 {
                return Err("unsupported_eddsa_key");
            }
            Verifier::new_without_digest(&key)
                .map_err(|_| "unsupported_signature_algorithm")?
                .verify_oneshot(signature, &data)
                .map_err(|_| "signature_verification_error")?
        }
        _ => return Err("unsupported_signature_algorithm"),
    };
    if !verified {
        return Err("invalid");
    }
    Ok(CoseResult {
        algorithm: algorithm.to_string(),
        certificate: hex(
            &hash(MessageDigest::sha256(), certificate).map_err(|_| "certificate_digest_failed")?
        ),
    })
}

struct BoxInfo<'a> {
    id: u64,
    parent: u64,
    offset: u64,
    size: u64,
    header: u64,
    label: &'a str,
    uuid: &'a str,
    kind: &'a str,
}
fn payload<'a>(node: &BoxInfo<'_>, packet: &'a [u8]) -> Check<&'a [u8]> {
    let start = node
        .offset
        .checked_add(node.header)
        .ok_or("invalid_jumbf_range")?;
    let end = node
        .offset
        .checked_add(node.size)
        .ok_or("invalid_jumbf_range")?;
    if node.header < 8 || node.header > node.size {
        return Err("invalid_jumbf_range");
    }
    packet
        .get(
            usize::try_from(start).map_err(|_| "invalid_jumbf_range")?
                ..usize::try_from(end).map_err(|_| "invalid_jumbf_range")?,
        )
        .ok_or("invalid_jumbf_range")
}
fn child<'a>(boxes: &'a [BoxInfo<'a>], parent: u64, label: &str) -> Check<&'a BoxInfo<'a>> {
    let mut found = boxes
        .iter()
        .filter(|b| b.parent == parent && b.label == label);
    let first = found.next().ok_or("missing_jumbf_reference")?;
    if found.next().is_some() {
        return Err("ambiguous_jumbf_label");
    }
    Ok(first)
}
fn cbor_child<'a>(boxes: &'a [BoxInfo<'a>], parent: u64, packet: &'a [u8]) -> Check<&'a [u8]> {
    let mut children = boxes
        .iter()
        .filter(|b| b.parent == parent && b.kind != "free");
    let first = children.next().ok_or("missing_cbor_box")?;
    if children.next().is_some() || first.kind != "cbor" {
        return Err("ambiguous_or_unsupported_content_box");
    }
    payload(first, packet)
}
fn label_segment(value: &str) -> Check<String> {
    let mut out = Vec::new();
    let mut at = 0;
    let bytes = value.as_bytes();
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let part = bytes.get(at + 1..at + 3).ok_or("malformed_jumbf_uri")?;
            let text = std::str::from_utf8(part).map_err(|_| "malformed_jumbf_uri")?;
            out.push(u8::from_str_radix(text, 16).map_err(|_| "malformed_jumbf_uri")?);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    let out = String::from_utf8(out).map_err(|_| "malformed_jumbf_uri")?;
    if out.is_empty() || out == "." || out == ".." || out.contains(['\0', '/']) {
        return Err("unsupported_jumbf_uri");
    }
    Ok(out)
}
fn resolve<'a>(
    boxes: &'a [BoxInfo<'a>],
    manifest: &BoxInfo<'_>,
    uri: &str,
) -> Check<&'a BoxInfo<'a>> {
    let path = uri
        .strip_prefix("self#jumbf=")
        .ok_or("unsupported_external_reference")?;
    if path.len() > 16384 {
        return Err("resource_limit");
    }
    let absolute = path.starts_with('/');
    let mut components = path
        .trim_start_matches('/')
        .split('/')
        .map(label_segment)
        .collect::<Check<Vec<_>>>()?;
    let mut current = if absolute {
        let root = boxes
            .iter()
            .find(|b| b.parent == u64::MAX)
            .ok_or("missing_jumbf_root")?;
        if components.first().map(String::as_str) != Some(root.label) {
            return Err("jumbf_reference_outside_store");
        }
        components.remove(0);
        root.id
    } else {
        manifest.id
    };
    for component in components {
        current = child(boxes, current, &component)?.id;
    }
    boxes
        .iter()
        .find(|b| b.id == current)
        .ok_or("missing_jumbf_reference")
}
fn algorithm<'a>(item: &'a Value, claim: &'a Value) -> Check<&'a str> {
    field(item, "alg")?
        .or(field(claim, "alg")?)
        .ok_or("unsupported_missing_hash_algorithm")
        .and_then(txt)
}
fn assertion_hashes<'a>(
    boxes: &'a [BoxInfo<'a>],
    manifest: &BoxInfo<'_>,
    claim: &Value,
    packet: &[u8],
) -> Check<Vec<u64>> {
    if field(claim, "redacted_assertions")?.is_some() {
        return Err("unsupported_redacted_assertions");
    }
    let store = child(boxes, manifest.id, "c2pa.assertions")?;
    if store.uuid != "6332617300110010800000aa00389b71" {
        return Err("invalid_assertion_store_type");
    }
    let legacy = field(claim, "assertions")?.is_some();
    let created = field(claim, "created_assertions")?.is_some();
    let gathered = field(claim, "gathered_assertions")?.is_some();
    if (legacy && (created || gathered)) || (!legacy && !created) {
        return Err("ambiguous_or_missing_assertion_schema");
    }
    let mut references = Vec::new();
    for name in ["assertions", "created_assertions", "gathered_assertions"] {
        if let Some(value) = field(claim, name)? {
            references.extend(array(value)?);
        }
    }
    if references.is_empty() || references.len() > 1024 {
        return Err("missing_or_excessive_assertion_references");
    }
    let mut verified = BTreeSet::new();
    for reference in references {
        let node = resolve(boxes, manifest, txt(required(reference, "url")?)?)?;
        if node.parent != store.id || node.kind != "jumb" || !verified.insert(node.id) {
            return Err("assertion_outside_manifest_or_duplicate");
        }
        let expected = bytes(required(reference, "hash")?)?;
        let actual = hash(
            digest(algorithm(reference, claim)?)?,
            payload(node, packet)?,
        )
        .map_err(|_| "assertion_digest_failed")?;
        if actual.as_ref() != expected {
            return Err("invalid");
        }
    }
    if boxes
        .iter()
        .any(|b| b.parent == store.id && b.kind != "free" && !verified.contains(&b.id))
    {
        return Err("undeclared_assertion");
    }
    Ok(verified.into_iter().collect())
}
fn merged(mut ranges: Vec<(u64, u64)>) -> Check<Vec<(u64, u64)>> {
    ranges.sort_unstable();
    let mut out: Vec<(u64, u64)> = Vec::new();
    for (start, length) in ranges {
        let end = start.checked_add(length).ok_or("invalid_exclusion_range")?;
        if length == 0 {
            return Err("invalid_exclusion_range");
        }
        if let Some(last) = out.last_mut() {
            let last_end = last.0 + last.1;
            if start < last_end {
                return Err("overlapping_exclusions");
            }
            if start == last_end {
                last.1 = end - last.0;
                continue;
            }
        }
        out.push((start, length));
    }
    Ok(out)
}
struct BindingContext<'a> {
    input: &'a BoundedInput,
    packet: &'a [u8],
    carrier: &'a [(u64, u64)],
    run: &'a mut crate::ParserRun,
    limits: ParserLimits,
}
fn binding(
    context: &mut BindingContext<'_>,
    boxes: &[BoxInfo<'_>],
    verified: &[u64],
    claim: &Value,
) -> Check<()> {
    let input = context.input;
    let packet = context.packet;
    let carrier = context.carrier;
    let limits = context.limits;
    let result = &mut context.run;
    if carrier.is_empty() {
        return Err("external_asset_required");
    }
    let candidates = boxes
        .iter()
        .filter(|b| verified.contains(&b.id) && b.label == "c2pa.hash.data")
        .collect::<Vec<_>>();
    if candidates.len() != 1
        || boxes.iter().any(|b| {
            verified.contains(&b.id)
                && b.label.starts_with("c2pa.hash.")
                && b.label != "c2pa.hash.data"
        })
    {
        return Err("unsupported_or_missing_hard_binding");
    }
    let value = cbor(cbor_child(boxes, candidates[0].id, packet)?)?;
    let mut ranges = Vec::new();
    for range in array(required(&value, "exclusions")?)? {
        ranges.push((
            uint(required(range, "start")?)?,
            uint(required(range, "length")?)?,
        ));
        if ranges.len() > 128 {
            return Err("resource_limit");
        }
    }
    let ranges = merged(ranges)?;
    if ranges != merged(carrier.to_vec())? {
        return Err("unsupported_exclusion_policy");
    }
    let size = read_input(input, 0, 0)
        .map_err(|_| "source_read_failed")?
        .source_size;
    if ranges
        .iter()
        .any(|(start, length)| *start > size || *length > size - *start)
    {
        return Err("invalid_exclusion_range");
    }
    let wanted = bytes(required(&value, "hash")?)?;
    let mut hasher =
        Hasher::new(digest(algorithm(&value, claim)?)?).map_err(|_| "content_digest_failed")?;
    let mut at = 0;
    for (end, next) in ranges
        .iter()
        .map(|(start, length)| (*start, start + length))
        .chain(std::iter::once((size, size)))
    {
        while at < end {
            let count = (end - at).min(65536) as u32;
            if u64::from(count) > limits.max_read_bytes.saturating_sub(result.read_bytes) {
                return Err("resource_limit");
            }
            let read = read_input(input, at, count).map_err(|_| "source_read_failed")?;
            if read.bytes.len() != count as usize {
                return Err("source_read_failed");
            }
            result.read_bytes += u64::from(count);
            hasher
                .update(&read.bytes)
                .map_err(|_| "content_digest_failed")?;
            at += u64::from(count);
        }
        at = next;
    }
    if hasher
        .finish()
        .map_err(|_| "content_digest_failed")?
        .as_ref()
        != wanted
    {
        return Err("invalid");
    }
    Ok(())
}
fn observed(namespace: &str, key: &str, value: &str, stream: &str) -> NativeObservation {
    NativeObservation {
        namespace: namespace.into(),
        key: key.into(),
        tag_code: None,
        value: RawValue::Text(TextValue {
            text: value.into(),
            original_encoding: Some("UTF-8".into()),
            language: None,
            encoded_bytes: None,
            lossless: true,
        }),
        encoded: Vec::new(),
        source_range: None,
        method: ExtractionMethod::NativeLibrary,
        stream_id: Some(stream.into()),
    }
}

/// Adds independent scoped verdicts. A cryptographically valid claim never asserts content truth.
pub fn verify_c2pa(input: &BoundedInput, result: &mut ExtractionResult, limits: ParserLimits) {
    if result.parser_id != "c2pa.manifest" {
        return;
    }
    let mut fields: BTreeMap<&str, BTreeMap<&str, &RawValue>> = BTreeMap::new();
    let mut carrier = Vec::new();
    let mut offset = None;
    let mut packet = None;
    for observation in &result.observations {
        fields
            .entry(&observation.namespace)
            .or_default()
            .insert(&observation.key, &observation.value);
        if observation.namespace == "c2pa.store0" && observation.key == "store_packet" {
            if let RawValue::Bytes(data) = &observation.value {
                packet = Some(data.as_slice());
            }
        }
        if observation.namespace == "c2pa.container" {
            if observation.key == "manifest_segment_offset" {
                offset = number(&observation.value);
            }
            if observation.key == "manifest_segment_length" {
                if let (Some(start), Some(length)) = (offset.take(), number(&observation.value)) {
                    carrier.push((start, length));
                }
            }
        }
    }
    let Some(packet) = packet else {
        return;
    };
    let mut boxes = Vec::new();
    for (namespace, fields) in &fields {
        if !namespace.starts_with("c2pa.store0.box") {
            continue;
        }
        let get = |key: &str| fields.get(key).and_then(|v| number(v));
        let read = |key: &str| fields.get(key).and_then(|v| text(v));
        if let (Some(id), Some(parent), Some(offset), Some(size), Some(header), Some(kind)) = (
            get("box_index"),
            get("parent_index"),
            get("box_offset"),
            get("box_size"),
            get("box_header_size"),
            read("box_type"),
        ) {
            boxes.push(BoxInfo {
                id,
                parent,
                offset,
                size,
                header,
                label: read("box_label").unwrap_or(""),
                uuid: read("box_uuid").unwrap_or(""),
                kind,
            });
        }
    }
    boxes.sort_by_key(|b| b.offset);
    let manifests = boxes
        .iter()
        .filter(|b| {
            b.parent == 0
                && b.kind == "jumb"
                && matches!(
                    b.uuid,
                    "63326d6100110010800000aa00389b71"
                        | "63326d6400110010800000aa00389b71"
                        | "6332756d00110010800000aa00389b71"
                        | "6332636d00110010800000aa00389b71"
                )
        })
        .collect::<Vec<_>>();
    let active = manifests.last().map(|b| b.id);
    let mut added = Vec::new();
    let store_count = fields
        .get("c2pa.container")
        .and_then(|f| f.get("store_count"))
        .and_then(|v| number(v));
    for (index, manifest) in manifests.iter().take(32).enumerate() {
        let namespace = format!("c2pa.verification.manifest{index}");
        let mut signature = "unverified_container";
        let mut assertions = "unverified_claim_signature";
        let mut content = "unverified_claim_signature";
        let mut extra = None;
        let working = 8 * 1024 * 1024 + (packet.len() as u64).saturating_mul(16);
        if working
            > limits
                .max_allocation_bytes
                .saturating_sub(result.run.allocation_bytes)
        {
            signature = "resource_limit";
        } else if result.error.is_none() && store_count == Some(1) {
            result.run.allocation_bytes += working;
            let check = (|| -> Check<(&[u8], Value, CoseResult)> {
                if !boxes.iter().any(|b| {
                    b.id == 0
                        && b.parent == u64::MAX
                        && b.label == "c2pa"
                        && b.uuid == "6332706100110010800000aa00389b71"
                }) {
                    return Err("invalid_c2pa_root");
                }
                if !matches!(
                    manifest.uuid,
                    "63326d6100110010800000aa00389b71" | "63326d6400110010800000aa00389b71"
                ) {
                    return Err("unsupported_manifest_type");
                }
                let mut claims = boxes.iter().filter(|b| {
                    b.parent == manifest.id && matches!(b.label, "c2pa.claim" | "c2pa.claim.v2")
                });
                let claim = claims.next().ok_or("missing_claim")?;
                if claims.next().is_some() {
                    return Err("multiple_claims");
                }
                if claim.uuid != "6332636c00110010800000aa00389b71" {
                    return Err("invalid_claim_box_type");
                }
                let data = cbor_child(&boxes, claim.id, packet)?;
                let value = cbor(data)?;
                let signature = resolve(&boxes, manifest, txt(required(&value, "signature")?)?)?;
                if signature.parent != manifest.id
                    || signature.label != "c2pa.signature"
                    || signature.uuid != "6332637300110010800000aa00389b71"
                {
                    return Err("invalid_signature_reference");
                }
                let verdict = cose(data, cbor_child(&boxes, signature.id, packet)?)?;
                Ok((data, value, verdict))
            })();
            match check {
                Err(error) => signature = error,
                Ok((_data, claim, verdict)) => {
                    signature = "valid_for_claim_bytes";
                    content = "unverified_assertion_integrity";
                    extra = Some(verdict);
                    match assertion_hashes(&boxes, manifest, &claim, packet) {
                        Err(error) => assertions = error,
                        Ok(verified) => {
                            assertions = "valid_referenced_assertion_hashes";
                            if Some(manifest.id) != active {
                                content = "not_current_asset_manifest";
                            } else {
                                content = match binding(
                                    &mut BindingContext {
                                        input,
                                        packet,
                                        carrier: &carrier,
                                        run: &mut result.run,
                                        limits,
                                    },
                                    &boxes,
                                    &verified,
                                    &claim,
                                ) {
                                    Ok(()) => "valid_for_supported_asset_ranges",
                                    Err(error) => error,
                                };
                            }
                        }
                    }
                }
            }
        }
        let stream = format!("c2pa.store0.box{}", manifest.id);
        let mut values=vec![("manifest_label",manifest.label.to_owned()),("manifest_box",stream.clone()),("active",if Some(manifest.id)==active {"true"}else{"false"}.into()),("signature_integrity",signature.into()),("assertion_integrity",assertions.into()),("content_binding",content.into()),("certificate_trust","unknown_no_C2PA_trust_policy".into()),("validation_scope","COSE_claim_signature_and_referenced_assertion_hashes_and_simple_data_hash; full_C2PA_profile_certificates_timestamps_ingredients_and_content_truth_not_verified".into())];
        if let Some(extra) = extra {
            values.push(("signature_algorithm", extra.algorithm));
            values.push(("signer_certificate_sha256", extra.certificate));
        }
        for (key, value) in values {
            if result.run.observations >= limits.max_observations {
                break;
            }
            added.push(observed(&namespace, key, &value, &stream));
            result.run.observations += 1;
        }
    }
    result.observations.extend(added);
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use openssl::{
        asn1::Asn1Time,
        ec::{EcGroup, EcKey},
        pkey::{PKey, Private},
        rsa::Rsa,
        sign::Signer,
        x509::X509NameBuilder,
    };
    fn signed_cose(algorithm: i64, key: &PKey<Private>) -> Vec<u8> {
        let mut name = X509NameBuilder::new().unwrap();
        name.append_entry_by_text("CN", "Synthetic COSE test")
            .unwrap();
        let name = name.build();
        let mut cert = X509::builder().unwrap();
        cert.set_version(2).unwrap();
        cert.set_subject_name(&name).unwrap();
        cert.set_issuer_name(&name).unwrap();
        cert.set_pubkey(key).unwrap();
        cert.set_not_before(&Asn1Time::days_from_now(0).unwrap())
            .unwrap();
        cert.set_not_after(&Asn1Time::days_from_now(1).unwrap())
            .unwrap();
        cert.sign(
            key,
            if key.id() == Id::ED25519 {
                MessageDigest::null()
            } else {
                MessageDigest::sha256()
            },
        )
        .unwrap();
        let protected = Value::Map(vec![
            (Value::Integer(1.into()), Value::Integer(algorithm.into())),
            (
                Value::Integer(33.into()),
                Value::Bytes(cert.build().to_der().unwrap()),
            ),
        ]);
        let mut encoded = Vec::new();
        ciborium::ser::into_writer(&protected, &mut encoded).unwrap();
        let mut data = Vec::new();
        ciborium::ser::into_writer(
            &Value::Array(vec![
                Value::Text("Signature1".into()),
                Value::Bytes(encoded.clone()),
                Value::Bytes(vec![]),
                Value::Bytes(b"claim".to_vec()),
            ]),
            &mut data,
        )
        .unwrap();
        let hash = match algorithm {
            -7 | -37 => MessageDigest::sha256(),
            -35 | -38 => MessageDigest::sha384(),
            _ => MessageDigest::sha512(),
        };
        let signature = if algorithm == -8 {
            Signer::new_without_digest(key)
                .unwrap()
                .sign_oneshot_to_vec(&data)
                .unwrap()
        } else {
            let mut signer = Signer::new(hash, key).unwrap();
            if (-39..=-37).contains(&algorithm) {
                signer.set_rsa_padding(Padding::PKCS1_PSS).unwrap();
                signer.set_rsa_mgf1_md(hash).unwrap();
                signer
                    .set_rsa_pss_saltlen(RsaPssSaltlen::DIGEST_LENGTH)
                    .unwrap();
            }
            let signature = signer.sign_oneshot_to_vec(&data).unwrap();
            if matches!(algorithm, -7 | -35 | -36) {
                let width = match algorithm {
                    -7 => 32,
                    -35 => 48,
                    _ => 66,
                };
                let signature = EcdsaSig::from_der(&signature).unwrap();
                [
                    signature.r().to_vec_padded(width).unwrap(),
                    signature.s().to_vec_padded(width).unwrap(),
                ]
                .concat()
            } else {
                signature
            }
        };
        let mut out = Vec::new();
        ciborium::ser::into_writer(
            &Value::Tag(
                18,
                Box::new(Value::Array(vec![
                    Value::Bytes(encoded),
                    Value::Map(vec![]),
                    Value::Null,
                    Value::Bytes(signature),
                ])),
            ),
            &mut out,
        )
        .unwrap();
        out
    }
    fn sample() -> Vec<u8> {
        let corpus: serde_json::Value =
            serde_json::from_str(include_str!("../../../conformance/corpus/native-c2pa.json"))
                .unwrap();
        base64::engine::general_purpose::STANDARD
            .decode(corpus["cases"][0]["base64"].as_str().unwrap())
            .unwrap()
    }
    fn verify(data: Vec<u8>, limits: ParserLimits) -> BTreeMap<String, String> {
        let input = BoundedInput::from_bytes(data, crate::InputLimits::default()).unwrap();
        let mut result =
            crate::extract_parser(&input, "c2pa.manifest", ParserLimits::default()).unwrap();
        assert!(result.error.is_none(), "{:?}", result.error);
        verify_c2pa(&input, &mut result, limits);
        result
            .observations
            .iter()
            .filter(|o| o.namespace.starts_with("c2pa.verification"))
            .filter_map(|o| text(&o.value).map(|v| (o.key.clone(), v.into())))
            .collect()
    }
    #[test]
    fn official_sdk_claim_assertions_and_asset_bytes_are_verified_separately() {
        let data = sample();
        let report = verify(data.clone(), ParserLimits::default());
        assert_eq!(report["signature_integrity"], "valid_for_claim_bytes");
        assert_eq!(
            report["assertion_integrity"],
            "valid_referenced_assertion_hashes"
        );
        assert_eq!(
            report["content_binding"],
            "valid_for_supported_asset_ranges"
        );
        assert!(report["certificate_trust"].starts_with("unknown"));
        let mut changed = data.clone();
        let end = changed.len() - 40;
        changed[end] ^= 1;
        let report = verify(changed, ParserLimits::default());
        assert_eq!(report["signature_integrity"], "valid_for_claim_bytes");
        assert_eq!(
            report["assertion_integrity"],
            "valid_referenced_assertion_hashes"
        );
        assert_eq!(report["content_binding"], "invalid");
        let mut changed = data.clone();
        let at = changed
            .windows(15)
            .position(|v| v == b"make_test_image")
            .unwrap();
        changed[at] = b'n';
        let report = verify(changed, ParserLimits::default());
        assert_eq!(report["signature_integrity"], "invalid");
        assert_eq!(report["content_binding"], "unverified_claim_signature");
        let mut changed = data;
        let at = changed
            .windows(12)
            .position(|v| v == b"c2pa.created")
            .unwrap();
        changed[at] = b'd';
        let report = verify(changed, ParserLimits::default());
        assert_eq!(report["signature_integrity"], "valid_for_claim_bytes");
        assert_eq!(report["assertion_integrity"], "invalid");
        assert_eq!(report["content_binding"], "unverified_assertion_integrity");
    }
    #[test]
    fn ambiguous_cbor_critical_headers_and_verification_limits_fail_closed() {
        assert!(matches!(
            cbor(b"\xa2\x61a\x01\x61a\x02"),
            Err("duplicate_cbor_map_key")
        ));
        assert!(cbor(&[0x81; 40]).is_err());
        let mut encoded = Vec::new();
        let mut protected = Vec::new();
        ciborium::ser::into_writer(
            &Value::Map(vec![(
                Value::Integer(1.into()),
                Value::Integer((-7).into()),
            )]),
            &mut protected,
        )
        .unwrap();
        let cose_value = Value::Array(vec![
            Value::Bytes(protected),
            Value::Map(vec![(
                Value::Integer(1.into()),
                Value::Integer((-37).into()),
            )]),
            Value::Null,
            Value::Bytes(vec![]),
        ]);
        ciborium::ser::into_writer(&cose_value, &mut encoded).unwrap();
        assert!(matches!(
            cose(b"claim", &encoded),
            Err("duplicate_cose_header")
        ));
        let mut protected = Vec::new();
        ciborium::ser::into_writer(
            &Value::Map(vec![
                (Value::Integer(1.into()), Value::Integer((-7).into())),
                (
                    Value::Integer(2.into()),
                    Value::Array(vec![Value::Integer(999.into())]),
                ),
                (Value::Integer(999.into()), Value::Bool(true)),
            ]),
            &mut protected,
        )
        .unwrap();
        let mut encoded = Vec::new();
        ciborium::ser::into_writer(
            &Value::Array(vec![
                Value::Bytes(protected),
                Value::Map(vec![]),
                Value::Null,
                Value::Bytes(vec![]),
            ]),
            &mut encoded,
        )
        .unwrap();
        assert!(matches!(
            cose(b"claim", &encoded),
            Err("unsupported_critical_cose_header")
        ));
        let report = verify(
            sample(),
            ParserLimits {
                max_allocation_bytes: 1,
                ..Default::default()
            },
        );
        assert_eq!(report["signature_integrity"], "resource_limit");
        assert!(merged(vec![(20, 10), (25, 10)]).is_err());
        assert!(merged(vec![(u64::MAX, 2)]).is_err());
    }
    #[test]
    fn supported_cose_algorithms_verify_claim_bytes_with_the_correct_key_type() {
        let rsa = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
        for algorithm in -39..=-37 {
            let signature = signed_cose(algorithm, &rsa);
            assert!(cose(b"claim", &signature).is_ok());
            assert!(cose(b"tampered", &signature).is_err());
        }
        for (algorithm, curve) in [
            (-7, Nid::X9_62_PRIME256V1),
            (-35, Nid::SECP384R1),
            (-36, Nid::SECP521R1),
        ] {
            let key = PKey::from_ec_key(
                EcKey::generate(&EcGroup::from_curve_name(curve).unwrap()).unwrap(),
            )
            .unwrap();
            let signature = signed_cose(algorithm, &key);
            assert!(cose(b"claim", &signature).is_ok());
            assert!(cose(b"tampered", &signature).is_err());
        }
        let key = PKey::generate_ed25519().unwrap();
        let signature = signed_cose(-8, &key);
        assert!(cose(b"claim", &signature).is_ok());
        assert!(cose(b"tampered", &signature).is_err());
    }
}
