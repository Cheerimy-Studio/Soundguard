use tracing::error;

use crate::{mailer::MailAttachment, models::Alert, state::AppState};

pub fn initial_email_status(recipients: &[String]) -> String {
    if recipients.is_empty() {
        "disabled".to_string()
    } else {
        "pending".to_string()
    }
}

pub fn spawn_email(
    state: AppState,
    alert_id: String,
    recipients: Vec<String>,
    subject: String,
    text: String,
    html: String,
    attachments: Vec<MailAttachment>,
) {
    if recipients.is_empty() {
        tokio::spawn(async move {
            if let Ok(Some(alert)) = state
                .store
                .update_alert_email(&alert_id, "disabled", Some("未配置收件人".to_string()))
                .await
            {
                state.hub.broadcast_alert(&alert);
            }
        });
        return;
    }

    tokio::spawn(async move {
        let result = state
            .mailer
            .send(&recipients, &subject, &text, &html, attachments)
            .await;
        if let Some(error) = &result.error {
            error!("Alert email status {}: {}", result.status, error);
        }
        match state
            .store
            .update_alert_email(&alert_id, &result.status, result.error.clone())
            .await
        {
            Ok(Some(alert)) => state.hub.broadcast_alert(&alert),
            Ok(None) => {}
            Err(error) => error!("Could not update alert email status: {error}"),
        }
    });
}

pub fn email_shell(accent: &str, eyebrow: &str, title: &str, body: &str) -> String {
    format!(
        r##"<div style="font-family:-apple-system,Segoe UI,Microsoft YaHei,Arial,sans-serif;background:#0b1220;padding:28px;color:#e2e8f0">
<div style="max-width:620px;margin:0 auto;background:#111a2e;border:1px solid #22304a;border-radius:18px;overflow:hidden">
  <div style="padding:24px 28px;background:{accent}">
    <div style="font-size:11px;letter-spacing:2px;color:rgba(255,255,255,.65)">{eyebrow}</div>
    <div style="font-size:23px;font-weight:700;margin-top:6px;color:#fff">{title}</div>
  </div>
  <div style="padding:24px 28px">{body}</div>
  <div style="padding:14px 28px;border-top:1px solid #1f2d46;color:#64748b;font-size:11px">
    声垣 SoundGuard
  </div>
</div>
</div>"##
    )
}

pub fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub fn alert_attachment(alert: &Alert, bytes: Vec<u8>) -> Vec<MailAttachment> {
    if alert.evidence_file.is_some() && !bytes.is_empty() {
        vec![MailAttachment {
            filename: alert
                .evidence_file
                .clone()
                .unwrap_or_else(|| "evidence.wav".to_string()),
            content_type: "audio/wav".to_string(),
            bytes,
        }]
    } else {
        Vec::new()
    }
}
