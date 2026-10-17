use chrono::{Local, Utc};
use tracing::{error, info};

use crate::{
    models::{Alert, AlertKind, Device, Settings},
    notify::{email_shell, html_escape, initial_email_status, spawn_email},
    state::AppState,
};

pub fn spawn_offline_monitor(state: AppState) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(10));
        loop {
            ticker.tick().await;
            let now = Utc::now();
            let snapshot = state.store.snapshot().await;
            let settings = snapshot.settings.clone();

            for device in snapshot.devices.clone() {
                let effective = crate::models::effective_settings(&settings, &device);
                let timeout = effective.offline_timeout_secs as i64;
                if device.last_seen.is_none()
                    || device.expected_offline
                    || device.offline_alert_active
                    || (!device.monitoring && !device.desired_monitoring)
                {
                    continue;
                }
                if device.is_online(now, effective.offline_timeout_secs) {
                    continue;
                }
                let last_seen = match device.last_seen {
                    Some(value) => value,
                    None => continue,
                };
                if now.signed_duration_since(last_seen).num_seconds() <= timeout {
                    continue;
                }

                let recipients = crate::models::notification_recipients(
                    &snapshot,
                    &device.id,
                    &settings.alert_recipients,
                );
                let email_status = initial_email_status(&recipients);
                let created = state
                    .store
                    .mutate(|state| {
                        let Some(target) = state
                            .devices
                            .iter_mut()
                            .find(|item| item.id == device.id)
                        else {
                            return None;
                        };
                        if target.expected_offline
                            || target.offline_alert_active
                            || (!target.monitoring && !target.desired_monitoring)
                        {
                            return None;
                        }
                        let last_seen = target.last_seen?;
                        let target_timeout =
                            crate::models::effective_settings(&state.settings, target)
                                .offline_timeout_secs as i64;
                        if Utc::now().signed_duration_since(last_seen).num_seconds()
                            <= target_timeout
                        {
                            return None;
                        }

                        let mut alert = Alert::new(
                            target.id.clone(),
                            AlertKind::Offline,
                            "critical",
                            "监测设备离线",
                            format!(
                                "设备自 {} 起失去心跳。可能是网络中断、软件被强杀、系统休眠或断电。",
                                last_seen.with_timezone(&Local).format("%m-%d %H:%M:%S")
                            ),
                        );
                        alert.email_status = email_status.clone();
                        state.alerts.insert(0, alert.clone());
                        target.offline_alert_active = true;
                        target.monitoring = false;
                        Some(alert)
                    })
                    .await;

                let alert = match created {
                    Ok(Some(alert)) => alert,
                    Ok(None) => continue,
                    Err(error) => {
                        error!("Could not create offline alert: {error}");
                        continue;
                    }
                };
                info!("Offline alert created for device {}", device.id);
                state.hub.broadcast_alert(&alert);
                send_offline_email(
                    state.clone(),
                    alert,
                    device.clone(),
                    effective.clone(),
                    recipients.clone(),
                )
                .await
                .unwrap_or_else(|error| error!("Could not prepare offline email: {error}"));

                let latest = state.store.snapshot().await;
                if let Some(updated) = latest.devices.iter().find(|item| item.id == device.id) {
                    let updated_settings =
                        crate::models::effective_settings(&latest.settings, updated);
                    state.hub.broadcast_event(
                        "device",
                        serde_json::to_value(updated.view(
                            Utc::now(),
                            updated_settings.offline_timeout_secs,
                        ))
                        .unwrap_or_default(),
                    );
                }
            }
        }
    });
}

async fn send_offline_email(
    state: AppState,
    alert: Alert,
    device: Device,
    settings: Settings,
    recipients: Vec<String>,
) -> anyhow::Result<()> {
    let subject = format!("【声垣告警】{} 已离线", device.name);
    let last_seen = device
        .last_seen
        .map(|time| time.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "未知".to_string());
    let text = format!(
        "声垣 SoundGuard 设备离线告警\n\n设备：{}\n最后心跳：{}\n判定：设备超过 {} 秒未上报，可能为异常关闭、断电或断网。\n\n请登录管理端核查现场情况。\n",
        device.name,
        last_seen,
        settings.offline_timeout_secs
    );
    let body = format!(
        r##"<p style="margin:0 0 18px;color:#cbd5e1;font-size:14px;line-height:1.8">设备 <strong style="color:#fff">{name}</strong> 已超过 {timeout} 秒未上报心跳，可能发生异常关闭、强杀进程、系统休眠、断电或网络中断。</p>
<div style="background:#2a1116;border:1px solid #6b2130;border-radius:12px;padding:18px;margin:18px 0">
  <div style="color:#fda4af;font-size:12px;letter-spacing:1px">LAST HEARTBEAT</div>
  <div style="color:#fff;font-size:20px;font-weight:700;margin-top:6px">{last_seen}</div>
</div>
<p style="margin:0;color:#94a3b8;font-size:13px;line-height:1.8">该告警只发送一次。设备恢复联网后会自动记录恢复事件。请管理员及时登录后台核查，必要时联系现场负责人。</p>"##,
        name = html_escape(&device.name),
        timeout = settings.offline_timeout_secs,
        last_seen = html_escape(&last_seen),
    );
    let html = email_shell("#7f1d1d", "DEVICE OFFLINE ALERT", "监测设备离线", &body);
    spawn_email(
        state,
        alert.id.clone(),
        recipients,
        subject,
        text,
        html,
        Vec::new(),
    );
    Ok(())
}

pub fn spawn_maintenance(state: AppState) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(6 * 3600));
        loop {
            ticker.tick().await;
            let settings = state.store.settings().await;
            match state.store.cleanup_readings(settings.retention_days).await {
                Ok(removed) if removed > 0 => {
                    info!("Removed {removed} expired telemetry files");
                }
                Ok(_) => {}
                Err(error) => error!("Telemetry cleanup failed: {error}"),
            }
        }
    });
}
