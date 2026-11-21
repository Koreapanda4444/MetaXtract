use std::{fmt, io};
use zeroize::Zeroizing;

/// A session-only credential. Deliberately has no serialization or plaintext Debug.
pub struct PdfPassword(Zeroizing<Vec<u8>>);
impl PdfPassword {
    pub fn new(bytes: Zeroizing<Vec<u8>>) -> io::Result<Self> {
        if bytes.len() > 16384 || bytes.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid PDF password length or NUL",
            ));
        }
        Ok(Self(bytes))
    }
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.0
    }
}
impl fmt::Debug for PdfPassword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PdfPassword([redacted])")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credential_debug_is_redacted_and_invalid_inputs_are_rejected() {
        let password = PdfPassword::new(Zeroizing::new(b"never-print-this".to_vec())).unwrap();
        assert!(!format!("{password:?}").contains("never-print-this"));
        assert!(PdfPassword::new(Zeroizing::new(vec![0])).is_err());
        assert!(PdfPassword::new(Zeroizing::new(vec![1; 16385])).is_err());
    }
}
