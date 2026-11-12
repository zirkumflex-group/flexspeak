//! A server on a temporary database, driven through its real router, and a software passkey.

#![allow(dead_code)]

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use flexspeak_monster::{AppState, Clock, Config, Mail, router};
use p256::ecdsa::{Signature, SigningKey, signature::Signer};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use tower::ServiceExt;

pub const CODE: &str = "123456";
pub const ORIGIN: &str = "http://localhost:8787";
/// 2026-11-12, so times in tests look like times.
pub const START: i64 = 1_794_441_600;

pub struct Server {
    app: Router,
    pub clock: Clock,
    _dir: TempDir,
}

pub struct Answer {
    pub status: StatusCode,
    pub body: Value,
}

impl Answer {
    /// The `error` code of a problem answer.
    pub fn error(&self) -> &str {
        self.body["error"].as_str().unwrap_or("")
    }
}

impl Server {
    pub fn new(passkeys: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            port: 8787,
            data_dir: dir.path().join("data"),
            public_url: ORIGIN.parse().unwrap(),
            passkeys,
            mail: Mail::Dev(CODE.into()),
        };
        let clock = Clock::manual(START);
        let state = AppState::new(config, clock.clone()).unwrap();
        Self {
            app: router(state),
            clock,
            _dir: dir,
        }
    }

    pub async fn send(
        &self,
        method: Method,
        path: &str,
        session: Option<&str>,
        ip: Option<&str>,
        body: Option<Value>,
    ) -> Answer {
        let mut req = Request::builder().method(method).uri(path);
        if let Some(session) = session {
            req = req.header("authorization", format!("Bearer {session}"));
        }
        if let Some(ip) = ip {
            req = req.header("x-forwarded-for", ip);
        }
        let req = match body {
            Some(body) => req
                .header("content-type", "application/json")
                .body(Body::from(body.to_string())),
            None => req.body(Body::empty()),
        }
        .unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = to_bytes(res.into_body(), 1 << 20).await.unwrap();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into()))
        };
        Answer { status, body }
    }

    pub async fn get(&self, path: &str, session: &str) -> Answer {
        self.send(Method::GET, path, Some(session), None, None)
            .await
    }

    pub async fn post(&self, path: &str, session: Option<&str>, body: Value) -> Answer {
        self.send(Method::POST, path, session, None, Some(body))
            .await
    }

    pub async fn delete(&self, path: &str, session: &str) -> Answer {
        self.send(Method::DELETE, path, Some(session), None, None)
            .await
    }

    /// Asks for a code for `email` and sends it back; the answer to `login/finish`.
    pub async fn code_sign_in(&self, email: &str) -> Answer {
        let started = self
            .post("/v1/login/start", None, json!({ "email": email }))
            .await;
        assert_eq!(started.status, StatusCode::NO_CONTENT, "{:?}", started.body);
        self.post(
            "/v1/login/finish",
            None,
            json!({ "email": email, "code": CODE }),
        )
        .await
    }

    /// Signs in on a server with passkeys off; the session and the account's id.
    pub async fn sign_in(&self, email: &str) -> (String, i64) {
        let finished = self.code_sign_in(email).await;
        assert_eq!(finished.status, StatusCode::OK, "{:?}", finished.body);
        assert_eq!(finished.body["next"], "signed_in");
        (
            finished.body["session"].as_str().unwrap().to_string(),
            finished.body["me"]["id"].as_i64().unwrap(),
        )
    }

    pub async fn add_can(&self, session: &str, flavour: &str, key: &str) -> Answer {
        self.post(
            "/v1/cans",
            Some(session),
            json!({ "flavour": flavour, "key": key }),
        )
        .await
    }
}

fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn unb64(text: &str) -> Vec<u8> {
    URL_SAFE_NO_PAD.decode(text).unwrap()
}

/// A CBOR item head: major type and argument.
fn cbor_head(out: &mut Vec<u8>, major: u8, n: u64) {
    let major = major << 5;
    match n {
        0..=23 => out.push(major | n as u8),
        24..=0xff => out.extend([major | 24, n as u8]),
        0x100..=0xffff => {
            out.push(major | 25);
            out.extend((n as u16).to_be_bytes());
        }
        _ => {
            out.push(major | 26);
            out.extend((n as u32).to_be_bytes());
        }
    }
}

fn cbor_int(out: &mut Vec<u8>, n: i64) {
    if n >= 0 {
        cbor_head(out, 0, n as u64);
    } else {
        cbor_head(out, 1, (-1 - n) as u64);
    }
}

fn cbor_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    cbor_head(out, 2, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

fn cbor_text(out: &mut Vec<u8>, text: &str) {
    cbor_head(out, 3, text.len() as u64);
    out.extend_from_slice(text.as_bytes());
}

/// A passkey held in memory, doing what a browser and an authenticator would: enough of
/// WebAuthn (no attestation, ES256, user verified) to run the server's ceremonies end to end.
pub struct SoftPasskey {
    key: SigningKey,
    pub credential_id: Vec<u8>,
    user_handle: Vec<u8>,
    counter: u32,
}

const UP: u8 = 0x01;
const UV: u8 = 0x04;
const AT: u8 = 0x40;

fn client_data(kind: &str, challenge: &str) -> Vec<u8> {
    json!({ "type": kind, "challenge": challenge, "origin": ORIGIN, "crossOrigin": false })
        .to_string()
        .into_bytes()
}

impl SoftPasskey {
    /// Answers `navigator.credentials.create({ publicKey })`.
    pub fn create(public_key: &Value) -> (Self, Value) {
        let key = loop {
            let mut bytes = [0u8; 32];
            getrandom::fill(&mut bytes).unwrap();
            if let Ok(key) = SigningKey::from_slice(&bytes) {
                break key;
            }
        };
        let mut credential_id = vec![0u8; 16];
        getrandom::fill(&mut credential_id).unwrap();
        let passkey = Self {
            key,
            credential_id,
            user_handle: unb64(public_key["user"]["id"].as_str().unwrap()),
            counter: 0,
        };
        let rp_id = public_key["rp"]["id"].as_str().unwrap();
        let point = passkey.key.verifying_key().to_sec1_point(false);
        let point = point.as_bytes();
        let mut cose = Vec::new();
        cbor_head(&mut cose, 5, 5);
        cbor_int(&mut cose, 1);
        cbor_int(&mut cose, 2); // kty: EC2
        cbor_int(&mut cose, 3);
        cbor_int(&mut cose, -7); // alg: ES256
        cbor_int(&mut cose, -1);
        cbor_int(&mut cose, 1); // crv: P-256
        cbor_int(&mut cose, -2);
        cbor_bytes(&mut cose, &point[1..33]);
        cbor_int(&mut cose, -3);
        cbor_bytes(&mut cose, &point[33..65]);

        let mut auth_data = Sha256::digest(rp_id.as_bytes()).to_vec();
        auth_data.push(UP | UV | AT);
        auth_data.extend(passkey.counter.to_be_bytes());
        auth_data.extend([0u8; 16]); // aaguid
        auth_data.extend((passkey.credential_id.len() as u16).to_be_bytes());
        auth_data.extend(&passkey.credential_id);
        auth_data.extend(cose);

        let mut attestation = Vec::new();
        cbor_head(&mut attestation, 5, 3);
        cbor_text(&mut attestation, "fmt");
        cbor_text(&mut attestation, "none");
        cbor_text(&mut attestation, "attStmt");
        cbor_head(&mut attestation, 5, 0);
        cbor_text(&mut attestation, "authData");
        cbor_bytes(&mut attestation, &auth_data);

        let challenge = public_key["challenge"].as_str().unwrap();
        let credential = json!({
            "id": b64(&passkey.credential_id),
            "rawId": b64(&passkey.credential_id),
            "type": "public-key",
            "response": {
                "attestationObject": b64(&attestation),
                "clientDataJSON": b64(&client_data("webauthn.create", challenge)),
            },
            "extensions": {},
        });
        (passkey, credential)
    }

    /// Answers `navigator.credentials.get({ publicKey })`.
    pub fn get(&mut self, public_key: &Value) -> Value {
        self.counter += 1;
        let rp_id = public_key["rpId"].as_str().unwrap();
        let mut auth_data = Sha256::digest(rp_id.as_bytes()).to_vec();
        auth_data.push(UP | UV);
        auth_data.extend(self.counter.to_be_bytes());
        let client_data = client_data("webauthn.get", public_key["challenge"].as_str().unwrap());
        let mut signed = auth_data.clone();
        signed.extend(Sha256::digest(&client_data));
        let signature: Signature = self.key.sign(&signed);
        json!({
            "id": b64(&self.credential_id),
            "rawId": b64(&self.credential_id),
            "type": "public-key",
            "response": {
                "authenticatorData": b64(&auth_data),
                "clientDataJSON": b64(&client_data),
                "signature": b64(signature.to_der().as_bytes()),
                "userHandle": b64(&self.user_handle),
            },
            "extensions": {},
        })
    }
}
