use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use rand::Rng;
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    config::Config,
    models::{User, UserRole},
};

pub fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

pub fn new_device_token() -> String {
    format!(
        "sg_{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

pub fn new_device_id() -> String {
    format!(
        "dev-{}",
        uuid::Uuid::new_v4()
            .simple()
            .to_string()
            .chars()
            .take(8)
            .collect::<String>()
    )
}

pub fn new_user_id() -> String {
    format!("usr-{}", uuid::Uuid::new_v4().simple())
}

pub fn generate_password(length: usize) -> String {
    rand::rng()
        .sample_iter(&rand::distr::Alphanumeric)
        .take(length.max(12))
        .map(char::from)
        .collect()
}

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| anyhow::anyhow!("密码哈希失败: {error}"))
}

pub fn verify_password(hash: &str, password: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

pub fn validate_password(password: &str) -> Result<(), String> {
    if password.chars().count() < 8 {
        return Err("密码至少需要 8 个字符".to_string());
    }
    if password.chars().count() > 128 {
        return Err("密码不能超过 128 个字符".to_string());
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub username: String,
    pub role: String,
    pub ver: u64,
    pub iat: usize,
    pub exp: usize,
}

#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub id: String,
    pub role: UserRole,
    pub device_ids: Vec<String>,
}

impl CurrentUser {
    pub fn from_user(user: &User) -> Self {
        Self {
            id: user.id.clone(),
            role: user.role.clone(),
            device_ids: user.device_ids.clone(),
        }
    }

    pub fn is_super(&self) -> bool {
        self.role.is_super()
    }

    pub fn can_access_device(&self, device_id: &str) -> bool {
        self.is_super() || self.device_ids.iter().any(|id| id == device_id)
    }
}

pub fn issue_user_token(
    config: &Config,
    user: &User,
    ttl_hours: i64,
) -> anyhow::Result<(String, chrono::DateTime<Utc>)> {
    let now = Utc::now();
    let expires_at = now + Duration::hours(ttl_hours.max(1));
    let claims = Claims {
        sub: user.id.clone(),
        username: user.username.clone(),
        role: user.role.as_str().to_string(),
        ver: user.token_version,
        iat: now.timestamp() as usize,
        exp: expires_at.timestamp() as usize,
    };
    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(config.jwt_secret.as_bytes()),
    )?;
    Ok((token, expires_at))
}

pub fn verify_token(config: &Config, token: &str) -> Option<Claims> {
    let validation = Validation::new(Algorithm::HS256);
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(config.jwt_secret.as_bytes()),
        &validation,
    )
    .ok()
    .map(|data| data.claims)
}

/// Compares secrets without short-circuiting on the first differing byte.
pub fn secure_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let mut diff = left.len() ^ right.len();
    let max = left.len().max(right.len());
    for index in 0..max {
        let a = left.get(index).copied().unwrap_or(0);
        let b = right.get(index).copied().unwrap_or(0);
        diff |= usize::from(a ^ b);
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secure_compare_handles_different_lengths() {
        assert!(secure_eq("2468", "2468"));
        assert!(!secure_eq("2468", "2469"));
        assert!(!secure_eq("2468", "24680"));
    }

    #[test]
    fn secure_compare_rejects_zero_padded_lengths_over_255() {
        assert!(!secure_eq("", &"\0".repeat(256)));
        assert!(!secure_eq("pin", &format!("pin{}", "\0".repeat(256))));
    }

    #[test]
    fn password_hash_round_trip() {
        let hash = hash_password("correct horse battery").expect("hash");
        assert!(verify_password(&hash, "correct horse battery"));
        assert!(!verify_password(&hash, "wrong password"));
    }
}
