//! Detached PDF CMS integrity. Trust, timestamps and document truth are separate questions.
use crate::{BoundedInput, ExtractionResult, NativeObservation, ParserLimits, read_input};
use metaxtract_schema::{ExtractionMethod, RawValue, TextValue};
use openssl::cms::{CMSOptions, CmsContentInfo};
use std::collections::BTreeMap;

const MAX_SIGNED_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CMS_BYTES: usize = 2 * 1024 * 1024;

fn number(value: &RawValue) -> Option<u64> {
    match value {
        RawValue::Signed(v) => (*v).try_into().ok(),
        RawValue::Unsigned(v) => Some(*v),
        _ => None,
    }
}
fn text(value: &RawValue) -> Option<&str> {
    match value {
        RawValue::Text(v) if v.lossless => Some(&v.text),
        _ => None,
    }
}

// Only DER framing is interpreted here; OpenSSL owns all CMS and cryptographic parsing.
fn tlv(input: &mut &[u8], tag: u8) -> Option<usize> {
    if input.first().copied()? != tag {
        return None;
    }
    let length = *input.get(1)?;
    let (header, size) = if length < 128 {
        (2, usize::from(length))
    } else {
        let count = usize::from(length & 127);
        if count == 0 || count > 4 || *input.get(2)? == 0 {
            return None;
        }
        let mut size = 0usize;
        for byte in input.get(2..2 + count)? {
            size = size.checked_mul(256)?.checked_add(usize::from(*byte))?;
        }
        if size < 128 {
            return None;
        }
        (2 + count, size)
    };
    if header + size > input.len() {
        return None;
    }
    *input = &input[header..];
    Some(size)
}
fn item<'a>(input: &mut &'a [u8], tag: u8) -> Option<&'a [u8]> {
    let size = tlv(input, tag)?;
    let (value, rest) = input.split_at(size);
    *input = rest;
    Some(value)
}
fn detached_der(padded: &[u8]) -> Option<&[u8]> {
    if padded.len() > MAX_CMS_BYTES {
        return None;
    }
    let mut rest = padded;
    let mut outer = item(&mut rest, 0x30)?;
    let size = padded.len() - rest.len();
    if rest.iter().any(|b| *b != 0) {
        return None;
    }
    if item(&mut outer, 0x06)? != b"\x2a\x86\x48\x86\xf7\x0d\x01\x07\x02" {
        return None;
    }
    let mut explicit = item(&mut outer, 0xa0)?;
    if !outer.is_empty() {
        return None;
    }
    let mut signed = item(&mut explicit, 0x30)?;
    if !explicit.is_empty() {
        return None;
    }
    item(&mut signed, 0x02)?;
    item(&mut signed, 0x31)?;
    let mut encapsulated = item(&mut signed, 0x30)?;
    if item(&mut encapsulated, 0x06)? != b"\x2a\x86\x48\x86\xf7\x0d\x01\x07\x01"
        || !encapsulated.is_empty()
    {
        return None;
    }
    if signed.first() == Some(&0xa0) {
        item(&mut signed, 0xa0)?;
    }
    if signed.first() == Some(&0xa1) {
        item(&mut signed, 0xa1)?;
    }
    let signers = item(&mut signed, 0x31)?;
    if signers.is_empty() || !signed.is_empty() {
        return None;
    }
    Some(&padded[..size])
}

fn observed(namespace: &str, key: &str, value: &str, stream: Option<String>) -> NativeObservation {
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
        stream_id: stream,
    }
}

/// Runs inside the isolated worker. Extra reads, working memory and observations are budgeted.
pub fn verify_pdf_signatures(
    input: &BoundedInput,
    result: &mut ExtractionResult,
    limits: ParserLimits,
) {
    verify_pdf_signatures_with_policy(input, result, limits, &crate::PdfTrustPolicy::default());
}

pub fn verify_pdf_signatures_with_policy(
    input: &BoundedInput,
    result: &mut ExtractionResult,
    limits: ParserLimits,
    policy: &crate::PdfTrustPolicy,
) {
    if result.parser_id != "pdf.security" {
        return;
    }
    let source_size = match read_input(input, 0, 0) {
        Ok(read) => read.source_size,
        Err(_) => return,
    };
    let mut groups: BTreeMap<String, BTreeMap<String, &NativeObservation>> = BTreeMap::new();
    for observation in &result.observations {
        if observation.namespace.starts_with("pdf.signature") {
            groups
                .entry(observation.namespace.clone())
                .or_default()
                .insert(observation.key.clone(), observation);
        }
    }
    let mut added = Vec::new();
    for (namespace, fields) in groups.into_iter().take(32) {
        let get = |key: &str| fields.get(key).and_then(|o| number(&o.value));
        let stream = fields
            .get("signature_object")
            .and_then(|o| o.stream_id.clone());
        let subfilter = fields.get("SubFilter").and_then(|o| text(&o.value));
        let status = if !matches!(
            subfilter,
            Some(
                "/adbe.pkcs7.detached"
                    | "/ETSI.CAdES.detached"
                    | "adbe.pkcs7.detached"
                    | "ETSI.CAdES.detached"
            )
        ) {
            "unsupported_subfilter"
        } else if get("byte_range_items") != Some(4)
            || get("byte_range_valid") != Some(1)
            || get("contents_gap_matches") != Some(1)
            || get("byte_range.0") != Some(0)
        {
            "unverifiable_byte_range"
        } else {
            let ranges = [
                get("byte_range.1"),
                get("byte_range.2"),
                get("byte_range.3"),
            ];
            if let [Some(first), Some(second), Some(last)] = ranges {
                let size = first.saturating_add(last);
                let memory = size.saturating_mul(3).saturating_add(32 * 1024 * 1024);
                if first > second || second > source_size || last > source_size - second {
                    "unverifiable_byte_range"
                } else if size > MAX_SIGNED_BYTES
                    || size > limits.max_read_bytes.saturating_sub(result.run.read_bytes)
                    || memory
                        > limits
                            .max_allocation_bytes
                            .saturating_sub(result.run.allocation_bytes)
                {
                    "resource_limit"
                } else if let Some(RawValue::Bytes(padded)) =
                    fields.get("signature_contents").map(|o| &o.value)
                {
                    if let Some(der) = detached_der(padded) {
                        result.run.allocation_bytes += memory;
                        let mut data = Vec::with_capacity(size as usize);
                        let mut readable = true;
                        for (begin, length) in [(0, first), (second, last)] {
                            let mut at = 0;
                            while at < length {
                                let request = (length - at).min(1024 * 1024) as u32;
                                match read_input(input, begin + at, request) {
                                    Ok(read) if read.bytes.len() == request as usize => {
                                        result.run.read_bytes += u64::from(request);
                                        data.extend_from_slice(&read.bytes);
                                    }
                                    _ => {
                                        readable = false;
                                        break;
                                    }
                                }
                                at += u64::from(request);
                            }
                            if !readable {
                                break;
                            }
                        }
                        if !readable {
                            "source_read_failed"
                        } else if let Ok(mut cms) = CmsContentInfo::from_der(der) {
                            // Verify every signer and the signed attributes/content digest. Only chain trust is skipped.
                            if cms
                                .verify(
                                    None,
                                    None,
                                    Some(&data),
                                    None,
                                    CMSOptions::BINARY | CMSOptions::NO_SIGNER_CERT_VERIFY,
                                )
                                .is_ok()
                            {
                                "valid_for_signed_revision"
                            } else {
                                "invalid"
                            }
                        } else {
                            "malformed_cms"
                        }
                    } else {
                        "malformed_or_non_detached_cms"
                    }
                } else {
                    "missing_cms"
                }
            } else {
                "unverifiable_byte_range"
            }
        };
        let mut certificate_fields = Vec::new();
        let mut certificate_trust = "unknown_not_evaluated";
        if status == "valid_for_signed_revision" {
            if let Some(RawValue::Bytes(padded)) =
                fields.get("signature_contents").map(|o| &o.value)
            {
                if let Some(der) = detached_der(padded) {
                    let evaluation = crate::pdf_certificates::assess_certificates(der, policy);
                    certificate_trust = evaluation.status;
                    certificate_fields = evaluation.fields;
                }
            }
        }
        for (key, value) in [
            ("cms_integrity", status),
            ("certificate_trust", certificate_trust),
            ("cms_crypto_library", openssl::version::version()),
            (
                "verification_scope",
                "detached_CMS_and_ByteRange_only; no_timestamp_revocation_or_content_truth_check",
            ),
        ] {
            if result.run.observations >= limits.max_observations {
                break;
            }
            added.push(observed(&namespace, key, value, stream.clone()));
            result.run.observations += 1;
        }
        for (key, value) in certificate_fields {
            if result.run.observations >= limits.max_observations {
                break;
            }
            added.push(observed(&namespace, &key, &value, stream.clone()));
            result.run.observations += 1;
        }
    }
    result.observations.extend(added);
}

#[cfg(test)]
mod tests {
    use super::*;
    use openssl::{
        asn1::Asn1Time,
        hash::MessageDigest,
        pkey::PKey,
        rsa::Rsa,
        x509::{X509, X509NameBuilder},
    };

    pub(super) fn signed_pdf(detached: bool) -> Vec<u8> {
        let key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
        let mut name = X509NameBuilder::new().unwrap();
        name.append_entry_by_text("CN", "Synthetic PDF test signer")
            .unwrap();
        let name = name.build();
        let mut cert = X509::builder().unwrap();
        cert.set_version(2).unwrap();
        cert.set_subject_name(&name).unwrap();
        cert.set_issuer_name(&name).unwrap();
        cert.set_pubkey(&key).unwrap();
        cert.set_not_before(&Asn1Time::days_from_now(0).unwrap())
            .unwrap();
        cert.set_not_after(&Asn1Time::days_from_now(30).unwrap())
            .unwrap();
        cert.sign(&key, MessageDigest::sha256()).unwrap();
        let cert = cert.build();
        let objects = [b"<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] >> >>".to_vec(),
            b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>".to_vec(),
            b"<< /FT /Sig /T (Signature) /V 5 0 R >>".to_vec(),
            format!("<< /Type /Sig /Filter /Adobe.PPKLite /SubFilter /adbe.pkcs7.detached /ByteRange [0 1111111111 2222222222 3333333333] /Contents <{}> >>", "0".repeat(16384)).into_bytes()];
        let mut pdf = b"%PDF-1.7\n".to_vec();
        let mut offsets = vec![0];
        for (i, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            pdf.extend(object);
            pdf.extend_from_slice(b"\nendobj\n");
        }
        let xref = pdf.len();
        pdf.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
        for offset in offsets.iter().skip(1) {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!("trailer << /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
        );
        let contents = pdf.windows(11).position(|s| s == b"/Contents <").unwrap() + 10;
        let end = contents + 16386;
        for (placeholder, actual) in [
            (b"1111111111", contents),
            (b"2222222222", end),
            (b"3333333333", pdf.len() - end),
        ] {
            let at = pdf.windows(10).position(|s| s == placeholder).unwrap();
            pdf[at..at + 10].copy_from_slice(format!("{actual:010}").as_bytes());
        }
        let mut data = pdf[..contents].to_vec();
        data.extend_from_slice(&pdf[end..]);
        let flags = CMSOptions::BINARY
            | if detached {
                CMSOptions::DETACHED
            } else {
                CMSOptions::empty()
            };
        let cms = CmsContentInfo::sign(Some(&cert), Some(&key), None, Some(&data), flags)
            .unwrap()
            .to_der()
            .unwrap();
        let hex: String = cms.iter().map(|b| format!("{b:02x}")).collect();
        assert!(hex.len() <= 16384);
        pdf[contents + 1..contents + 1 + hex.len()].copy_from_slice(hex.as_bytes());
        pdf
    }
    fn check(bytes: Vec<u8>, limits: ParserLimits) -> String {
        let input = BoundedInput::from_bytes(bytes, crate::InputLimits::default()).unwrap();
        let mut result =
            crate::extract_parser(&input, "pdf.security", ParserLimits::default()).unwrap();
        assert!(result.error.is_none(), "{:?}", result.error);
        verify_pdf_signatures(&input, &mut result, limits);
        result
            .observations
            .iter()
            .find(|o| o.key == "cms_integrity")
            .and_then(|o| text(&o.value))
            .unwrap()
            .into()
    }
    #[test]
    fn real_cms_binds_the_exact_pdf_revision_and_detects_tampering() {
        let pdf = signed_pdf(true);
        assert_eq!(
            check(pdf.clone(), ParserLimits::default()),
            "valid_for_signed_revision"
        );
        let mut changed = pdf.clone();
        let at = changed.windows(9).position(|s| s == b"Signature").unwrap();
        changed[at] = b'X';
        assert_eq!(check(changed, ParserLimits::default()), "invalid");
        let mut tail = pdf;
        tail.extend_from_slice(b"\n% unsigned tail\n");
        assert_eq!(
            check(tail, ParserLimits::default()),
            "valid_for_signed_revision"
        );
        assert_eq!(
            check(signed_pdf(false), ParserLimits::default()),
            "malformed_or_non_detached_cms"
        );
    }
    #[test]
    fn cms_framing_and_working_memory_fail_closed() {
        assert!(detached_der(b"\x30\x80\0\0").is_none());
        assert!(detached_der(b"\x30\x81\x01\0").is_none());
        assert_eq!(
            check(
                signed_pdf(true),
                ParserLimits {
                    max_allocation_bytes: 1,
                    ..Default::default()
                }
            ),
            "resource_limit"
        );
    }
}
