use openssl::{
    error::ErrorStack,
    hash::MessageDigest,
    pkcs7::{Pkcs7, Pkcs7Flags},
    stack::Stack,
    x509::{
        X509, X509ExtendedKeyUsage, X509KeyUsage, X509PurposeId, X509StoreContext,
        store::X509StoreBuilder,
        verify::{X509VerifyFlags, X509VerifyParam},
    },
};
use std::{
    io,
    time::{SystemTime, UNIX_EPOCH},
};

/// Explicit, bounded session policy. No operating-system trust store or network lookup.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PdfTrustPolicy {
    #[serde(default)]
    pub anchors_pem: Vec<u8>,
    #[serde(default)]
    pub evaluation_unix_seconds: Option<i64>,
}
impl PdfTrustPolicy {
    pub fn validate(&self) -> io::Result<()> {
        if self.anchors_pem.len() > 128 * 1024
            || self
                .evaluation_unix_seconds
                .is_some_and(|v| !(-62135596800..=253402300799).contains(&v))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "PDF trust policy exceeds limits",
            ));
        }
        if !self.anchors_pem.is_empty() {
            let mut rest = std::str::from_utf8(&self.anchors_pem)
                .map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "trust anchors must be PEM certificates",
                    )
                })?
                .trim();
            let mut count = 0;
            while !rest.is_empty() {
                let body = rest
                    .strip_prefix("-----BEGIN CERTIFICATE-----")
                    .ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "invalid trust anchor PEM envelope",
                        )
                    })?;
                let (body, tail) =
                    body.split_once("-----END CERTIFICATE-----")
                        .ok_or_else(|| {
                            io::Error::new(
                                io::ErrorKind::InvalidInput,
                                "incomplete trust anchor PEM",
                            )
                        })?;
                if body.len() > 32 * 1024
                    || !body.bytes().all(|b| {
                        b.is_ascii_whitespace() || b.is_ascii_alphanumeric() || b"+/=".contains(&b)
                    })
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "invalid trust anchor PEM encoding",
                    ));
                }
                count += 1;
                if count > 16 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "too many trust anchors",
                    ));
                }
                rest = tail.trim();
            }
        }
        Ok(())
    }
}
pub(crate) struct CertificateAssessment {
    pub status: &'static str,
    pub fields: Vec<(String, String)>,
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn name(name: &openssl::x509::X509NameRef) -> String {
    name.entries()
        .take(32)
        .map(|entry| {
            let key = entry.object().nid().short_name().unwrap_or("OID");
            let value = entry
                .data()
                .to_string()
                .unwrap_or_else(|_| hex(entry.data().as_slice()));
            format!("{key}={}", value.chars().take(4096).collect::<String>())
        })
        .collect::<Vec<_>>()
        .join(", ")
}
fn evaluate(der: &[u8], policy: &PdfTrustPolicy) -> Result<CertificateAssessment, ErrorStack> {
    let now = policy.evaluation_unix_seconds.unwrap_or_else(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs().min(i64::MAX as u64) as i64)
    });
    let pkcs7 = Pkcs7::from_der(der)?;
    let empty = Stack::new()?;
    let signers = pkcs7.signers(&empty, Pkcs7Flags::empty())?;
    let certificates = pkcs7.signed().and_then(|s| s.certificates());
    let mut out=CertificateAssessment{status:"unknown_no_trust_anchors",fields:vec![
        ("certificate_evaluation_unix_seconds".into(),now.to_string()),
        ("revocation_status".into(),"unknown_offline_not_checked".into()),
        ("timestamp_status".into(),"not_validated; signing_time_is_a_claim".into()),
        ("certificate_policy_scope".into(),"strict_X509_path_at_evaluation_time; key_usage; restricted_EKU_requires_separate_document_policy; no_legal_identity_assurance".into()),
    ]};
    if signers.is_empty() || signers.len() > 16 || certificates.is_some_and(|c| c.len() > 64) {
        out.status = "unknown_certificate_limits";
        return Ok(out);
    }
    let mut store = X509StoreBuilder::new()?;
    let mut param = X509VerifyParam::new()?;
    param.set_depth(8);
    param.set_auth_level(2);
    #[cfg(target_pointer_width = "32")]
    if !(i64::from(i32::MIN)..=i64::from(i32::MAX)).contains(&now) {
        return Err(ErrorStack::get());
    }
    param.set_time(now as _);
    param.set_flags(X509VerifyFlags::X509_STRICT | X509VerifyFlags::TRUSTED_FIRST)?;
    store.set_param(&param)?;
    store.set_purpose(X509PurposeId::ANY)?;
    if !policy.anchors_pem.is_empty() {
        let roots = X509::stack_from_pem(&policy.anchors_pem)?;
        if roots.is_empty() || roots.len() > 16 {
            out.status = "unknown_invalid_trust_configuration";
            return Ok(out);
        }
        for (index, root) in roots.into_iter().enumerate() {
            out.fields.push((
                format!("trust_anchor{index}.sha256"),
                hex(&root.digest(MessageDigest::sha256())?),
            ));
            store.add_cert(root)?;
        }
        out.status = "chain_valid_to_configured_anchor";
    }
    let store = store.build();
    let mut chain = Stack::new()?;
    if let Some(certificates) = certificates {
        for cert in certificates {
            chain.push(cert.to_owned())?;
        }
    }
    for (index, cert) in signers.iter().enumerate() {
        let prefix = format!("signer{index}");
        for (key, value) in [
            ("sha256", hex(&cert.digest(MessageDigest::sha256())?)),
            ("subject", name(cert.subject_name())),
            ("issuer", name(cert.issuer_name())),
            ("not_before", cert.not_before().to_string()),
            ("not_after", cert.not_after().to_string()),
            (
                "serial",
                cert.serial_number().to_bn()?.to_hex_str()?.to_string(),
            ),
        ] {
            out.fields.push((format!("{prefix}.{key}"), value));
        }
        if policy.anchors_pem.is_empty() {
            continue;
        }
        let mut context = X509StoreContext::new()?;
        let (valid, error, path) = context.init(&store, cert, &chain, |ctx| {
            let valid = ctx.verify_cert()?;
            let path = ctx
                .chain()
                .map(|chain| {
                    chain
                        .iter()
                        .map(|cert| cert.digest(MessageDigest::sha256()).map(|d| hex(&d)))
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?
                .unwrap_or_default();
            Ok((valid, ctx.error(), path))
        })?;
        out.fields.push((
            format!("{prefix}.chain_result"),
            error.error_string().into(),
        ));
        out.fields.push((
            format!("{prefix}.chain_error_code"),
            error.as_raw().to_string(),
        ));
        out.fields
            .push((format!("{prefix}.chain_sha256"), path.join(" -> ")));
        if !valid {
            out.status = "untrusted_or_invalid_chain";
        } else if cert.key_usage().is_some_and(|usage| {
            !usage.intersects(X509KeyUsage::DIGITAL_SIGNATURE | X509KeyUsage::NON_REPUDIATION)
        }) {
            out.status = "invalid_signing_key_usage";
        } else if cert
            .extended_key_usage()
            .is_some_and(|usage| !usage.contains(X509ExtendedKeyUsage::ANYEKU))
            && out.status == "chain_valid_to_configured_anchor"
        {
            out.status = "unknown_restricted_extended_key_usage";
        }
    }
    Ok(out)
}
pub(crate) fn assess_certificates(der: &[u8], policy: &PdfTrustPolicy) -> CertificateAssessment {
    if policy.validate().is_err() {
        return CertificateAssessment {
            status: "unknown_invalid_trust_configuration",
            fields: vec![],
        };
    }
    evaluate(der, policy).unwrap_or_else(|_| CertificateAssessment {
        status: "unknown_certificate_evaluation_failed",
        fields: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use openssl::{
        asn1::Asn1Time,
        bn::BigNum,
        cms::{CMSOptions, CmsContentInfo},
        hash::MessageDigest,
        pkey::{PKey, Private},
        rsa::Rsa,
        x509::{
            X509NameBuilder,
            extension::{
                AuthorityKeyIdentifier, BasicConstraints, ExtendedKeyUsage, KeyUsage,
                SubjectKeyIdentifier,
            },
        },
    };
    fn certificate(
        label: &str,
        issuer: Option<(&X509, &PKey<Private>)>,
        ca: bool,
        signing: bool,
        restricted: bool,
    ) -> (X509, PKey<Private>) {
        let key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
        let mut name = X509NameBuilder::new().unwrap();
        name.append_entry_by_text("CN", label).unwrap();
        let name = name.build();
        let mut cert = X509::builder().unwrap();
        cert.set_version(2).unwrap();
        cert.set_serial_number(
            &BigNum::from_u32(if ca { 1 } else { 2 })
                .unwrap()
                .to_asn1_integer()
                .unwrap(),
        )
        .unwrap();
        cert.set_subject_name(&name).unwrap();
        cert.set_issuer_name(issuer.map_or(&*name, |i| i.0.subject_name()))
            .unwrap();
        cert.set_pubkey(&key).unwrap();
        cert.set_not_before(&Asn1Time::from_unix(1600000000).unwrap())
            .unwrap();
        cert.set_not_after(&Asn1Time::from_unix(2000000000).unwrap())
            .unwrap();
        let constraints = if ca {
            BasicConstraints::new().critical().ca().build().unwrap()
        } else {
            BasicConstraints::new().critical().build().unwrap()
        };
        cert.append_extension(constraints).unwrap();
        let usage = if ca {
            KeyUsage::new()
                .critical()
                .key_cert_sign()
                .crl_sign()
                .build()
                .unwrap()
        } else if signing {
            KeyUsage::new()
                .critical()
                .digital_signature()
                .build()
                .unwrap()
        } else {
            KeyUsage::new()
                .critical()
                .key_encipherment()
                .build()
                .unwrap()
        };
        cert.append_extension(usage).unwrap();
        let ski = SubjectKeyIdentifier::new()
            .build(&cert.x509v3_context(issuer.map(|i| &**i.0), None))
            .unwrap();
        cert.append_extension(ski).unwrap();
        if let Some((issuer, _)) = issuer {
            let aki = AuthorityKeyIdentifier::new()
                .keyid(true)
                .build(&cert.x509v3_context(Some(issuer), None))
                .unwrap();
            cert.append_extension(aki).unwrap();
        }
        if restricted {
            cert.append_extension(ExtendedKeyUsage::new().server_auth().build().unwrap())
                .unwrap();
        }
        cert.sign(issuer.map_or(&key, |i| i.1), MessageDigest::sha256())
            .unwrap();
        (cert.build(), key)
    }
    fn signed(leaf: &X509, key: &PKey<Private>, chain: &[&X509]) -> Vec<u8> {
        let mut certs = Stack::new().unwrap();
        for cert in chain {
            certs.push((*cert).clone()).unwrap();
        }
        CmsContentInfo::sign(
            Some(leaf),
            Some(key),
            Some(&certs),
            Some(b"certificate trust test"),
            CMSOptions::DETACHED | CMSOptions::BINARY,
        )
        .unwrap()
        .to_der()
        .unwrap()
    }
    #[test]
    fn explicit_anchor_and_complete_chain_are_required_at_the_recorded_time() {
        let (root, root_key) = certificate("Test root", None, true, true, false);
        let (intermediate, intermediate_key) = certificate(
            "Test intermediate",
            Some((&root, &root_key)),
            true,
            true,
            false,
        );
        let (leaf, key) = certificate(
            "Test signer",
            Some((&intermediate, &intermediate_key)),
            false,
            true,
            false,
        );
        let cms = signed(&leaf, &key, &[&intermediate, &root]);
        let mut policy = PdfTrustPolicy {
            anchors_pem: root.to_pem().unwrap(),
            evaluation_unix_seconds: Some(1800000000),
        };
        let result = assess_certificates(&cms, &policy);
        assert_eq!(
            result.status, "chain_valid_to_configured_anchor",
            "{:?}",
            result.fields
        );
        assert!(
            result
                .fields
                .iter()
                .any(|(k, v)| k == "revocation_status" && v.starts_with("unknown"))
        );
        assert_eq!(
            assess_certificates(&cms, &PdfTrustPolicy::default()).status,
            "unknown_no_trust_anchors"
        );
        assert_eq!(
            assess_certificates(&signed(&leaf, &key, &[]), &policy).status,
            "untrusted_or_invalid_chain"
        );
        policy.evaluation_unix_seconds = Some(2100000000);
        let expired = assess_certificates(&cms, &policy);
        assert_eq!(expired.status, "untrusted_or_invalid_chain");
        assert!(
            expired
                .fields
                .iter()
                .any(|(k, v)| k.ends_with("chain_result") && v.contains("expired"))
        );
        policy.evaluation_unix_seconds = Some(1500000000);
        assert_eq!(
            assess_certificates(&cms, &policy).status,
            "untrusted_or_invalid_chain"
        );
        let (other, _) = certificate("Unrelated root", None, true, true, false);
        policy.anchors_pem = other.to_pem().unwrap();
        policy.evaluation_unix_seconds = Some(1800000000);
        assert_eq!(
            assess_certificates(&cms, &policy).status,
            "untrusted_or_invalid_chain"
        );
    }
    #[test]
    fn signing_usage_restricted_eku_and_malformed_anchors_are_not_trusted() {
        let (root, root_key) = certificate("Test root", None, true, true, false);
        let policy = PdfTrustPolicy {
            anchors_pem: root.to_pem().unwrap(),
            evaluation_unix_seconds: Some(1800000000),
        };
        let (leaf, key) = certificate(
            "Encryption only",
            Some((&root, &root_key)),
            false,
            false,
            false,
        );
        assert_eq!(
            assess_certificates(&signed(&leaf, &key, &[&root]), &policy).status,
            "invalid_signing_key_usage"
        );
        let (leaf, key) = certificate("TLS only", Some((&root, &root_key)), false, true, true);
        assert_eq!(
            assess_certificates(&signed(&leaf, &key, &[&root]), &policy).status,
            "unknown_restricted_extended_key_usage"
        );
        assert!(
            PdfTrustPolicy {
                anchors_pem: b"not a certificate".to_vec(),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            PdfTrustPolicy {
                anchors_pem: vec![b'A'; 128 * 1024 + 1],
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
}
