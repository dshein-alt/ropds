use crate::config::SmtpConfig;
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor, message::header::ContentType,
    transport::smtp::authentication::Credentials,
};

/// Returns true if SMTP is configured (host + from + at least one recipient).
pub fn is_email_configured(cfg: &SmtpConfig) -> bool {
    !cfg.host.is_empty() && !cfg.from.is_empty() && !cfg.send_to.is_empty()
}

/// Send an email asynchronously in a spawned Tokio task.
/// Errors are logged as warnings and never surfaced to the caller.
pub fn send_async(cfg: SmtpConfig, recipients: Vec<String>, subject: String, body: String) {
    tokio::spawn(async move {
        for to in recipients {
            if let Err(e) = do_send(&cfg, &to, &subject, &body).await {
                tracing::warn!("Email send failed to {to}: {e}");
            }
        }
    });
}

async fn do_send(
    cfg: &SmtpConfig,
    to: &str,
    subject: &str,
    body: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let email = Message::builder()
        .from(cfg.from.parse()?)
        .to(to.parse()?)
        .subject(subject)
        .header(ContentType::TEXT_PLAIN)
        .body(body.to_string())?;

    let creds = if !cfg.username.is_empty() {
        Some(Credentials::new(cfg.username.clone(), cfg.password.clone()))
    } else {
        None
    };

    let builder = if cfg.starttls {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.host)?
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::relay(&cfg.host)?
    }
    .port(cfg.port);

    let transport = if let Some(c) = creds {
        builder.credentials(c).build()
    } else {
        builder.build()
    };

    transport.send(email).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SmtpConfig;

    #[test]
    fn test_email_disabled_when_unconfigured() {
        let cfg = SmtpConfig::default();
        assert!(!is_email_configured(&cfg));
    }

    #[test]
    fn test_email_enabled_when_configured() {
        let cfg = SmtpConfig {
            host: "smtp.example.com".into(),
            from: "ropds@example.com".into(),
            send_to: vec!["admin@example.com".into()],
            port: 587,
            starttls: true,
            ..Default::default()
        };
        assert!(is_email_configured(&cfg));
    }
}
