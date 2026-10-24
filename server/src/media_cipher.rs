use std::{fs, path::Path};

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use rand::RngCore;

const MAGIC: &[u8; 5] = b"SGME1";
const NONCE_LEN: usize = 12;

/// Encrypts evidence WAV and call recordings at rest with AES-256-GCM.
pub struct MediaCipher {
    cipher: Aes256Gcm,
}

impl MediaCipher {
    pub fn load_or_create(path: &Path) -> anyhow::Result<Self> {
        let key = match fs::read(path) {
            Ok(bytes) if bytes.len() == 32 => bytes,
            Ok(_) => {
                return Err(anyhow::anyhow!(
                    "媒体密钥 {} 长度无效，请从备份恢复",
                    path.display()
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut key = [0u8; 32];
                rand::rng().fill_bytes(&mut key);
                crate::private_file::atomic_write(path, &key)?;
                key.to_vec()
            }
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            cipher: Aes256Gcm::new_from_slice(&key)
                .map_err(|_| anyhow::anyhow!("媒体加密密钥无效"))?,
        })
    }

    #[cfg(test)]
    fn from_key(key: [u8; 32]) -> Self {
        Self {
            cipher: Aes256Gcm::new_from_slice(&key).expect("valid key"),
        }
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> anyhow::Result<Vec<u8>> {
        let mut nonce = [0u8; NONCE_LEN];
        rand::rng().fill_bytes(&mut nonce);
        let ciphertext = self
            .cipher
            .encrypt(Nonce::from_slice(&nonce), plaintext)
            .map_err(|_| anyhow::anyhow!("媒体加密失败"))?;
        let mut output = Vec::with_capacity(MAGIC.len() + NONCE_LEN + ciphertext.len());
        output.extend_from_slice(MAGIC);
        output.extend_from_slice(&nonce);
        output.extend_from_slice(&ciphertext);
        Ok(output)
    }

    pub fn decrypt(&self, stored: &[u8]) -> anyhow::Result<Vec<u8>> {
        // Existing plaintext media from older versions remains readable.
        if !stored.starts_with(MAGIC) {
            return Ok(stored.to_vec());
        }
        if stored.len() < MAGIC.len() + NONCE_LEN + 16 {
            return Err(anyhow::anyhow!("媒体文件已损坏"));
        }
        let nonce_start = MAGIC.len();
        let body_start = nonce_start + NONCE_LEN;
        let plaintext = self
            .cipher
            .decrypt(
                Nonce::from_slice(&stored[nonce_start..body_start]),
                &stored[body_start..],
            )
            .map_err(|_| anyhow::anyhow!("媒体解密失败，密钥可能不匹配"))?;
        Ok(plaintext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypts_and_decrypts_media() {
        let cipher = MediaCipher::from_key([7u8; 32]);
        let plaintext = b"RIFF....WAVE test evidence";
        let encrypted = cipher.encrypt(plaintext).expect("encrypt");
        assert_ne!(&encrypted[..], plaintext);
        assert_eq!(&encrypted[..MAGIC.len()], MAGIC);
        assert_eq!(cipher.decrypt(&encrypted).expect("decrypt"), plaintext);
    }

    #[test]
    fn rejects_modified_ciphertext_and_wrong_keys() {
        let cipher = MediaCipher::from_key([7; 32]);
        let mut encrypted = cipher.encrypt(b"private audio").unwrap();
        assert!(MediaCipher::from_key([8; 32]).decrypt(&encrypted).is_err());
        let last = encrypted.len() - 1;
        encrypted[last] ^= 1;
        assert!(cipher.decrypt(&encrypted).is_err());
        assert_ne!(
            cipher.encrypt(b"same").unwrap(),
            cipher.encrypt(b"same").unwrap()
        );
    }

    #[test]
    fn keeps_legacy_plaintext_readable() {
        let cipher = MediaCipher::from_key([9u8; 32]);
        assert_eq!(
            cipher.decrypt(b"legacy wav").expect("plaintext"),
            b"legacy wav"
        );
    }
}
