use crate::{Result, hex, invalid};
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, VerifyingKey, pkcs8::DecodePublicKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct ManifestSignature {
    pub algorithm: String,
    pub manifest_sha256: String,
    pub public_key_sha256: String,
    pub signature: String,
}
pub fn verify_manifest(
    manifest: &[u8],
    signature: &ManifestSignature,
    trusted_public_pem: &str,
) -> Result<String> {
    if trusted_public_pem.len() > 16 * 1024 {
        return Err(invalid("public key is too large"));
    }
    let key = VerifyingKey::from_public_key_pem(trusted_public_pem)
        .map_err(|_| invalid("invalid Ed25519 public key"))?;
    let fingerprint = hex(&Sha256::digest(key.as_bytes()));
    if signature.algorithm != "Ed25519"
        || signature.manifest_sha256 != hex(&Sha256::digest(manifest))
        || signature.public_key_sha256 != fingerprint
    {
        return Err(invalid("manifest hash, algorithm or trusted key differs"));
    }
    if signature.signature.len() > 128 {
        return Err(invalid("invalid signature encoding"));
    }
    let bytes = STANDARD
        .decode(&signature.signature)
        .map_err(|_| invalid("invalid signature encoding"))?;
    let signature =
        Signature::from_slice(&bytes).map_err(|_| invalid("invalid Ed25519 signature length"))?;
    key.verify_strict(manifest, &signature)
        .map_err(|_| invalid("manifest signature verification failed"))?;
    Ok(fingerprint)
}
