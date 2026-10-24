//! RFC 6238 TOTP (time-based one-time password) implementation.
//!
//! Used for device-level 2FA: the server provisions a random secret per
//! device, the client stores it (encrypted) and computes the current 6-digit
//! code locally — even without network access. The server can verify the
//! code for sensitive operations.

use hmac::{Hmac, Mac};
use sha1::Sha1;

const TOTP_STEP_SECS: u64 = 30;
const TOTP_DIGITS: u32 = 6;

fn hotp(secret: &[u8], counter: u64) -> u32 {
    let mut mac = Hmac::<Sha1>::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(&counter.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = (digest[digest.len() - 1] & 0x0F) as usize;
    let code = u32::from_be_bytes([
        digest[offset],
        digest[offset + 1],
        digest[offset + 2],
        digest[offset + 3],
    ]) & 0x7FFF_FFFF;
    code % 10u32.pow(TOTP_DIGITS)
}

/// Generate the current TOTP code for a base32-encoded secret.
pub fn totp_now(base32_secret: &str) -> Result<String, String> {
    let secret = decode_base32(base32_secret)?;
    let counter = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("系统时间异常: {e}"))?
        .as_secs()
        / TOTP_STEP_SECS;
    Ok(format!("{:06}", hotp(&secret, counter)))
}

/// Verify a TOTP code allowing ±1 step of clock drift.
pub fn totp_verify(base32_secret: &str, code: &str) -> bool {
    let secret = match decode_base32(base32_secret) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let code_num: u32 = match code.trim().parse() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / TOTP_STEP_SECS;
    for offset in [-1i64, 0, 1] {
        let counter = (now as i64 + offset) as u64;
        if hotp(&secret, counter) == code_num {
            return true;
        }
    }
    false
}

/// Generate a random base32-encoded secret (20 bytes = 160 bits).
pub fn generate_secret() -> String {
    let bytes: [u8; 20] = rand::random();
    encode_base32(&bytes)
}

const BASE32_ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

pub fn encode_base32(data: &[u8]) -> String {
    let mut output = String::new();
    let mut bit_buffer = 0u32;
    let mut bit_count = 0;
    for &byte in data {
        bit_buffer = (bit_buffer << 8) | byte as u32;
        bit_count += 8;
        while bit_count >= 5 {
            let index = (bit_buffer >> (bit_count - 5)) & 0x1F;
            output.push(BASE32_ALPHABET[index as usize] as char);
            bit_count -= 5;
        }
    }
    if bit_count > 0 {
        let index = (bit_buffer << (5 - bit_count)) & 0x1F;
        output.push(BASE32_ALPHABET[index as usize] as char);
    }
    output
}

pub fn decode_base32(encoded: &str) -> Result<Vec<u8>, String> {
    let mut bit_buffer = 0u32;
    let mut bit_count = 0;
    let mut output = Vec::new();
    for ch in encoded.to_ascii_uppercase().chars() {
        let index = BASE32_ALPHABET
            .iter()
            .position(|&c| c == ch as u8)
            .ok_or_else(|| format!("无效的 Base32 字符: {ch}"))?;
        bit_buffer = (bit_buffer << 5) | index as u32;
        bit_count += 5;
        if bit_count >= 8 {
            output.push((bit_buffer >> (bit_count - 8)) as u8);
            bit_count -= 8;
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base32_roundtrip() {
        let secret = generate_secret();
        let decoded = decode_base32(&secret).unwrap();
        assert_eq!(decoded.len(), 20);
        let re_encoded = encode_base32(&decoded);
        assert_eq!(secret, re_encoded);
    }

    #[test]
    fn totp_deterministic() {
        // RFC 6238 test vector: secret "12345678901234567890" (ASCII)
        let secret = encode_base32(b"12345678901234567890");
        let counter = 1u64;
        let secret_bytes = decode_base32(&secret).unwrap();
        let code = hotp(&secret_bytes, counter);
        // RFC 4226 test vector: counter=1 -> 287082
        assert_eq!(code, 287082);
    }

    #[test]
    fn totp_verify_accepts_current_code() {
        let secret = generate_secret();
        let code = totp_now(&secret).unwrap();
        assert!(totp_verify(&secret, &code), "current code should verify");
    }

    #[test]
    fn totp_verify_rejects_wrong_code() {
        let secret = generate_secret();
        assert!(!totp_verify(&secret, "000000"));
        assert!(!totp_verify(&secret, "abc"));
        assert!(!totp_verify(&secret, ""));
    }

    #[test]
    fn totp_verify_rejects_invalid_secret() {
        assert!(!totp_verify("!!!invalid!!!", "123456"));
    }
}
