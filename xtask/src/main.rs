//! `cargo xtask seal -- <value>` seals a secret literal into AES-256-GCM
//! ciphertext for `crates/domain/src/secrets_data.rs`.
//!
//! Usage:
//!   cargo xtask seal -- "https://host.tld"        # prints a Sealed block
//!   cargo xtask seal --name DB_HOST -- "https://…"
//!
//! The KEK is derived from a fresh random index table + salt per seal, so the
//! same plaintext never produces the same blob twice.

use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use rand::RngCore;
use sha2::{Digest, Sha256};

fn derive_key(salt: &str, table: &[(u32, u32)]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(salt.as_bytes());
    for (a, b) in table {
        hasher.update(a.to_le_bytes());
        hasher.update(b.to_le_bytes());
        hasher.update((b.wrapping_sub(*a)).to_le_bytes());
    }
    hasher.update([0xB7_u8]);
    hasher.finalize().into()
}

fn seal(plain: &str) -> (Vec<u8>, String, Vec<(u32, u32)>, String) {
    let mut rng = rand::thread_rng();
    let mut table = Vec::with_capacity(16);
    for _ in 0..16 {
        table.push((rng.next_u32() % 977, rng.next_u32() % 977));
    }
    let salt = {
        let mut bytes = [0u8; 12];
        rng.fill_bytes(&mut bytes);
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    };
    let key = derive_key(&salt, &table);
    let cipher = Aes256Gcm::new_from_slice(&key).expect("32-byte key");
    let mut nonce_bytes = [0u8; 12];
    rng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plain.as_bytes())
        .expect("encryption cannot fail");
    let nonce_b64 = B64.encode(nonce_bytes);
    (ciphertext, nonce_b64, table, salt)
}

fn rust_bytes(bytes: &[u8]) -> String {
    let parts: Vec<String> = bytes.iter().map(|b| format!("0x{b:02X}")).collect();
    format!("&[{}]", parts.join(", "))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: cargo xtask seal [--name NAME] -- <plaintext>";
    let mut name = String::from("SEALED_VALUE");
    let mut rest = args.clone();
    if rest.first().map(String::as_str) == Some("seal") {
        rest.remove(0);
    }
    if rest.first().map(String::as_str) == Some("--name") {
        name = rest.get(1).cloned().unwrap_or_else(|| usage.into());
        rest.drain(..2);
    }
    if rest.first().map(String::as_str) != Some("--") {
        eprintln!("{usage}");
        std::process::exit(2);
    }
    let plain = rest[1..].join(" ");
    if plain.is_empty() {
        eprintln!("{usage}");
        std::process::exit(2);
    }

    let (ciphertext, nonce_b64, table, salt) = seal(&plain);
    let table_lines: Vec<String> = table
        .iter()
        .map(|(a, b)| format!("        ({a}, {b}),"))
        .collect();

    println!("// Sealed with `cargo xtask seal --name {name}`. Plaintext is not stored anywhere.");
    println!("pub const {name}: Sealed = Sealed {{");
    println!("    ciphertext: {},", rust_bytes(&ciphertext));
    println!("    nonce_b64: \"{nonce_b64}\",");
    println!("    table: &[");
    for line in &table_lines {
        println!("{line}");
    }
    println!("    ],");
    println!("    salt: \"{salt}\",");
    println!("}};");
}
