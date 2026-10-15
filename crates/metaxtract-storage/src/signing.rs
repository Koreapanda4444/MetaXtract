use crate::{CaseWorkspace, ManifestSignature, Result, db, hex, invalid, now_ms, text};
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{
    Signer, SigningKey, VerifyingKey,
    pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey},
};
use pkcs8::{EncryptedPrivateKeyInfo, LineEnding, SecretDocument, pkcs5::pbes2::Kdf};
use rand_core::{OsRng, RngCore};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::Path,
};
use zeroize::Zeroizing;

#[derive(Clone, Debug)]
pub struct KeyDerivationLimits {
    pub max_scrypt_memory_bytes: u64,
    pub max_scrypt_work: u64,
    pub max_pbkdf2_iterations: u32,
}
impl Default for KeyDerivationLimits {
    fn default() -> Self {
        Self {
            max_scrypt_memory_bytes: 128 * 1024 * 1024,
            max_scrypt_work: 8 * 1024 * 1024,
            max_pbkdf2_iterations: 5_000_000,
        }
    }
}

pub struct SigningIdentity {
    name: String,
    key: SigningKey,
}
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct NamedIdentity {
    pub name: String,
    pub fingerprint: String,
    pub public_pem: String,
    pub created_ms: i64,
    pub updated_ms: i64,
}
impl SigningIdentity {
    pub fn generate(name: &str) -> Result<Self> {
        name_valid(name)?;
        let mut bytes = Zeroizing::new([0u8; 32]);
        OsRng
            .try_fill_bytes(bytes.as_mut())
            .map_err(|_| invalid("operating system random generator unavailable"))?;
        Ok(Self {
            name: name.trim().into(),
            key: SigningKey::from_bytes(&bytes),
        })
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn fingerprint(&self) -> String {
        hex(&Sha256::digest(self.key.verifying_key().as_bytes()))
    }
    pub fn public_pem(&self) -> Result<String> {
        self.key
            .verifying_key()
            .to_public_key_pem(LineEnding::LF)
            .map_err(|_| invalid("public key encoding failed"))
    }
    pub fn encrypted_pem(&self, password: &str) -> Result<Zeroizing<String>> {
        password_valid(password)?;
        let der = self
            .key
            .to_pkcs8_der()
            .map_err(|_| invalid("private key encoding failed"))?;
        let mut info = pkcs8::PrivateKeyInfo::try_from(der.as_bytes())
            .map_err(|_| invalid("private key encoding failed"))?;
        // PKCS#8 v1 is accepted by OpenSSL and the frozen Python reference.
        info.public_key = None;
        info.encrypt(OsRng, password)
            .map_err(|_| invalid("private key encryption failed"))?
            .to_pem("ENCRYPTED PRIVATE KEY", LineEnding::LF)
            .map_err(|_| invalid("encrypted key PEM encoding failed"))
    }
    pub fn from_encrypted_pem(
        name: &str,
        pem: &str,
        password: &str,
        limits: &KeyDerivationLimits,
    ) -> Result<Self> {
        name_valid(name)?;
        password_valid(password)?;
        if pem.len() > 64 * 1024 {
            return Err(invalid("encrypted key exceeds byte budget"));
        }
        let (label, der) =
            SecretDocument::from_pem(pem).map_err(|_| invalid("invalid encrypted private key"))?;
        if label != "ENCRYPTED PRIVATE KEY" {
            return Err(invalid("an encrypted PKCS#8 key is required"));
        }
        let encrypted = EncryptedPrivateKeyInfo::try_from(der.as_bytes())
            .map_err(|_| invalid("invalid encrypted PKCS#8 key"))?;
        let params = encrypted
            .encryption_algorithm
            .pbes2()
            .ok_or_else(|| invalid("only PBES2 key encryption is supported"))?;
        match &params.kdf {
            Kdf::Scrypt(p) => {
                let memory = p
                    .cost_parameter
                    .checked_mul(p.block_size as u64)
                    .and_then(|v| v.checked_mul(128));
                let work = p
                    .cost_parameter
                    .checked_mul(p.block_size as u64)
                    .and_then(|v| v.checked_mul(p.parallelization as u64));
                if p.cost_parameter < 2
                    || !p.cost_parameter.is_power_of_two()
                    || p.block_size == 0
                    || p.parallelization == 0
                    || memory.is_none_or(|v| v > limits.max_scrypt_memory_bytes)
                    || work.is_none_or(|v| v > limits.max_scrypt_work)
                {
                    return Err(invalid("scrypt key derivation exceeds resource policy"));
                }
            }
            Kdf::Pbkdf2(p) => {
                if p.iteration_count == 0 || p.iteration_count > limits.max_pbkdf2_iterations {
                    return Err(invalid("PBKDF2 key derivation exceeds resource policy"));
                }
            }
            _ => return Err(invalid("unsupported key derivation algorithm")),
        }
        let decrypted = encrypted
            .decrypt(password)
            .map_err(|_| invalid("password or encrypted private key is invalid"))?;
        let key = SigningKey::from_pkcs8_der(decrypted.as_bytes())
            .map_err(|_| invalid("decrypted key is not a valid Ed25519 key"))?;
        Ok(Self {
            name: name.trim().into(),
            key,
        })
    }
    pub fn import_legacy_pem(name: &str, pem: &str) -> Result<Self> {
        name_valid(name)?;
        if pem.len() > 64 * 1024 {
            return Err(invalid("legacy key exceeds byte budget"));
        }
        let key = SigningKey::from_pkcs8_pem(pem)
            .map_err(|_| invalid("invalid legacy Ed25519 private key"))?;
        Ok(Self {
            name: name.trim().into(),
            key,
        })
    }
    pub fn load_encrypted(
        name: &str,
        path: impl AsRef<Path>,
        password: &str,
        limits: &KeyDerivationLimits,
    ) -> Result<Self> {
        let mut bytes = Zeroizing::new(Vec::new());
        std::fs::File::open(path)?
            .take(64 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        let pem = std::str::from_utf8(&bytes).map_err(|_| invalid("key PEM is not UTF-8"))?;
        Self::from_encrypted_pem(name, pem, password, limits)
    }
    pub fn save_encrypted(&self, path: impl AsRef<Path>, password: &str) -> Result<()> {
        let path = path.as_ref();
        match std::fs::symlink_metadata(path) {
            Ok(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "refusing to replace an existing key",
                ));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let pem = self.encrypted_pem(password)?;
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        temp.write_all(pem.as_bytes())?;
        temp.as_file().sync_all()?;
        temp.persist_noclobber(path).map_err(|e| e.error)?;
        #[cfg(unix)]
        std::fs::File::open(parent)?.sync_all()?;
        Ok(())
    }
    pub fn sign_manifest(&self, manifest: &[u8]) -> ManifestSignature {
        ManifestSignature {
            algorithm: "Ed25519".into(),
            manifest_sha256: hex(&Sha256::digest(manifest)),
            public_key_sha256: self.fingerprint(),
            signature: STANDARD.encode(self.key.sign(manifest).to_bytes()),
        }
    }
}
impl CaseWorkspace {
    pub fn register_identity(&self, identity: &SigningIdentity) -> Result<NamedIdentity> {
        self.register_public_identity(identity.name(), &identity.public_pem()?)
    }
    pub fn register_public_identity(&self, name: &str, pem: &str) -> Result<NamedIdentity> {
        name_valid(name)?;
        if pem.len() > 16 * 1024 {
            return Err(invalid("public PEM exceeds byte budget"));
        }
        let key = VerifyingKey::from_public_key_pem(pem)
            .map_err(|_| invalid("invalid Ed25519 public key"))?;
        let fingerprint = hex(&Sha256::digest(key.as_bytes()));
        let public_pem = key
            .to_public_key_pem(LineEnding::LF)
            .map_err(|_| invalid("public key encoding failed"))?;
        self.write_named("identity.register",|tx|{
            let previous:Option<String>=tx.query_row("SELECT fingerprint FROM identities WHERE name=?1 COLLATE NOCASE",[name.trim()],|r|r.get(0)).optional().map_err(db)?;
            if previous.is_some_and(|p|p!=fingerprint){return Err(invalid("identity name already belongs to another key"));}
            let now=now_ms()?;tx.execute("INSERT INTO identities VALUES(?1,?2,?3,?4,?4) ON CONFLICT(name) DO UPDATE SET public_pem=excluded.public_pem,updated_ms=excluded.updated_ms",params![name.trim(),fingerprint,public_pem,now]).map_err(db)?;
            tx.query_row("SELECT name,fingerprint,public_pem,created_ms,updated_ms FROM identities WHERE name=?1 COLLATE NOCASE",[name.trim()],row_identity).map_err(db)
        })
    }
    pub fn identity(&self, name: &str) -> Result<Option<NamedIdentity>> {
        self.lock()?.query_row("SELECT name,fingerprint,public_pem,created_ms,updated_ms FROM identities WHERE name=?1 COLLATE NOCASE",[name],row_identity).optional().map_err(db)
    }
    pub fn identities(&self, after: Option<&str>, limit: usize) -> Result<Vec<NamedIdentity>> {
        if !(1..=256).contains(&limit) {
            return Err(invalid("identity page must contain 1..256 rows"));
        }
        let conn = self.lock()?;
        let mut q=conn.prepare("SELECT name,fingerprint,public_pem,created_ms,updated_ms FROM identities WHERE ?1 IS NULL OR name>?1 COLLATE NOCASE ORDER BY name COLLATE NOCASE LIMIT ?2").map_err(db)?;
        q.query_map(params![after, limit as i64], row_identity)
            .map_err(db)?
            .map(|r| r.map_err(db))
            .collect()
    }
    pub fn remove_identity(&self, name: &str) -> Result<bool> {
        self.write_named("identity.remove", |tx| {
            tx.execute(
                "DELETE FROM identities WHERE name=?1 COLLATE NOCASE",
                [name],
            )
            .map(|n| n != 0)
            .map_err(db)
        })
    }
}
fn row_identity(row: &rusqlite::Row<'_>) -> rusqlite::Result<NamedIdentity> {
    Ok(NamedIdentity {
        name: row.get(0)?,
        fingerprint: row.get(1)?,
        public_pem: row.get(2)?,
        created_ms: row.get(3)?,
        updated_ms: row.get(4)?,
    })
}
fn name_valid(name: &str) -> Result<()> {
    text(name, 1024, false)?;
    if name.chars().count() > 100 || name.chars().any(char::is_control) {
        return Err(invalid("invalid signing identity name"));
    }
    Ok(())
}
fn password_valid(password: &str) -> Result<()> {
    if password.is_empty() || password.len() > 1024 {
        return Err(invalid("key password must contain 1..1024 bytes"));
    }
    Ok(())
}
