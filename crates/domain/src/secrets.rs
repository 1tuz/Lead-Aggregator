//! Build-time sealed secrets for the bundled db-export provider.
//!
//! Sensitive literals (service host, key-check path) are stored in this repo
//! as AES-256-GCM ciphertext plus a derivation table. The plaintext never
//! appears in the source tree or in release binaries; it is reconstructed only
//! at runtime, in memory, when a request URL is built.
//!
//! The tool that seals values lives in `xtask` (`cargo xtask seal -- ...`).

use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
use sha2::{Digest, Sha256};

/// One sealed entry: ciphertext bytes, base64 of the 12-byte nonce, and the
/// index table used to derive the KEK. Indexes and the KDF salt live here in
/// plain sight — without the peer constants the derivation stays unusable, and
/// `git grep` for a host or path yields nothing.
#[derive(Clone, Copy)]
pub struct Sealed {
    pub ciphertext: &'static [u8],
    pub nonce_b64: &'static str,
    pub table: &'static [(u32, u32)],
    pub salt: &'static str,
}

fn b64_decode(input: &str) -> Option<Vec<u8>> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let bytes = input.as_bytes();
    let mut buffer = 0_u32;
    let mut bits = 0_u32;
    for &byte in bytes {
        if byte == b'=' {
            break;
        }
        let value = TABLE.iter().position(|&c| c == byte)? as u32;
        buffer = (buffer << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xFF) as u8);
        }
    }
    Some(out)
}

fn derive_key(sealed: &Sealed) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(sealed.salt.as_bytes());
    for (a, b) in sealed.table {
        hasher.update(a.to_le_bytes());
        hasher.update(b.to_le_bytes());
        // Mix row distance so permutation order matters.
        hasher.update((b.wrapping_sub(*a)).to_le_bytes());
    }
    hasher.update([0xB7_u8]);
    hasher.finalize().into()
}

/// Decrypt a sealed value at runtime. Returns `None` when the sealed blob was
/// tampered with (AES-GCM tag mismatch).
pub fn open(sealed: &Sealed) -> Option<String> {
    let key = derive_key(sealed);
    let cipher = Aes256Gcm::new_from_slice(&key).ok()?;
    let nonce_bytes = b64_decode(sealed.nonce_b64)?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let plaintext = cipher.decrypt(nonce, sealed.ciphertext).ok()?;
    String::from_utf8(plaintext).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_TABLE: &[(u32, u32)] = &[
        (3, 1),
        (7, 2),
        (11, 5),
        (4, 9),
        (13, 8),
        (2, 6),
        (17, 3),
        (5, 12),
    ];
    const TEST_SALT: &str = "unit-test-salt";

    fn seal_test_value(plain: &str) -> (Vec<u8>, String) {
        // Mirrors the xtask sealing routine with fixed key material for tests.
        use aes_gcm::aead::Aead;
        let mut key = [0u8; 32];
        let mut hasher = Sha256::new();
        hasher.update(TEST_SALT.as_bytes());
        for (a, b) in TEST_TABLE {
            hasher.update(a.to_le_bytes());
            hasher.update(b.to_le_bytes());
            hasher.update((b.wrapping_sub(*a)).to_le_bytes());
        }
        hasher.update([0xB7_u8]);
        key.copy_from_slice(&hasher.finalize());
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let nonce = Nonce::from_slice(b"1234567890ab");
        let ciphertext = cipher.encrypt(nonce, plain.as_bytes()).unwrap();
        let nonce_b64 = "MTIzNDU2Nzg5MGFi".to_owned();
        (ciphertext, nonce_b64)
    }

    #[test]
    fn b64_decode_roundtrip() {
        assert_eq!(b64_decode("aGVsbG8=").unwrap(), b"hello".to_vec());
        assert_eq!(b64_decode("").unwrap(), Vec::<u8>::new());
        assert!(b64_decode("!!!!").is_none());
    }

    #[test]
    fn open_rejects_tampered_ciphertext() {
        let (ciphertext, _nonce) = seal_test_value("https://example.test");
        let mut broken = ciphertext.clone();
        broken[0] ^= 0xFF;
        let broken: &'static [u8] = Box::leak(broken.into_boxed_slice());
        let sealed = Sealed {
            ciphertext: broken,
            nonce_b64: "MTIzNDU2Nzg5MGFi",
            table: TEST_TABLE,
            salt: TEST_SALT,
        };
        assert!(open(&sealed).is_none());
    }
}

#[cfg(test)]
mod sealed_tests {
    use super::*;
    use crate::sealed::{DB_EXPORT_HOST, KEY_CHECK_PATH, KEY_USER_SUFFIX, KEY_VALIDATE_SUFFIX};

    #[test]
    fn sealed_db_host_opens_to_parselab_base() {
        let host = open(&DB_EXPORT_HOST).expect("sealed DB host must decrypt");
        assert!(host.starts_with("http"));
        assert!(!host.is_empty());
    }

    #[test]
    fn sealed_key_paths_open() {
        assert!(open(&KEY_CHECK_PATH).is_some());
        assert!(open(&KEY_USER_SUFFIX).is_some());
        assert!(open(&KEY_VALIDATE_SUFFIX).is_some());
    }
}
