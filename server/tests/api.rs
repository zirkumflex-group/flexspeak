//! The app's calls, on a server with passkeys off and a fixed sign-in code.

mod common;

use axum::http::{Method, StatusCode};
use base64::{Engine, engine::general_purpose::STANDARD};
use common::{CODE, Server};
use monster_core::{Rarity, Standing, api::link_message};
use serde_json::{Value, json};
use tsproto_types::crypto::EccKeyPrivP256;

#[tokio::test]
async fn signing_in_with_a_code_makes_an_account() {
    let server = Server::new(false);
    let finished = server.code_sign_in("  Sam@Example.com ").await;
    assert_eq!(finished.status, StatusCode::OK, "{:?}", finished.body);
    assert_eq!(finished.body["next"], "signed_in");
    let me = &finished.body["me"];
    assert_eq!(me["name"], "sam");
    assert_eq!(me["email"], "sam@example.com");
    assert_eq!(me["hidden"], false);
    assert_eq!(me["imported"], false);
    assert_eq!(me["identities"], json!([]));
    let session = finished.body["session"].as_str().unwrap();

    let got = server.get("/v1/me", session).await;
    assert_eq!(got.status, StatusCode::OK);
    assert_eq!(&got.body, me);

    // A second sign-in finds the same account.
    server.clock.advance(61);
    let (_, id) = server.sign_in("sam@example.com").await;
    assert_eq!(id, me["id"].as_i64().unwrap());

    assert_eq!(
        server.delete("/v1/session", session).await.status,
        StatusCode::NO_CONTENT
    );
    let after = server.get("/v1/me", session).await;
    assert_eq!(after.status, StatusCode::UNAUTHORIZED);
    assert_eq!(after.error(), "signed_out");
}

#[tokio::test]
async fn calls_need_a_session() {
    let server = Server::new(false);
    for (method, path) in [
        (Method::GET, "/v1/me"),
        (Method::GET, "/v1/cans"),
        (Method::GET, "/v1/leaderboard"),
        (Method::POST, "/v1/identities/challenge"),
    ] {
        let answer = server.send(method, path, None, None, None).await;
        assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{path}");
        assert_eq!(answer.error(), "signed_out");
    }
    let made_up = server.get("/v1/me", "made-up").await;
    assert_eq!(made_up.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn health_and_unknown_routes() {
    let server = Server::new(false);
    let health = server.send(Method::GET, "/health", None, None, None).await;
    assert_eq!(health.status, StatusCode::OK);
    let nothing = server
        .send(Method::GET, "/v1/nothing", None, None, None)
        .await;
    assert_eq!(nothing.status, StatusCode::NOT_FOUND);
    assert_eq!(nothing.error(), "not_found");
    let bad = server
        .post(
            "/v1/login/start",
            None,
            json!({ "email": "not an address" }),
        )
        .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    assert_eq!(bad.error(), "invalid");
    let broken = server.post("/v1/login/start", None, json!({})).await;
    assert_eq!(broken.status, StatusCode::BAD_REQUEST);
    assert_eq!(broken.error(), "invalid");
}

#[tokio::test]
async fn wrong_codes_count_down_then_cancel_the_code() {
    let server = Server::new(false);
    let email = "sam@example.com";
    server
        .post("/v1/login/start", None, json!({ "email": email }))
        .await;
    for left in (0..5).rev() {
        let wrong = server
            .post(
                "/v1/login/finish",
                None,
                json!({ "email": email, "code": "000000" }),
            )
            .await;
        assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
        assert_eq!(wrong.error(), "wrong_code");
        assert_eq!(wrong.body["tries_left"], left);
    }
    let right_too_late = server
        .post(
            "/v1/login/finish",
            None,
            json!({ "email": email, "code": CODE }),
        )
        .await;
    assert_eq!(right_too_late.status, StatusCode::GONE);
    assert_eq!(right_too_late.error(), "expired");
}

#[tokio::test]
async fn a_code_runs_out_after_ten_minutes() {
    let server = Server::new(false);
    let email = "sam@example.com";
    server
        .post("/v1/login/start", None, json!({ "email": email }))
        .await;
    server.clock.advance(10 * 60 + 1);
    let late = server
        .post(
            "/v1/login/finish",
            None,
            json!({ "email": email, "code": CODE }),
        )
        .await;
    assert_eq!(late.status, StatusCode::GONE);

    // A code never asked for is the same as one that ran out.
    let never = server
        .post(
            "/v1/login/finish",
            None,
            json!({ "email": "kim@example.com", "code": CODE }),
        )
        .await;
    assert_eq!(never.status, StatusCode::GONE);

    // A code is used once.
    let (_, _) = server.sign_in("lee@example.com").await;
    let reused = server
        .post(
            "/v1/login/finish",
            None,
            json!({ "email": "lee@example.com", "code": CODE }),
        )
        .await;
    assert_eq!(reused.status, StatusCode::GONE);
}

#[tokio::test]
async fn codes_are_rate_limited_by_address_and_network_address() {
    let server = Server::new(false);
    let start = |email: &'static str, ip: &'static str| {
        let server = &server;
        async move {
            server
                .send(
                    Method::POST,
                    "/v1/login/start",
                    None,
                    Some(ip),
                    Some(json!({ "email": email })),
                )
                .await
        }
    };
    assert_eq!(
        start("sam@example.com", "10.0.0.1").await.status,
        StatusCode::NO_CONTENT
    );
    let soon = start("sam@example.com", "10.0.0.1").await;
    assert_eq!(soon.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(soon.error(), "slow_down");
    for _ in 0..4 {
        server.clock.advance(61);
        assert_eq!(
            start("sam@example.com", "10.0.0.1").await.status,
            StatusCode::NO_CONTENT
        );
    }
    server.clock.advance(61);
    assert_eq!(
        start("sam@example.com", "10.0.0.1").await.status,
        StatusCode::TOO_MANY_REQUESTS,
        "a sixth code in an hour"
    );
    server.clock.advance(3600);
    assert_eq!(
        start("sam@example.com", "10.0.0.1").await.status,
        StatusCode::NO_CONTENT
    );

    // Twenty codes from one network address in an hour, then no more from there.
    let emails: Vec<&'static str> = (0..21)
        .map(|i| &*Box::leak(format!("person{i}@example.com").into_boxed_str()))
        .collect();
    for email in &emails[..20] {
        assert_eq!(
            start(email, "10.0.0.2").await.status,
            StatusCode::NO_CONTENT
        );
    }
    assert_eq!(
        start(emails[20], "10.0.0.2").await.status,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        start(emails[20], "10.0.0.3").await.status,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn sessions_last_thirty_days_from_their_last_use() {
    let server = Server::new(false);
    let (session, _) = server.sign_in("sam@example.com").await;
    let day = 24 * 3600;
    server.clock.advance(29 * day);
    assert_eq!(server.get("/v1/me", &session).await.status, StatusCode::OK);
    server.clock.advance(29 * day);
    assert_eq!(server.get("/v1/me", &session).await.status, StatusCode::OK);
    server.clock.advance(30 * day + 1);
    assert_eq!(
        server.get("/v1/me", &session).await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn names_and_hiding_can_change() {
    let server = Server::new(false);
    let (session, _) = server.sign_in("sam@example.com").await;
    let patched = server
        .send(
            Method::PATCH,
            "/v1/me",
            Some(&session),
            None,
            Some(json!({ "name": "  Sam M  " })),
        )
        .await;
    assert_eq!(patched.status, StatusCode::OK);
    assert_eq!(patched.body["name"], "Sam M");
    assert_eq!(patched.body["hidden"], false);
    let hidden = server
        .send(
            Method::PATCH,
            "/v1/me",
            Some(&session),
            None,
            Some(json!({ "hidden": true })),
        )
        .await;
    assert_eq!(hidden.body["name"], "Sam M");
    assert_eq!(hidden.body["hidden"], true);
    for bad in ["", "   ", &"x".repeat(33)] {
        let refused = server
            .send(
                Method::PATCH,
                "/v1/me",
                Some(&session),
                None,
                Some(json!({ "name": bad })),
            )
            .await;
        assert_eq!(refused.status, StatusCode::BAD_REQUEST, "{bad:?}");
    }
}

#[tokio::test]
async fn adding_and_removing_cans() {
    let server = Server::new(false);
    let (session, _) = server.sign_in("sam@example.com").await;

    let first = server.add_can(&session, "mango-loco", "k1").await;
    assert_eq!(first.status, StatusCode::OK, "{:?}", first.body);
    assert_eq!(first.body["id"], 1);
    assert_eq!(first.body["flavour"], "mango-loco");
    assert_eq!(first.body["at"], common::START);
    assert_eq!(first.body["legacy"], false);
    let rarity: Rarity = serde_json::from_value(first.body["rarity"].clone()).unwrap();
    assert!(Rarity::ALL.contains(&rarity));

    // Sending the same key again returns the same can and adds nothing.
    server.clock.advance(5);
    let again = server.add_can(&session, "mango-loco", "k1").await;
    assert_eq!(again.body, first.body);
    let listed = server.get("/v1/cans", &session).await;
    assert_eq!(listed.body["cans"], json!([first.body]));

    let second = server.add_can(&session, "original", "k2").await;
    assert_eq!(second.body["id"], 2);
    assert_eq!(
        server.delete("/v1/cans/2", &session).await.status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        server.delete("/v1/cans/2", &session).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        server.delete("/v1/cans/99", &session).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        server.delete("/v1/cans/abc", &session).await.status,
        StatusCode::NOT_FOUND
    );

    // A removed can's number isn't given again.
    let third = server.add_can(&session, "original", "k3").await;
    assert_eq!(third.body["id"], 3);
    let listed = server.get("/v1/cans", &session).await;
    let ids: Vec<i64> = listed.body["cans"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_i64().unwrap())
        .collect();
    assert_eq!(ids, [1, 3]);

    let unknown = server.add_can(&session, "pumpkin-spice", "k4").await;
    assert_eq!(unknown.status, StatusCode::BAD_REQUEST);
    assert_eq!(unknown.error(), "invalid");

    // Someone else's numbers start from 1, and they can't remove this person's cans.
    server.clock.advance(61);
    let (other, _) = server.sign_in("kim@example.com").await;
    assert_eq!(server.add_can(&other, "original", "k1").await.body["id"], 1);
    assert_eq!(
        server.delete("/v1/cans/3", &other).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        server.get("/v1/cans", &session).await.body["cans"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn adding_is_limited_to_ten_cans_a_minute() {
    let server = Server::new(false);
    let (session, _) = server.sign_in("sam@example.com").await;
    for i in 0..10 {
        let added = server.add_can(&session, "original", &format!("k{i}")).await;
        assert_eq!(added.status, StatusCode::OK);
    }
    let too_many = server.add_can(&session, "original", "k10").await;
    assert_eq!(too_many.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(too_many.error(), "slow_down");
    // A retry of a can already added still answers.
    assert_eq!(
        server.add_can(&session, "original", "k3").await.status,
        StatusCode::OK
    );
    server.clock.advance(61);
    assert_eq!(
        server.add_can(&session, "original", "k10").await.body["id"],
        11
    );
}

#[tokio::test]
async fn an_import_happens_once_and_isnt_counted() {
    let server = Server::new(false);
    let (session, user) = server.sign_in("sam@example.com").await;
    let added = server.add_can(&session, "original", "k1").await;

    let cans = json!({ "cans": [
        { "flavour": "ultra-gold", "at": common::START - 200, "rarity": "legendary", "seed": 7 },
        { "flavour": "khaotic", "at": common::START - 300, "rarity": "mythic", "seed": 8 },
    ]});
    let imported = server
        .post("/v1/cans/import", Some(&session), cans.clone())
        .await;
    assert_eq!(imported.status, StatusCode::OK, "{:?}", imported.body);
    assert_eq!(imported.body, json!({ "imported": 2 }));
    assert_eq!(server.get("/v1/me", &session).await.body["imported"], true);

    let again = server.post("/v1/cans/import", Some(&session), cans).await;
    assert_eq!(again.status, StatusCode::CONFLICT);
    assert_eq!(again.error(), "taken");

    // Oldest first; legacy cans keep the card they came with and get the next numbers.
    let listed = server.get("/v1/cans", &session).await.body["cans"].clone();
    assert_eq!(
        listed,
        json!([
            { "id": 2, "flavour": "khaotic", "at": common::START - 300, "rarity": "mythic", "seed": 8, "legacy": true },
            { "id": 3, "flavour": "ultra-gold", "at": common::START - 200, "rarity": "legendary", "seed": 7, "legacy": true },
            added.body,
        ])
    );
    assert_eq!(
        server.add_can(&session, "original", "k2").await.body["id"],
        4
    );

    // The leaderboard counts only the server's own two cans.
    let board = server.get("/v1/leaderboard?by=flavours", &session).await;
    let row = &board.body["rows"][0];
    assert_eq!(row["user"], user);
    assert_eq!(row["standing"]["flavours"], 1);
    let server_rolled: Vec<Rarity> = listed
        .as_array()
        .unwrap()
        .iter()
        .chain([&server.get("/v1/cans", &session).await.body["cans"][3]])
        .filter(|c| c["legacy"] == false)
        .map(|c| serde_json::from_value(c["rarity"].clone()).unwrap())
        .collect();
    let expected = Standing::count(server_rolled.iter().map(|r| ("original", *r)));
    assert_eq!(row["standing"]["legendary"], expected.legendary);
    assert_eq!(row["standing"]["cards"], expected.cards);
}

#[tokio::test]
async fn an_import_is_checked() {
    let server = Server::new(false);
    let (session, _) = server.sign_in("sam@example.com").await;
    let bad_flavour = json!({ "cans": [
        { "flavour": "pumpkin-spice", "at": common::START - 10, "rarity": "common", "seed": 1 },
    ]});
    let refused = server
        .post("/v1/cans/import", Some(&session), bad_flavour)
        .await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST);
    let can =
        json!({ "flavour": "original", "at": common::START - 10, "rarity": "common", "seed": 1 });
    let too_many = json!({ "cans": vec![can; 5001] });
    let refused = server
        .post("/v1/cans/import", Some(&session), too_many)
        .await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST);
    // A refused import doesn't use up the one import.
    assert_eq!(server.get("/v1/me", &session).await.body["imported"], false);
    let empty = server
        .post("/v1/cans/import", Some(&session), json!({ "cans": [] }))
        .await;
    assert_eq!(empty.body["imported"], 0);
}

#[tokio::test]
async fn the_leaderboard_ranks_collections_and_leaves_out_hidden_people() {
    let server = Server::new(false);
    let mut people = Vec::new();
    for email in [
        "ann@example.com",
        "bob@example.com",
        "cat@example.com",
        "dan@example.com",
    ] {
        people.push(server.sign_in(email).await);
        server.clock.advance(61);
    }
    let [(ann, ann_id), (bob, bob_id), (cat, cat_id), (dan, dan_id)] = &people[..] else {
        unreachable!()
    };
    for (i, flavour) in ["original", "khaotic", "nitro"].iter().enumerate() {
        server.add_can(ann, flavour, &format!("a{i}")).await;
        server.add_can(cat, flavour, &format!("c{i}")).await;
    }
    for i in 0..5 {
        server.add_can(bob, "original", &format!("b{i}")).await;
    }
    // Dan's four flavours would lead, but Dan is hidden.
    for (i, flavour) in ["original", "khaotic", "nitro", "lo-carb"]
        .iter()
        .enumerate()
    {
        server.add_can(dan, flavour, &format!("d{i}")).await;
    }
    server
        .send(
            Method::PATCH,
            "/v1/me",
            Some(dan),
            None,
            Some(json!({ "hidden": true })),
        )
        .await;

    let board = server.get("/v1/leaderboard?by=flavours", bob).await;
    assert_eq!(board.status, StatusCode::OK);
    let rows = board.body["rows"].as_array().unwrap();
    let order: Vec<i64> = rows.iter().map(|r| r["user"].as_i64().unwrap()).collect();
    // Ann and Cat tie on three flavours; Ann's account is older.
    assert_eq!(order, [*ann_id, *cat_id, *bob_id]);
    let you: Vec<bool> = rows.iter().map(|r| r["you"].as_bool().unwrap()).collect();
    assert_eq!(you, [false, false, true]);
    assert_eq!(rows[0]["name"], "ann");
    assert_eq!(rows[2]["standing"]["flavours"], 1);

    // Every standing matches the person's own cans.
    for (session, id) in [(ann, ann_id), (bob, bob_id), (cat, cat_id)] {
        let cans = server.get("/v1/cans", session).await.body["cans"].clone();
        let cans: Vec<(String, Rarity)> = cans
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                (
                    c["flavour"].as_str().unwrap().to_string(),
                    serde_json::from_value(c["rarity"].clone()).unwrap(),
                )
            })
            .collect();
        let expected = Standing::count(cans.iter().map(|(f, r)| (f.as_str(), *r)));
        let row = rows.iter().find(|r| r["user"] == *id).unwrap();
        assert_eq!(
            serde_json::from_value::<Standing>(row["standing"].clone()).unwrap(),
            expected
        );
    }

    // The default ranking is by collection, and it is sorted.
    let board = server.get("/v1/leaderboard", ann).await;
    let scores: Vec<i64> = board.body["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["standing"]["cards"].as_i64().unwrap())
        .collect();
    assert!(scores.windows(2).all(|w| w[0] >= w[1]), "{scores:?}");
    let bad = server.get("/v1/leaderboard?by=cans", ann).await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);

    // Hidden people are off the board and closed to others, but not to themselves.
    let hidden = server.get(&format!("/v1/users/{dan_id}/cans"), ann).await;
    assert_eq!(hidden.status, StatusCode::NOT_FOUND);
    assert_eq!(hidden.error(), "not_found");
    let own = server.get(&format!("/v1/users/{dan_id}/cans"), dan).await;
    assert_eq!(own.status, StatusCode::OK);
    assert_eq!(own.body["cans"].as_array().unwrap().len(), 4);
    let dan_board = server.get("/v1/leaderboard", dan).await;
    assert!(
        dan_board.body["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["you"] == false)
    );
}

#[tokio::test]
async fn another_persons_collection() {
    let server = Server::new(false);
    let (sam, sam_id) = server.sign_in("sam@example.com").await;
    server.clock.advance(61);
    let (kim, _) = server.sign_in("kim@example.com").await;
    let first = server.add_can(&sam, "original", "k1").await.body;
    server.add_can(&sam, "nitro", "k2").await;
    server.delete("/v1/cans/2", &sam).await;
    let third = server.add_can(&sam, "khaotic", "k3").await.body;

    let board = server.get(&format!("/v1/users/{sam_id}/cans"), &kim).await;
    assert_eq!(board.status, StatusCode::OK);
    assert_eq!(
        board.body,
        json!({ "user": sam_id, "name": "sam", "cans": [first, third] })
    );
    assert_eq!(
        server.get("/v1/users/9999/cans", &kim).await.status,
        StatusCode::NOT_FOUND
    );
}

/// Links a new TeamSpeak identity to the account; the answer and the key.
async fn link(server: &Server, session: &str, key: &EccKeyPrivP256) -> common::Answer {
    let challenge = server
        .post("/v1/identities/challenge", Some(session), Value::Null)
        .await;
    assert_eq!(challenge.status, StatusCode::OK);
    let challenge = challenge.body["challenge"].as_str().unwrap();
    link_with(server, session, key, challenge, &link_message(challenge)).await
}

async fn link_with(
    server: &Server,
    session: &str,
    key: &EccKeyPrivP256,
    challenge: &str,
    signed: &[u8],
) -> common::Answer {
    let signature = key.clone().sign(signed);
    server
        .post(
            "/v1/identities",
            Some(session),
            json!({
                "challenge": challenge,
                "public_key": key.to_pub().to_ts(),
                "signature": STANDARD.encode(signature),
                "source": "Flexspeak",
            }),
        )
        .await
}

#[tokio::test]
async fn linking_a_teamspeak_identity() {
    let server = Server::new(false);
    let (sam, _) = server.sign_in("sam@example.com").await;
    let key = EccKeyPrivP256::create();
    let linked = link(&server, &sam, &key).await;
    assert_eq!(linked.status, StatusCode::OK, "{:?}", linked.body);
    let identities = linked.body["identities"].as_array().unwrap();
    assert_eq!(identities.len(), 1);
    assert_eq!(identities[0]["uid"], key.to_pub().get_uid());
    assert_eq!(identities[0]["source"], "Flexspeak");
    assert_eq!(identities[0]["linked_at"], common::START);
    assert_eq!(
        server.get("/v1/me", &sam).await.body["identities"],
        linked.body["identities"]
    );

    // Linking it again is harmless.
    let again = link(&server, &sam, &key).await;
    assert_eq!(again.body["identities"].as_array().unwrap().len(), 1);

    // Unlinking.
    let uid = key.to_pub().get_uid();
    let unlinked = server
        .post("/v1/identities/unlink", Some(&sam), json!({ "uid": uid }))
        .await;
    assert_eq!(unlinked.status, StatusCode::OK);
    assert_eq!(unlinked.body["identities"], json!([]));
    let gone = server
        .post("/v1/identities/unlink", Some(&sam), json!({ "uid": uid }))
        .await;
    assert_eq!(gone.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_challenge_works_once_for_its_own_account_and_runs_out() {
    let server = Server::new(false);
    let (sam, _) = server.sign_in("sam@example.com").await;
    server.clock.advance(61);
    let (kim, _) = server.sign_in("kim@example.com").await;
    let key = EccKeyPrivP256::create();
    let challenge = server
        .post("/v1/identities/challenge", Some(&sam), Value::Null)
        .await
        .body["challenge"]
        .as_str()
        .unwrap()
        .to_string();
    let message = link_message(&challenge);

    // Someone else's challenge doesn't work, and doesn't use it up.
    let theirs = link_with(&server, &kim, &key, &challenge, &message).await;
    assert_eq!(theirs.status, StatusCode::GONE);

    // A bad signature fails and uses the challenge up.
    let bad = link_with(&server, &sam, &key, &challenge, b"something else").await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    assert_eq!(bad.error(), "invalid");
    let reused = link_with(&server, &sam, &key, &challenge, &message).await;
    assert_eq!(reused.status, StatusCode::GONE);
    assert_eq!(reused.error(), "expired");

    let challenge = server
        .post("/v1/identities/challenge", Some(&sam), Value::Null)
        .await
        .body["challenge"]
        .as_str()
        .unwrap()
        .to_string();
    server.clock.advance(5 * 60 + 1);
    let late = link_with(&server, &sam, &key, &challenge, &link_message(&challenge)).await;
    assert_eq!(late.status, StatusCode::GONE);

    // A key signed by another key's owner doesn't match.
    let challenge = server
        .post("/v1/identities/challenge", Some(&sam), Value::Null)
        .await
        .body["challenge"]
        .as_str()
        .unwrap()
        .to_string();
    let other = EccKeyPrivP256::create();
    let forged = server
        .post(
            "/v1/identities",
            Some(&sam),
            json!({
                "challenge": challenge,
                "public_key": key.to_pub().to_ts(),
                "signature": STANDARD.encode(other.sign(&link_message(&challenge))),
                "source": "Flexspeak",
            }),
        )
        .await;
    assert_eq!(forged.status, StatusCode::BAD_REQUEST);

    // A private key is refused.
    let challenge = server
        .post("/v1/identities/challenge", Some(&sam), Value::Null)
        .await
        .body["challenge"]
        .as_str()
        .unwrap()
        .to_string();
    let private = server
        .post(
            "/v1/identities",
            Some(&sam),
            json!({
                "challenge": challenge,
                "public_key": key.to_ts(),
                "signature": STANDARD.encode(key.clone().sign(&link_message(&challenge))),
                "source": "Flexspeak",
            }),
        )
        .await;
    assert_eq!(private.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn an_identity_belongs_to_one_account() {
    let server = Server::new(false);
    let (sam, _) = server.sign_in("sam@example.com").await;
    server.clock.advance(61);
    let (kim, _) = server.sign_in("kim@example.com").await;
    let key = EccKeyPrivP256::create();
    assert_eq!(link(&server, &sam, &key).await.status, StatusCode::OK);
    let taken = link(&server, &kim, &key).await;
    assert_eq!(taken.status, StatusCode::CONFLICT);
    assert_eq!(taken.error(), "taken");

    server
        .post(
            "/v1/identities/unlink",
            Some(&sam),
            json!({ "uid": key.to_pub().get_uid() }),
        )
        .await;
    assert_eq!(link(&server, &kim, &key).await.status, StatusCode::OK);
}

#[tokio::test]
async fn deleting_the_account_removes_everything() {
    let server = Server::new(false);
    let (sam, sam_id) = server.sign_in("sam@example.com").await;
    server.clock.advance(61);
    let (kim, _) = server.sign_in("kim@example.com").await;
    server.add_can(&sam, "original", "k1").await;
    server
        .post("/v1/cans/import", Some(&sam), json!({ "cans": [] }))
        .await;
    let key = EccKeyPrivP256::create();
    link(&server, &sam, &key).await;

    assert_eq!(
        server.delete("/v1/me", &sam).await.status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        server.get("/v1/me", &sam).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        server
            .get(&format!("/v1/users/{sam_id}/cans"), &kim)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let board = server.get("/v1/leaderboard", &kim).await;
    assert_eq!(board.body["rows"].as_array().unwrap().len(), 1);
    // The identity is free again.
    assert_eq!(link(&server, &kim, &key).await.status, StatusCode::OK);

    // Signing in with the address again starts a new, empty account.
    let (again, again_id) = server.sign_in("sam@example.com").await;
    assert_ne!(again_id, sam_id);
    let me = server.get("/v1/me", &again).await.body;
    assert_eq!(me["imported"], false);
    assert_eq!(me["identities"], json!([]));
    assert_eq!(server.get("/v1/cans", &again).await.body["cans"], json!([]));
    assert_eq!(server.add_can(&again, "original", "k1").await.body["id"], 1);
}
