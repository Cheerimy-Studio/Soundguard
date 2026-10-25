#[allow(dead_code)] // Reuse server algorithm; provisioning helpers are server-only.
#[path = "../../server/src/totp.rs"]
mod algorithm;
pub struct TotpEngine {
    secret: String,
    offset: i64,
}
impl TotpEngine {
    pub fn new() -> Self {
        Self {
            secret: String::new(),
            offset: 0,
        }
    }
    pub fn set_secret(&mut self, s: String) {
        self.secret = s;
    }
    pub fn set_offset(&mut self, seconds: i64) {
        self.offset = seconds;
    }
    fn now(&self) -> u64 {
        (chrono::Utc::now().timestamp() + self.offset).max(0) as u64
    }
    pub fn verify(&self, s: &str) -> bool {
        !self.secret.is_empty()
            && s.len() == 6
            && s.bytes().all(|b| b.is_ascii_digit())
            && algorithm::totp_verify_at(&self.secret, s, self.now())
    }
}
