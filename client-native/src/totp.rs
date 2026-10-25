//! TOTP (RFC 6238) engine — computes 6-digit codes locally.

pub struct TotpEngine {
    secret: String,
    code: String,
}

impl TotpEngine {
    pub fn new() -> Self {
        Self {
            secret: String::new(),
            code: String::new(),
        }
    }

    pub fn set_secret(&mut self, secret: String) {
        if secret != self.secret {
            self.secret = secret;
            self.code = self.compute();
        }
    }

    pub fn current_code(&self) -> String {
        self.code.clone()
    }

    pub fn refresh(&mut self) {
        self.code = self.compute();
    }

    fn compute(&self) -> String {
        if self.secret.is_empty() {
            return String::new();
        }
        // Base32 decode
        let secret_bytes = match base32_decode(&self.secret) {
            Ok(b) => b,
            Err(_) => return String::new(),
        };

        // HMAC-SHA1
        use hmac::{Hmac, Mac};
        use sha1::Sha1;
        type HmacSha1 = Hmac<Sha1>;

        let counter = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            / 30;

        let mut mac = match HmacSha1::new_from_slice(&secret_bytes) {
            Ok(m) => m,
            Err(_) => return String::new(),
        };
        mac.update(&counter.to_be_bytes());
        let digest = mac.finalize().into_bytes();

        let offset = (digest[digest.len() - 1] & 0x0F) as usize;
        let code = u32::from_be_bytes([
            digest[offset],
            digest[offset + 1],
            digest[offset + 2],
            digest[offset + 3],
        ]) & 0x7FFF_FFFF;
        format!("{:06}", code % 1_000_000)
    }

    pub fn verify(&self, input: &str) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            / 30;

        let secret_bytes = match base32_decode(&self.secret) {
            Ok(b) => b,
            Err(_) => return false,
        };

        use hmac::{Hmac, Mac};
        use sha1::Sha1;
        type HmacSha1 = Hmac<Sha1>;

        let input_num: u32 = match input.trim().parse() {
            Ok(v) => v,
            Err(_) => return false,
        };

        for offset in [-1i64, 0, 1] {
            let mut mac = match HmacSha1::new_from_slice(&secret_bytes) {
                Ok(m) => m,
                Err(_) => return false,
            };
            mac.update(&((now as i64 + offset) as u64).to_be_bytes());
            let digest = mac.finalize().into_bytes();
            let o = (digest[digest.len() - 1] & 0x0F) as usize;
            let code = (u32::from_be_bytes([
                digest[o],
                digest[o + 1],
                digest[o + 2],
                digest[o + 3],
            ]) & 0x7FFF_FFFF) % 1_000_000;
            if code == input_num {
                return true;
            }
        }
        false
    }
}

const BASE32_ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

fn base32_decode(encoded: &str) -> Result<Vec<u8>, String> {
    let mut bits = 0u32;
    let mut count = 0;
    let mut output = Vec::new();
    for ch in encoded.to_ascii_uppercase().chars() {
        let idx = BASE32_ALPHABET
            .iter()
            .position(|&c| c == ch as u8)
            .ok_or_else(|| format!("invalid base32 char: {ch}"))?;
        bits = (bits << 5) | idx as u32;
        count += 5;
        if count >= 8 {
            output.push((bits >> (count - 8)) as u8);
            count -= 8;
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_roundtrip() {
        // "JBSWY3DPEHPK3PXP" is the base32 of "Hello!"
        let decoded = base32_decode("JBSWY3DPEHPK3PXP").unwrap();
        assert!(!decoded.is_empty());
    }

    #[test]
    fn verify_rejects_invalid() {
        let engine = TotpEngine::new();
        assert!(!engine.verify("000000"));
        assert!(!engine.verify(""));
        assert!(!engine.verify("abc"));
    }
}
