use std::time::{Duration, Instant};

#[derive(Default)]
pub struct PasswordGuard {
    failures: u32,
    locked_until: Option<Instant>,
    authorized_until: Option<Instant>,
}

impl PasswordGuard {
    pub fn verify(&mut self, check: impl FnOnce() -> bool) -> Result<(), String> {
        let now = Instant::now();
        if self.locked_until.is_some_and(|until| until > now) {
            return Err("密码错误过多，请 5 分钟后重试".into());
        }
        if self.locked_until.take().is_some() {
            self.failures = 0;
        }
        if !check() {
            self.authorized_until = None;
            self.failures += 1;
            if self.failures >= 5 {
                self.locked_until = Some(now + Duration::from_secs(300));
                return Err("密码错误过多，请 5 分钟后重试".into());
            }
            return Err(format!("本地密码不正确，还可尝试 {} 次", 5 - self.failures));
        }
        self.failures = 0;
        self.authorized_until = Some(now + Duration::from_secs(300));
        Ok(())
    }

    pub fn require_authorized(&self) -> Result<(), String> {
        if self
            .authorized_until
            .is_some_and(|until| until > Instant::now())
        {
            Ok(())
        } else {
            Err("请重新输入本地密码解锁设置".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_require_password_and_lockout_is_enforced() {
        let mut guard = PasswordGuard::default();
        assert!(guard.require_authorized().is_err());
        assert!(guard.verify(|| true).is_ok());
        assert!(guard.require_authorized().is_ok());
        for _ in 0..5 {
            assert!(guard.verify(|| false).is_err());
        }
        assert!(guard
            .verify(|| panic!("must not check a password during lockout"))
            .is_err());
        assert!(guard.require_authorized().is_err());
        guard.locked_until = Some(Instant::now() - Duration::from_secs(1));
        assert!(guard.verify(|| true).is_ok());
    }
}
