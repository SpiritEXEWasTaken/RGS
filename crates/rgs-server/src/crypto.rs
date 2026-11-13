//! Password hashing, random tokens, and encryption of secrets at rest.

use argon2::password_hash::{rand_core::OsRng as HashRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::path::Path;

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut HashRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| anyhow::anyhow!("hashing password: {e}"))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|parsed| Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
        .unwrap_or(false)
}

/// A random, URL-safe token with 256 bits of entropy.
pub fn new_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Tokens and codes are stored as SHA-256 hashes; a database leak doesn't hand out logins.
pub fn sha256_hex(value: &str) -> String {
    Sha256::digest(value.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// A short code for people to type, from an alphabet without look-alikes (no 0/O, 1/I/L).
pub fn human_code(len: usize) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
    let mut rng = rand::thread_rng();
    (0..len).map(|_| ALPHABET[(rng.next_u32() as usize) % ALPHABET.len()] as char).collect()
}

/// Encrypts small secrets (2FA keys) with the server key before they go in the database.
#[derive(Clone)]
pub struct SecretBox {
    cipher: ChaCha20Poly1305,
}

impl SecretBox {
    /// Loads the 32-byte server key from `secret.key` in the data folder, creating it on first run.
    /// Back this file up: without it, 2FA secrets can't be read and everyone has to set 2FA up again.
    pub fn load_or_create(data_dir: &Path) -> anyhow::Result<Self> {
        let path = data_dir.join("secret.key");
        let key = match std::fs::read_to_string(&path) {
            Ok(text) => STANDARD.decode(text.trim())?,
            Err(_) => {
                let mut key = vec![0u8; 32];
                rand::thread_rng().fill_bytes(&mut key);
                std::fs::write(&path, STANDARD.encode(&key))?;
                // Only the account running the server may read it.
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
                }
                tracing::info!("created a new server key at {}", path.display());
                key
            }
        };
        anyhow::ensure!(key.len() == 32, "secret.key must hold 32 bytes");
        Ok(Self { cipher: ChaCha20Poly1305::new(Key::from_slice(&key)) })
    }

    pub fn seal(&self, plain: &[u8]) -> anyhow::Result<String> {
        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
        let mut out = nonce.to_vec();
        out.extend(self.cipher.encrypt(&nonce, plain).map_err(|_| anyhow::anyhow!("encrypt failed"))?);
        Ok(STANDARD.encode(out))
    }

    pub fn open(&self, sealed: &str) -> anyhow::Result<Vec<u8>> {
        let bytes = STANDARD.decode(sealed)?;
        anyhow::ensure!(bytes.len() > 12, "sealed value too short");
        let (nonce, ct) = bytes.split_at(12);
        self.cipher.decrypt(Nonce::from_slice(nonce), ct).map_err(|_| anyhow::anyhow!("decrypt failed"))
    }
}
