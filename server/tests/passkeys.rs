//! Signing in with passkeys required, driven end to end with a software passkey.

mod common;

use axum::http::StatusCode;
use common::{Server, SoftPasskey};
use serde_json::{Value, json};

async fn options(server: &Server, ticket: &str) -> common::Answer {
    server
        .post("/v1/passkeys/options", None, json!({ "ticket": ticket }))
        .await
}

async fn finish(server: &Server, ticket: &str, credential: Value) -> common::Answer {
    server
        .post(
            "/v1/passkeys/finish",
            None,
            json!({ "ticket": ticket, "credential": credential }),
        )
        .await
}

async fn poll(server: &Server, ticket: &str) -> common::Answer {
    server
        .post("/v1/login/poll", None, json!({ "ticket": ticket }))
        .await
}

/// Signs up with a code and a new passkey; the passkey and the account's id.
async fn sign_up(server: &Server, email: &str) -> (SoftPasskey, i64) {
    let finished = server.code_sign_in(email).await;
    assert_eq!(finished.body["next"], "passkey");
    let ticket = finished.body["ticket"].as_str().unwrap();
    let opts = options(server, ticket).await;
    let (passkey, credential) = SoftPasskey::create(&opts.body["publicKey"]);
    assert_eq!(
        finish(server, ticket, credential).await.status,
        StatusCode::NO_CONTENT
    );
    let done = poll(server, ticket).await;
    (passkey, done.body["me"]["id"].as_i64().unwrap())
}

#[tokio::test]
async fn a_code_then_a_new_passkey_signs_in() {
    let server = Server::new(true);
    let finished = server.code_sign_in("sam@example.com").await;
    assert_eq!(finished.status, StatusCode::OK);
    assert_eq!(finished.body["next"], "passkey");
    let ticket = finished.body["ticket"].as_str().unwrap();

    assert_eq!(
        poll(&server, ticket).await.body,
        json!({ "status": "pending" })
    );

    let opts = options(&server, ticket).await;
    assert_eq!(opts.status, StatusCode::OK, "{:?}", opts.body);
    assert_eq!(opts.body["mode"], "create");
    let public_key = &opts.body["publicKey"];
    assert_eq!(public_key["rp"]["id"], "localhost");
    assert_eq!(public_key["user"]["name"], "sam@example.com");
    assert_eq!(
        public_key["authenticatorSelection"]["residentKey"],
        "required"
    );

    let (_, credential) = SoftPasskey::create(public_key);
    let done = finish(&server, ticket, credential).await;
    assert_eq!(done.status, StatusCode::NO_CONTENT, "{:?}", done.body);

    let polled = poll(&server, ticket).await;
    assert_eq!(polled.status, StatusCode::OK);
    assert_eq!(polled.body["status"], "done");
    assert_eq!(polled.body["me"]["email"], "sam@example.com");
    let session = polled.body["session"].as_str().unwrap();
    assert_eq!(server.get("/v1/me", session).await.status, StatusCode::OK);

    // The session comes out once; the ticket is gone after.
    let again = poll(&server, ticket).await;
    assert_eq!(again.status, StatusCode::GONE);
    assert_eq!(again.error(), "expired");
    assert_eq!(options(&server, ticket).await.status, StatusCode::GONE);
}

#[tokio::test]
async fn a_passkey_made_earlier_signs_in_without_email() {
    let server = Server::new(true);
    let (mut passkey, user) = sign_up(&server, "sam@example.com").await;

    let ticket = server.post("/v1/login/passkey", None, Value::Null).await;
    assert_eq!(ticket.status, StatusCode::OK, "{:?}", ticket.body);
    let ticket = ticket.body["ticket"].as_str().unwrap();
    let opts = options(&server, ticket).await;
    assert_eq!(opts.body["mode"], "get");
    assert_eq!(poll(&server, ticket).await.body["status"], "pending");

    let answer = passkey.get(&opts.body["publicKey"]);
    let done = finish(&server, ticket, answer).await;
    assert_eq!(done.status, StatusCode::NO_CONTENT, "{:?}", done.body);
    let polled = poll(&server, ticket).await;
    assert_eq!(polled.body["status"], "done");
    assert_eq!(polled.body["me"]["id"], user);

    // And again, on a fresh ticket, with the counter moved on.
    let ticket = server.post("/v1/login/passkey", None, Value::Null).await;
    let ticket = ticket.body["ticket"].as_str().unwrap();
    let opts = options(&server, ticket).await;
    let answer = passkey.get(&opts.body["publicKey"]);
    assert_eq!(
        finish(&server, ticket, answer).await.status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(poll(&server, ticket).await.body["me"]["id"], user);
}

#[tokio::test]
async fn a_forged_answer_is_refused_and_the_ticket_waits() {
    let server = Server::new(true);
    let (mut passkey, _) = sign_up(&server, "sam@example.com").await;
    let ticket = server.post("/v1/login/passkey", None, Value::Null).await;
    let ticket = ticket.body["ticket"].as_str().unwrap();
    let opts = options(&server, ticket).await;
    let mut answer = passkey.get(&opts.body["publicKey"]);
    // A signature from a different key.
    let (mut other, _) = SoftPasskey::create(&json!({
        "rp": { "id": "localhost" }, "user": { "id": "AA" }, "challenge": "AA"
    }));
    answer["response"]["signature"] =
        other.get(&opts.body["publicKey"])["response"]["signature"].clone();
    let refused = finish(&server, ticket, answer).await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST);
    assert_eq!(refused.error(), "invalid");
    assert_eq!(poll(&server, ticket).await.body["status"], "pending");

    // The options were good for one try; finishing again without new ones runs out.
    let answer = passkey.get(&opts.body["publicKey"]);
    assert_eq!(
        finish(&server, ticket, answer).await.status,
        StatusCode::GONE
    );

    // Fresh options work.
    let opts = options(&server, ticket).await;
    let answer = passkey.get(&opts.body["publicKey"]);
    assert_eq!(
        finish(&server, ticket, answer).await.status,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn a_passkey_whose_account_is_gone_finds_nobody() {
    let server = Server::new(true);
    let finished = server.code_sign_in("sam@example.com").await;
    let ticket = finished.body["ticket"].as_str().unwrap();
    let opts = options(&server, ticket).await;
    let (mut passkey, credential) = SoftPasskey::create(&opts.body["publicKey"]);
    finish(&server, ticket, credential).await;
    let session = poll(&server, ticket).await.body["session"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        server.delete("/v1/me", &session).await.status,
        StatusCode::NO_CONTENT
    );

    let ticket = server.post("/v1/login/passkey", None, Value::Null).await;
    let ticket = ticket.body["ticket"].as_str().unwrap();
    let opts = options(&server, ticket).await;
    let refused = finish(&server, ticket, passkey.get(&opts.body["publicKey"])).await;
    assert_eq!(refused.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn tickets_run_out_after_fifteen_minutes() {
    let server = Server::new(true);
    let finished = server.code_sign_in("sam@example.com").await;
    let ticket = finished.body["ticket"].as_str().unwrap();
    let opts = options(&server, ticket).await;
    let (_, credential) = SoftPasskey::create(&opts.body["publicKey"]);

    server.clock.advance(15 * 60 + 1);
    let late = finish(&server, ticket, credential).await;
    assert_eq!(late.status, StatusCode::GONE);
    assert_eq!(late.error(), "expired");
    assert_eq!(options(&server, ticket).await.status, StatusCode::GONE);
    assert_eq!(poll(&server, ticket).await.status, StatusCode::GONE);
    assert_eq!(poll(&server, "made-up").await.status, StatusCode::GONE);
}

#[tokio::test]
async fn a_finished_ticket_cant_make_a_second_passkey() {
    let server = Server::new(true);
    let finished = server.code_sign_in("sam@example.com").await;
    let ticket = finished.body["ticket"].as_str().unwrap();
    let opts = options(&server, ticket).await;
    let (_, credential) = SoftPasskey::create(&opts.body["publicKey"]);
    assert_eq!(
        finish(&server, ticket, credential).await.status,
        StatusCode::NO_CONTENT
    );
    let again = options(&server, ticket).await;
    assert_eq!(again.status, StatusCode::GONE);
}

#[tokio::test]
async fn the_page_is_one_self_contained_file() {
    let server = Server::new(true);
    let page = server
        .send(axum::http::Method::GET, "/passkey", None, None, None)
        .await;
    assert_eq!(page.status, StatusCode::OK);
    let html = page.body.as_str().unwrap();
    assert!(html.contains("navigator.credentials.create"));
    assert!(html.contains("/v1/passkeys/options"));
    assert!(!html.contains("src=\"http"));
    assert!(!html.contains("href=\"http"));
}

#[tokio::test]
async fn passkey_sign_in_is_off_when_passkeys_are() {
    let server = Server::new(false);
    let answer = server.post("/v1/login/passkey", None, Value::Null).await;
    assert_eq!(answer.status, StatusCode::NOT_FOUND);
}
