//! What the server is told by its environment.

use std::path::PathBuf;

use webauthn_rs::prelude::Url;

/// How sign-in codes reach people.
#[derive(Debug, Clone)]
pub enum Mail {
    /// Every code is this one and no mail goes out. For running locally and for tests.
    Dev(String),
    /// Codes are sent through Resend's HTTP API.
    Resend { api_key: String, from: String },
}

#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    /// Where `monster.sqlite` lives.
    pub data_dir: PathBuf,
    /// The server's own origin. The passkey relying party's id and origin come from it.
    pub public_url: Url,
    /// Whether a sign-in by code must be finished by creating a passkey.
    pub passkeys: bool,
    pub mail: Mail,
}

impl Config {
    /// Reads `PORT`, `DATA_DIR`, `PUBLIC_URL`, `PASSKEYS`, `RESEND_API_KEY`, `MAIL_FROM` and
    /// `DEV_CODE`. The error says, in words fit for a log, what is wrong.
    pub fn from_env() -> Result<Self, String> {
        Self::from_lookup(|name| std::env::var(name).ok().filter(|v| !v.trim().is_empty()))
    }

    /// The same as [`Config::from_env`], with the variables read through `var`.
    pub fn from_lookup(var: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let port = match var("PORT") {
            Some(port) => port
                .trim()
                .parse()
                .map_err(|_| format!("PORT must be a port number, not {port:?}"))?,
            None => 8787,
        };
        let data_dir = PathBuf::from(var("DATA_DIR").unwrap_or_else(|| "/data".into()));
        let public_url = var("PUBLIC_URL").unwrap_or_else(|| format!("http://localhost:{port}"));
        let public_url = Url::parse(public_url.trim())
            .map_err(|e| format!("PUBLIC_URL must be a URL such as https://example.com: {e}"))?;
        if public_url.host_str().is_none() {
            return Err("PUBLIC_URL must name a host, such as https://example.com".into());
        }
        let passkeys = match var("PASSKEYS").as_deref().map(str::trim) {
            None | Some("required") => true,
            Some("off") => false,
            Some(other) => return Err(format!("PASSKEYS must be required or off, not {other:?}")),
        };
        let mail = match (var("DEV_CODE"), var("RESEND_API_KEY")) {
            (Some(_), Some(_)) => {
                return Err("DEV_CODE and RESEND_API_KEY are both set; set only one".into());
            }
            (Some(code), None) => {
                let code = code.trim().to_string();
                if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
                    return Err("DEV_CODE must be six digits".into());
                }
                Mail::Dev(code)
            }
            (None, Some(api_key)) => {
                let from = var("MAIL_FROM").ok_or(
                    "MAIL_FROM must be set with RESEND_API_KEY, e.g. Flexspeak <monster@example.com>",
                )?;
                Mail::Resend {
                    api_key: api_key.trim().to_string(),
                    from: from.trim().to_string(),
                }
            }
            (None, None) => {
                return Err(
                    "No way to send sign-in codes: set RESEND_API_KEY and MAIL_FROM, \
                    or DEV_CODE for local use"
                        .into(),
                );
            }
        };
        Ok(Self {
            port,
            data_dir,
            public_url,
            passkeys,
            mail,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(vars: &[(&str, &str)]) -> Result<Config, String> {
        Config::from_lookup(|name| {
            vars.iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| v.to_string())
        })
    }

    #[test]
    fn it_needs_exactly_one_way_to_send_codes() {
        assert!(config(&[]).is_err());
        assert!(config(&[("DEV_CODE", "123456"), ("RESEND_API_KEY", "k")]).is_err());
        assert!(config(&[("RESEND_API_KEY", "k")]).is_err());
        assert!(config(&[("DEV_CODE", "12345")]).is_err());
        let ok = config(&[("DEV_CODE", "123456")]).unwrap();
        assert_eq!(ok.port, 8787);
        assert!(ok.passkeys);
        assert_eq!(ok.public_url.as_str(), "http://localhost:8787/");
    }

    #[test]
    fn passkeys_can_be_turned_off() {
        let off = config(&[("DEV_CODE", "123456"), ("PASSKEYS", "off")]).unwrap();
        assert!(!off.passkeys);
        assert!(config(&[("DEV_CODE", "123456"), ("PASSKEYS", "maybe")]).is_err());
    }
}
