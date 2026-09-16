use std::fmt;

use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug)]
pub struct DigestError(serde_json::Error);

impl fmt::Display for DigestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("value cannot be represented by RFC 8785 JCS")
    }
}

impl std::error::Error for DigestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

#[allow(clippy::missing_errors_doc)]
pub fn canonicalize_jcs<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, DigestError> {
    serde_jcs::to_vec(value).map_err(DigestError)
}

#[allow(clippy::missing_errors_doc)]
pub fn jcs_sha256<T: Serialize + ?Sized>(value: &T) -> Result<String, DigestError> {
    let canonical = canonicalize_jcs(value)?;
    let digest = Sha256::digest(canonical);
    let mut output = String::with_capacity(7 + digest.len() * 2);
    output.push_str("sha256:");
    for byte in digest {
        use std::fmt::Write;
        write!(output, "{byte:02x}").expect("writing to String cannot fail");
    }
    Ok(output)
}
