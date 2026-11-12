//! Settings kept the same on every computer, and the commands a server owner runs on the
//! database they live in: backup and delete-account.

mod common;

use axum::http::StatusCode;
use common::{START, Server};
use monster_core::api::{SETTINGS_BYTES, SettingsSaved, SyncedSettings};
use serde_json::json;

#[tokio::test]
async fn saving_and_fetching_settings() {
    let server = Server::new(false);
    let (session, _) = server.sign_in("sam@example.com").await;

    let none = server.get("/v1/settings", &session).await;
    assert_eq!(none.status, StatusCode::NOT_FOUND);
    assert_eq!(none.error(), "not_found");

    let data = json!({ "theme": "dark", "volume": 0.8, "nested": { "list": [1, 2, 3] } });
    let first = server
        .put("/v1/settings", &session, json!({ "base": 0, "data": data }))
        .await;
    assert_eq!(first.status, StatusCode::OK, "{:?}", first.body);
    let first: SettingsSaved = serde_json::from_value(first.body).unwrap();
    assert_eq!(
        first,
        SettingsSaved {
            version: 1,
            updated_at: START
        }
    );

    let got = server.get("/v1/settings", &session).await;
    assert_eq!(got.status, StatusCode::OK);
    let got: SyncedSettings = serde_json::from_value(got.body).unwrap();
    assert_eq!(
        got,
        SyncedSettings {
            version: 1,
            updated_at: START,
            data: data.clone()
        }
    );

    server.clock.advance(5);
    let changed = json!({ "theme": "light" });
    let second = server
        .put(
            "/v1/settings",
            &session,
            json!({ "base": 1, "data": changed }),
        )
        .await;
    assert_eq!(second.status, StatusCode::OK, "{:?}", second.body);
    assert_eq!(
        second.body,
        json!({ "version": 2, "updated_at": START + 5 })
    );
    let got = server.get("/v1/settings", &session).await.body;
    assert_eq!(got["version"], 2);
    assert_eq!(got["data"], changed);
}

#[tokio::test]
async fn settings_come_back_as_the_text_they_were_saved_as() {
    let server = Server::new(false);
    let (session, _) = server.sign_in("sam@example.com").await;
    // Keys out of order and an exact decimal, which a round trip through a JSON tree could change.
    let data = r#"{"zeta":1,"alpha":{"b":2,"a":1},"price":1.10}"#;
    let saved = server
        .put_text(
            "/v1/settings",
            &session,
            format!(r#"{{"base":0,"data":{data}}}"#),
        )
        .await;
    assert_eq!(saved.status, StatusCode::OK, "{:?}", saved.body);
    let (status, text) = server.get_text("/v1/settings", &session).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        text,
        format!(r#"{{"version":1,"updated_at":{START},"data":{data}}}"#)
    );
}

#[tokio::test]
async fn a_stale_base_is_a_conflict_and_changes_nothing() {
    let server = Server::new(false);
    let (session, _) = server.sign_in("sam@example.com").await;
    let kept = json!({ "theme": "dark" });
    server
        .put("/v1/settings", &session, json!({ "base": 0, "data": kept }))
        .await;
    server
        .put("/v1/settings", &session, json!({ "base": 1, "data": kept }))
        .await;

    // Another computer last saw version 1, or none at all, or one from the future.
    for base in [1, 0, 3, -1] {
        let stale = server
            .put(
                "/v1/settings",
                &session,
                json!({ "base": base, "data": { "theme": "light" } }),
            )
            .await;
        assert_eq!(stale.status, StatusCode::CONFLICT, "base {base}");
        assert_eq!(stale.error(), "conflict");
    }
    let got = server.get("/v1/settings", &session).await.body;
    assert_eq!(got["version"], 2);
    assert_eq!(got["data"], kept);

    // With nothing saved, only base 0 is taken.
    let (kim, _) = server.sign_in("kim@example.com").await;
    let early = server
        .put("/v1/settings", &kim, json!({ "base": 1, "data": {} }))
        .await;
    assert_eq!(early.status, StatusCode::CONFLICT);
    assert_eq!(
        server.get("/v1/settings", &kim).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn settings_must_be_an_object() {
    let server = Server::new(false);
    let (session, _) = server.sign_in("sam@example.com").await;
    for data in [
        json!([1, 2]),
        json!("dark"),
        json!(3),
        json!(null),
        json!(true),
    ] {
        let wrong = server
            .put("/v1/settings", &session, json!({ "base": 0, "data": data }))
            .await;
        assert_eq!(wrong.status, StatusCode::BAD_REQUEST, "{data}");
        assert_eq!(wrong.error(), "invalid");
    }
    let missing = server
        .put("/v1/settings", &session, json!({ "base": 0 }))
        .await;
    assert_eq!(missing.status, StatusCode::BAD_REQUEST);
    assert_eq!(
        server.get("/v1/settings", &session).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn settings_have_a_size_limit() {
    let server = Server::new(false);
    let (session, _) = server.sign_in("sam@example.com").await;
    // {"s":"…"} is 8 bytes around the string.
    let exactly = json!({ "s": "x".repeat(SETTINGS_BYTES - 8) });
    assert_eq!(exactly.to_string().len(), SETTINGS_BYTES);
    let fits = server
        .put(
            "/v1/settings",
            &session,
            json!({ "base": 0, "data": exactly }),
        )
        .await;
    assert_eq!(fits.status, StatusCode::OK, "{:?}", fits.body);

    let over = json!({ "s": "x".repeat(SETTINGS_BYTES - 7) });
    let too_large = server
        .put("/v1/settings", &session, json!({ "base": 1, "data": over }))
        .await;
    assert_eq!(too_large.status, StatusCode::BAD_REQUEST);
    assert_eq!(too_large.error(), "invalid");
    assert!(
        too_large.body["message"]
            .as_str()
            .unwrap()
            .contains("too large")
    );
    assert_eq!(
        server.get("/v1/settings", &session).await.body["version"],
        1
    );
}

#[tokio::test]
async fn accounts_dont_see_each_others_settings() {
    let server = Server::new(false);
    let (sam, _) = server.sign_in("sam@example.com").await;
    server.clock.advance(61);
    let (kim, _) = server.sign_in("kim@example.com").await;
    server
        .put(
            "/v1/settings",
            &sam,
            json!({ "base": 0, "data": { "who": "sam" } }),
        )
        .await;
    assert_eq!(
        server.get("/v1/settings", &kim).await.status,
        StatusCode::NOT_FOUND
    );
    // Kim's versions count on their own.
    let kims = server
        .put(
            "/v1/settings",
            &kim,
            json!({ "base": 0, "data": { "who": "kim" } }),
        )
        .await;
    assert_eq!(kims.body["version"], 1);
    assert_eq!(
        server.get("/v1/settings", &sam).await.body["data"]["who"],
        "sam"
    );
    assert_eq!(
        server.get("/v1/settings", &kim).await.body["data"]["who"],
        "kim"
    );
}

#[tokio::test]
async fn settings_need_a_session() {
    let server = Server::new(false);
    let get = server
        .send(axum::http::Method::GET, "/v1/settings", None, None, None)
        .await;
    assert_eq!(get.status, StatusCode::UNAUTHORIZED);
    let put = server
        .send(
            axum::http::Method::PUT,
            "/v1/settings",
            None,
            None,
            Some(json!({ "base": 0, "data": {} })),
        )
        .await;
    assert_eq!(put.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn deleting_the_account_removes_its_settings() {
    let server = Server::new(false);
    let (sam, _) = server.sign_in("sam@example.com").await;
    server
        .put(
            "/v1/settings",
            &sam,
            json!({ "base": 0, "data": { "who": "sam" } }),
        )
        .await;
    assert_eq!(
        server.delete("/v1/me", &sam).await.status,
        StatusCode::NO_CONTENT
    );
    let (again, _) = server.sign_in("sam@example.com").await;
    assert_eq!(
        server.get("/v1/settings", &again).await.status,
        StatusCode::NOT_FOUND
    );
    // And nothing of it is left in the file.
    let db = rusqlite::Connection::open(server.data_dir().join("monster.sqlite")).unwrap();
    let rows: i64 = db
        .query_row("SELECT COUNT(*) FROM settings", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 0);
}

#[tokio::test]
async fn saving_is_rate_limited_per_account() {
    let server = Server::new(false);
    let (sam, _) = server.sign_in("sam@example.com").await;
    server.clock.advance(61);
    let (kim, _) = server.sign_in("kim@example.com").await;
    for base in 0..30 {
        let saved = server
            .put("/v1/settings", &sam, json!({ "base": base, "data": {} }))
            .await;
        assert_eq!(saved.status, StatusCode::OK, "save {base}");
    }
    let too_often = server
        .put("/v1/settings", &sam, json!({ "base": 30, "data": {} }))
        .await;
    assert_eq!(too_often.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(too_often.error(), "slow_down");
    // Fetching still works, and another account isn't held back.
    assert_eq!(server.get("/v1/settings", &sam).await.body["version"], 30);
    assert_eq!(
        server
            .put("/v1/settings", &kim, json!({ "base": 0, "data": {} }))
            .await
            .status,
        StatusCode::OK
    );

    server.clock.advance(61);
    let later = server
        .put("/v1/settings", &sam, json!({ "base": 30, "data": {} }))
        .await;
    assert_eq!(later.status, StatusCode::OK);
    assert_eq!(later.body["version"], 31);
}

#[tokio::test]
async fn a_backup_is_a_whole_copy_that_opens() {
    let server = Server::new(false);
    let (sam, _) = server.sign_in("sam@example.com").await;
    server.add_can(&sam, "original", "k1").await;
    server
        .put(
            "/v1/settings",
            &sam,
            json!({ "base": 0, "data": { "who": "sam" } }),
        )
        .await;

    let out = tempfile::tempdir().unwrap();
    let dest = out.path().join("copy.sqlite");
    flexspeak_monster::backup(&server.data_dir(), &dest).unwrap();
    let copy = rusqlite::Connection::open(&dest).unwrap();
    let (users, cans, settings): (i64, i64, String) = copy
        .query_row(
            "SELECT (SELECT COUNT(*) FROM users), (SELECT COUNT(*) FROM cans),
                    (SELECT data FROM settings)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!((users, cans), (1, 1));
    assert_eq!(settings, r#"{"who":"sam"}"#);

    // It never overwrites, and says so when there's nothing to copy.
    assert!(
        flexspeak_monster::backup(&server.data_dir(), &dest)
            .unwrap_err()
            .contains("already exists")
    );
    assert!(
        flexspeak_monster::backup(out.path(), &out.path().join("other.sqlite"))
            .unwrap_err()
            .contains("no database")
    );
}

#[tokio::test]
async fn the_owner_can_delete_an_account_by_its_address() {
    let server = Server::new(false);
    let (sam, _) = server.sign_in("sam@example.com").await;
    server.clock.advance(61);
    let (kim, _) = server.sign_in("kim@example.com").await;
    server.add_can(&sam, "original", "k1").await;
    server
        .put(
            "/v1/settings",
            &sam,
            json!({ "base": 0, "data": { "who": "sam" } }),
        )
        .await;

    assert_eq!(
        flexspeak_monster::delete_account(&server.data_dir(), " Sam@Example.com "),
        Ok(true)
    );
    assert_eq!(
        server.get("/v1/me", &sam).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(server.get("/v1/me", &kim).await.status, StatusCode::OK);
    let db = rusqlite::Connection::open(server.data_dir().join("monster.sqlite")).unwrap();
    let left: i64 = db
        .query_row(
            "SELECT (SELECT COUNT(*) FROM users) + (SELECT COUNT(*) FROM cans)
                  + (SELECT COUNT(*) FROM settings) + (SELECT COUNT(*) FROM sessions)",
            [],
            |row| row.get(0),
        )
        .unwrap();
    // Kim's account and session.
    assert_eq!(left, 2);
    assert_eq!(
        flexspeak_monster::delete_account(&server.data_dir(), "sam@example.com"),
        Ok(false)
    );
}
