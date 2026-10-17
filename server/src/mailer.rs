use std::{path::PathBuf, time::Duration};

use chrono::Utc;
use lettre::{
    message::{
        header::ContentType, Attachment, Mailbox, Message, MultiPart, SinglePart,
    },
    transport::smtp::authentication::Credentials,
    AsyncSmtpTransport, AsyncTransport, Tokio1Executor,
};
use tracing::{error, info, warn};

use crate::config::{Config, SmtpTls};

#[derive(Debug, Clone)]
pub struct MailAttachment {
    pub filename: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct MailResult {
    pub status: String,
    pub error: Option<String>,
}

impl MailResult {
    fn sent() -> Self {
        Self {
            status: "sent".to_string(),
            error: None,
        }
    }

    fn dry_run() -> Self {
        Self {
            status: "dry_run".to_string(),
            error: None,
        }
    }

    fn failed(message: impl Into<String>) -> Self {
        Self {
            status: "failed".to_string(),
            error: Some(message.into()),
        }
    }
}

pub struct Mailer {
    transport: Option<AsyncSmtpTransport<Tokio1Executor>>,
    from: Option<Mailbox>,
    from_address: String,
    from_name: String,
    outbox_dir: PathBuf,
}

impl Mailer {
    pub fn new(config: &Config) -> anyhow::Result<Self> {
        let (transport, from) = match &config.smtp {
            Some(smtp) => {
                let mut builder = match smtp.tls {
                    SmtpTls::StartTls => {
                        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&smtp.host)?
                    }
                    SmtpTls::Wrapper => {
                        AsyncSmtpTransport::<Tokio1Executor>::relay(&smtp.host)?
                    }
                    SmtpTls::None => {
                        AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&smtp.host)
                    }
                };
                builder = builder.port(smtp.port).timeout(Some(Duration::from_secs(18)));
                if let Some(username) = &smtp.username {
                    builder = builder.credentials(Credentials::new(
                        username.clone(),
                        smtp.password.clone().unwrap_or_default(),
                    ));
                }
                let from = Mailbox::new(
                    Some(smtp.from_name.clone()),
                    smtp.from.parse()?,
                );
                (Some(builder.build()), Some(from))
            }
            None => {
                warn!("SMTP is not configured; outgoing mail will be written to data/outbox instead");
                (None, None)
            }
        };

        Ok(Self {
            transport,
            from,
            from_address: config
                .smtp
                .as_ref()
                .map(|smtp| smtp.from.clone())
                .unwrap_or_else(|| "soundguard@localhost".to_string()),
            from_name: config
                .smtp
                .as_ref()
                .map(|smtp| smtp.from_name.clone())
                .unwrap_or_else(|| "声垣 SoundGuard".to_string()),
            outbox_dir: config.data_path("outbox"),
        })
    }

    pub fn configured(&self) -> bool {
        self.transport.is_some()
    }

    pub async fn send(
        &self,
        recipients: &[String],
        subject: &str,
        text: &str,
        html: &str,
        attachments: Vec<MailAttachment>,
    ) -> MailResult {
        if recipients.is_empty() {
            return MailResult {
                status: "disabled".to_string(),
                error: Some("未配置收件人".to_string()),
            };
        }

        let parsed: Vec<Mailbox> = recipients
            .iter()
            .filter_map(|address| match address.parse::<Mailbox>() {
                Ok(mailbox) => Some(mailbox),
                Err(error) => {
                    warn!("Ignoring invalid email recipient {address}: {error}");
                    None
                }
            })
            .collect();
        if parsed.is_empty() {
            return MailResult::failed("所有收件人地址均无效");
        }

        let Some(transport) = &self.transport else {
            return self.write_outbox(&parsed, subject, text, &attachments).await;
        };
        let Some(from) = &self.from else {
            return MailResult::failed("发件人地址未配置");
        };

        let text_part = SinglePart::builder()
            .header(ContentType::TEXT_PLAIN)
            .body(text.to_string());
        let html_part = SinglePart::builder()
            .header(ContentType::TEXT_HTML)
            .body(html.to_string());
        let alternative = MultiPart::alternative()
            .singlepart(text_part)
            .singlepart(html_part);
        let mut mixed = MultiPart::mixed().multipart(alternative);

        for attachment in attachments {
            let content_type = attachment
                .content_type
                .parse::<ContentType>()
                .unwrap_or(ContentType::parse("application/octet-stream").unwrap());
            mixed = mixed.singlepart(
                Attachment::new(attachment.filename)
                    .body(attachment.bytes, content_type),
            );
        }

        let mut builder = Message::builder()
            .from(from.clone())
            .subject(subject)
            .date_now();
        for mailbox in parsed {
            builder = builder.to(mailbox);
        }
        let message = match builder.multipart(mixed) {
            Ok(message) => message,
            Err(error) => {
                error!("Could not build email: {error}");
                return MailResult::failed(format!("邮件格式错误: {error}"));
            }
        };

        match transport.send(message).await {
            Ok(response) => {
                info!(
                    "Email sent to {}: {}",
                    recipients.join(", "),
                    response.code()
                );
                MailResult::sent()
            }
            Err(error) => {
                error!("SMTP delivery failed: {error}");
                MailResult::failed(format!("SMTP 发送失败: {error}"))
            }
        }
    }

    async fn write_outbox(
        &self,
        recipients: &[Mailbox],
        subject: &str,
        text: &str,
        attachments: &[MailAttachment],
    ) -> MailResult {
        if let Err(error) = tokio::fs::create_dir_all(&self.outbox_dir).await {
            return MailResult::failed(format!("无法创建 outbox 目录: {error}"));
        }
        let stamp = Utc::now().format("%Y%m%d-%H%M%S-%3f");
        let filename = format!("{stamp}-{}.txt", sanitize_subject(subject));
        let mut preview = String::new();
        preview.push_str("=== SoundGuard dry-run outbox ===\n");
        preview.push_str(&format!(
            "From: {} <{}>\n",
            self.from_name, self.from_address
        ));
        preview.push_str(&format!(
            "To: {}\n",
            recipients
                .iter()
                .map(|mailbox| mailbox.email.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
        preview.push_str(&format!("Subject: {subject}\n\n{text}\n\n"));
        if !attachments.is_empty() {
            preview.push_str("Attachments (not included in text preview):\n");
            for attachment in attachments {
                preview.push_str(&format!(
                    "- {} ({}, {} bytes)\n",
                    attachment.filename,
                    attachment.content_type,
                    attachment.bytes.len()
                ));
            }
        }
        let path = self.outbox_dir.join(filename);
        match tokio::fs::write(&path, preview).await {
            Ok(_) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = tokio::fs::set_permissions(
                        &path,
                        std::fs::Permissions::from_mode(0o600),
                    )
                    .await;
                }
                info!("SMTP not configured; email preview written to data/outbox");
                MailResult::dry_run()
            }
            Err(error) => MailResult::failed(format!("无法写入 outbox: {error}")),
        }
    }
}

fn sanitize_subject(subject: &str) -> String {
    subject
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch
            } else if ch.is_whitespace() {
                '-'
            } else {
                '_'
            }
        })
        .take(48)
        .collect()
}
