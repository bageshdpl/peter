//! Encrypting values kept in the database, such as a workspace's AI key.
//!
//! XChaCha20-Poly1305 with a random nonce per value. The key comes from
//! `PETER_SECRET_KEY` (any long random text), or else from a file Peter
//! creates in its data folder on first start. On hosts that wipe the disk
//! (Render's free plan), set `PETER_SECRET_KEY`, or stored keys cannot be read
//! after a restart.

use base64::{Engine, engine::general_purpose::STANDARD};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, Generate, KeyInit},
};
use sha2::{Digest, Sha256};

pub struct Secret([u8; 32]);

const PREFIX: &str = "v1:";

impl Secret {
    pub fn from_text(text: &str) -> Self {
        Self(Sha256::digest(text.as_bytes()).into())
    }

    /// From `PETER_SECRET_KEY`, or the data folder's `secret.key` (made if missing).
    pub async fn load(data_dir: &std::path::Path) -> std::io::Result<Self> {
        if let Some(text) = std::env::var("PETER_SECRET_KEY").ok().filter(|v| v.trim().len() >= 16) {
            return Ok(Self::from_text(text.trim()));
        }
        let path = data_dir.join("secret.key");
        tracing::info!(path = %path.display(), "PETER_SECRET_KEY is not set; using the secret key file in the data folder");
        match tokio::fs::read_to_string(&path).await {
            Ok(text) if text.trim().len() >= 16 => Ok(Self::from_text(text.trim())),
            _ => {
                tokio::fs::create_dir_all(data_dir).await?;
                let text = crate::security::new_token();
                tokio::fs::write(&path, &text).await?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).await;
                }
                Ok(Self::from_text(&text))
            }
        }
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new(&self.0.into())
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<String, String> {
        let nonce = XNonce::generate();
        let sealed = self.cipher().encrypt(&nonce, plaintext.as_bytes()).map_err(|_| "could not encrypt".to_string())?;
        let mut out = nonce.to_vec();
        out.extend_from_slice(&sealed);
        Ok(format!("{PREFIX}{}", STANDARD.encode(out)))
    }

    pub fn decrypt(&self, stored: &str) -> Result<String, String> {
        let bytes = stored.strip_prefix(PREFIX).and_then(|b| STANDARD.decode(b).ok()).ok_or("not an encrypted value")?;
        if bytes.len() < 24 + 16 {
            return Err("the encrypted value is too short".into());
        }
        let (nonce, sealed) = bytes.split_at(24);
        let nonce = XNonce::try_from(nonce).map_err(|_| "bad nonce".to_string())?;
        let plain = self
            .cipher()
            .decrypt(&nonce, sealed)
            .map_err(|_| "it was encrypted with a different secret key (was PETER_SECRET_KEY changed?)".to_string())?;
        String::from_utf8(plain).map_err(|_| "not text".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_tamper_detection() {
        let s = Secret::from_text("a long secret for testing only");
        let a = s.encrypt("sk-or-v1-abc").unwrap();
        let b = s.encrypt("sk-or-v1-abc").unwrap();
        assert_ne!(a, b, "nonces must differ");
        assert!(!a.contains("sk-or"));
        assert_eq!(s.decrypt(&a).unwrap(), "sk-or-v1-abc");
        let other = Secret::from_text("a different secret entirely");
        assert!(other.decrypt(&a).unwrap_err().contains("different secret key"));
        let mut raw = STANDARD.decode(a.strip_prefix(PREFIX).unwrap()).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 1;
        assert!(s.decrypt(&format!("{PREFIX}{}", STANDARD.encode(raw))).is_err());
    }
}
