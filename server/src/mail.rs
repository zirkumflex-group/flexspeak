//! Sending sign-in codes.

use std::time::Duration;

use crate::config::Mail;

/// Sends sign-in codes, or with `DEV_CODE` pretends to.
#[derive(Clone)]
pub struct Mailer {
    mail: Mail,
    http: reqwest::Client,
}

impl Mailer {
    pub fn new(mail: Mail) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("an HTTP client with default settings builds");
        Self { mail, http }
    }

    /// The code to send next: random, or the fixed one in development.
    pub fn new_code(&self) -> String {
        match &self.mail {
            Mail::Dev(code) => code.clone(),
            Mail::Resend { .. } => crate::secret::six_digits(),
        }
    }

    /// Emails `code` to `to`. The error is fit for a log and never holds the code.
    pub async fn send_code(&self, to: &str, code: &str) -> Result<(), String> {
        let Mail::Resend { api_key, from } = &self.mail else {
            return Ok(());
        };
        let body = serde_json::json!({
            "from": from,
            "to": [to],
            "subject": format!("{code} is your Flexspeak sign-in code"),
            "text": format!(
                "Your Flexspeak sign-in code is {code}\n\n\
                 Type it in Flexspeak to sign in. It works for 10 minutes.\n\n\
                 If you didn't ask for it, ignore this email.\n"
            ),
        });
        let answer = self
            .http
            .post("https://api.resend.com/emails")
            .bearer_auth(api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("could not reach Resend: {}", e.without_url()))?;
        let status = answer.status();
        if status.is_success() {
            Ok(())
        } else {
            // Resend's error body names the problem (a bad key, an unverified domain) and
            // never echoes the message, so it is safe to log.
            let detail = answer.text().await.unwrap_or_default();
            Err(format!("Resend answered {status}: {detail}"))
        }
    }
}
