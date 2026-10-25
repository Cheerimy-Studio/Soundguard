use super::*;

pub async fn authorize(state: &RuntimeState, pin: &str) -> Result<(), String> {
    if pin.is_empty() || pin.len() > 512 {
        return Err("请输入本地密码、固定密码或动态验证码".into());
    }
    state
        .local_auth
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .check_unlocked()?;
    let config = state.config();
    if config.local_password_hash.is_empty() {
        return Err("请先完成本地密码配置".into());
    }
    let local = verify_local_password(&config.local_password_hash, pin);
    let remote = if !local {
        post_json::<Value, _>(
            state,
            &config,
            "/api/device/totp",
            &json!({"deviceId":config.device_id,"code":pin}),
        )
        .await
        .map(|v| v["verified"] == true)
        .unwrap_or(false)
    } else {
        false
    };
    let current = state.config();
    if current.server_url != config.server_url
        || current.device_id != config.device_id
        || current.device_token != config.device_token
    {
        return Err("设备配置已变化，请重新验证".into());
    }
    state
        .local_auth
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .verify(|| local || remote)
}
