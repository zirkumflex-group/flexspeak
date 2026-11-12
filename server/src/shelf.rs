//! Cans: adding, rolling, removing and importing them, and ranking collections.

use std::collections::HashMap;

use axum::{Json, extract::State, http::StatusCode};
use monster_core::{
    Rarity, Standing,
    api::{
        AddCan, Can, Cans, Collection, Import, Imported, Leaderboard, LeaderboardQuery,
        LeaderboardRow,
    },
    draw, flavour_by_id,
};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    AppState,
    error::{ApiError, ApiResult, Body, Path, Query},
    me::Caller,
};

/// Cans one account can add in a minute: enough for a round of drinks, too few for a script.
pub const ADDS_PER_MINUTE: i64 = 10;
/// The most cans one import can carry.
pub const IMPORT_MAX: usize = 5000;
/// How far ahead of the server's clock an imported can's time may be, for a computer whose
/// clock runs fast.
const IMPORT_FUTURE_SLACK: i64 = 24 * 3600;

fn rarity_text(rarity: Rarity) -> String {
    rarity.name().to_lowercase()
}

fn rarity_from(text: &str) -> Rarity {
    Rarity::ALL
        .into_iter()
        .find(|r| r.name().eq_ignore_ascii_case(text))
        .unwrap_or(Rarity::Common)
}

const CAN_COLUMNS: &str = "number, flavour, at, rarity, seed, legacy";

fn can_from(row: &Row) -> rusqlite::Result<Can> {
    Ok(Can {
        id: row.get(0)?,
        flavour: row.get(1)?,
        at: row.get(2)?,
        rarity: rarity_from(&row.get::<_, String>(3)?),
        seed: row.get(4)?,
        legacy: row.get(5)?,
    })
}

/// Someone's cans that aren't removed, oldest first.
fn cans_of(conn: &Connection, user: i64) -> rusqlite::Result<Vec<Can>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {CAN_COLUMNS} FROM cans WHERE user_id = ?1 AND removed_at IS NULL
         ORDER BY at, number"
    ))?;
    stmt.query_map([user], can_from)?.collect()
}

pub async fn list(State(state): State<AppState>, caller: Caller) -> ApiResult<Json<Cans>> {
    Ok(Json(Cans {
        cans: cans_of(&state.db(), caller.user)?,
    }))
}

pub async fn add(
    State(state): State<AppState>,
    caller: Caller,
    Body(req): Body<AddCan>,
) -> ApiResult<Json<Can>> {
    let Some((_, flavour)) = flavour_by_id(req.flavour.trim()) else {
        return Err(ApiError::invalid(format!(
            "There's no flavour {:?}.",
            req.flavour
        )));
    };
    let key = req.key.trim();
    if key.is_empty() || key.len() > 100 {
        return Err(ApiError::invalid("key is 1 to 100 characters."));
    }
    let now = state.now();
    let mut db = state.db();
    let tx = db.transaction()?;
    let earlier = tx
        .query_row(
            &format!("SELECT {CAN_COLUMNS} FROM cans WHERE user_id = ?1 AND add_key = ?2"),
            params![caller.user, key],
            can_from,
        )
        .optional()?;
    if let Some(can) = earlier {
        return Ok(Json(can));
    }
    let recent: i64 = tx.query_row(
        "SELECT COUNT(*) FROM cans WHERE user_id = ?1 AND legacy = 0 AND created_at > ?2",
        params![caller.user, now - 60],
        |row| row.get(0),
    )?;
    if recent >= ADDS_PER_MINUTE {
        return Err(ApiError::slow_down(format!(
            "That's {ADDS_PER_MINUTE} cans in a minute. Wait a moment before adding more."
        )));
    }
    let (number, roll_key): (i64, Vec<u8>) = tx.query_row(
        "UPDATE users SET last_can = last_can + 1 WHERE id = ?1 RETURNING last_can, roll_key",
        [caller.user],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let roll_key: [u8; 32] = roll_key.try_into().map_err(|_| ApiError::server())?;
    let (rarity, seed) = draw(&roll_key, flavour.id, now, number);
    tx.execute(
        "INSERT INTO cans (user_id, number, flavour, at, rarity, seed, add_key, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?4)",
        params![
            caller.user,
            number,
            flavour.id,
            now,
            rarity_text(rarity),
            seed,
            key
        ],
    )?;
    tx.commit()?;
    Ok(Json(Can {
        id: number,
        flavour: flavour.id.into(),
        at: now,
        rarity,
        seed,
        legacy: false,
    }))
}

pub async fn remove(
    State(state): State<AppState>,
    caller: Caller,
    Path(number): Path<i64>,
) -> ApiResult<StatusCode> {
    let removed = state.db().execute(
        "UPDATE cans SET removed_at = ?1
         WHERE user_id = ?2 AND number = ?3 AND removed_at IS NULL",
        params![state.now(), caller.user, number],
    )?;
    if removed == 0 {
        return Err(ApiError::not_found(
            "There's no such can in your collection.",
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn import(
    State(state): State<AppState>,
    caller: Caller,
    Body(req): Body<Import>,
) -> ApiResult<Json<Imported>> {
    if req.cans.len() > IMPORT_MAX {
        return Err(ApiError::invalid(format!(
            "An import holds at most {IMPORT_MAX} cans."
        )));
    }
    let now = state.now();
    for can in &req.cans {
        if flavour_by_id(&can.flavour).is_none() {
            return Err(ApiError::invalid(format!(
                "There's no flavour {:?}.",
                can.flavour
            )));
        }
        if can.at <= 0 || can.at > now + IMPORT_FUTURE_SLACK {
            return Err(ApiError::invalid(format!(
                "A can's time, {}, isn't believable.",
                can.at
            )));
        }
    }
    let mut cans: Vec<_> = req.cans.iter().collect();
    cans.sort_by_key(|c| c.at);

    let mut db = state.db();
    let tx = db.transaction()?;
    let (imported, mut number): (bool, i64) = tx.query_row(
        "SELECT imported, last_can FROM users WHERE id = ?1",
        [caller.user],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if imported {
        return Err(ApiError::taken(
            "This account's cans were imported already.",
        ));
    }
    {
        let mut insert = tx.prepare(
            "INSERT INTO cans (user_id, number, flavour, at, rarity, seed, legacy, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7)",
        )?;
        for can in &cans {
            number += 1;
            insert.execute(params![
                caller.user,
                number,
                can.flavour,
                can.at,
                rarity_text(can.rarity),
                can.seed,
                now
            ])?;
        }
    }
    tx.execute(
        "UPDATE users SET imported = 1, last_can = ?1 WHERE id = ?2",
        params![number, caller.user],
    )?;
    tx.commit()?;
    Ok(Json(Imported {
        imported: cans.len(),
    }))
}

pub async fn leaderboard(
    State(state): State<AppState>,
    caller: Caller,
    Query(query): Query<LeaderboardQuery>,
) -> ApiResult<Json<Leaderboard>> {
    let db = state.db();
    let mut people: Vec<(i64, String, i64)> = db
        .prepare("SELECT id, name, created_at FROM users WHERE hidden = 0")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<Result<_, _>>()?;
    // Only the cards the server rolled count: legacy cans came with their rarity.
    let mut cards: HashMap<i64, Vec<(String, Rarity)>> = HashMap::new();
    let mut stmt = db.prepare(
        "SELECT c.user_id, c.flavour, c.rarity FROM cans c JOIN users u ON u.id = c.user_id
         WHERE u.hidden = 0 AND c.legacy = 0 AND c.removed_at IS NULL",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    for row in rows {
        let (user, flavour, rarity) = row?;
        cards
            .entry(user)
            .or_default()
            .push((flavour, rarity_from(&rarity)));
    }
    let standing = |user: i64| match cards.get(&user) {
        Some(cans) => Standing::count(cans.iter().map(|(f, r)| (f.as_str(), *r))),
        None => Standing::default(),
    };
    let by = query.by;
    let mut rows: Vec<(i64, LeaderboardRow)> = people
        .drain(..)
        .map(|(id, name, created_at)| {
            (
                created_at,
                LeaderboardRow {
                    user: id,
                    name,
                    you: id == caller.user,
                    standing: standing(id),
                },
            )
        })
        .collect();
    rows.sort_by(|(a_age, a), (b_age, b)| {
        b.standing
            .score(by)
            .cmp(&a.standing.score(by))
            .then(a_age.cmp(b_age))
            .then(a.user.cmp(&b.user))
    });
    Ok(Json(Leaderboard {
        rows: rows.into_iter().map(|(_, row)| row).collect(),
    }))
}

pub async fn collection(
    State(state): State<AppState>,
    caller: Caller,
    Path(user): Path<i64>,
) -> ApiResult<Json<Collection>> {
    let db = state.db();
    let person: Option<(String, bool)> = db
        .query_row(
            "SELECT name, hidden FROM users WHERE id = ?1",
            [user],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    match person {
        Some((name, hidden)) if !hidden || user == caller.user => Ok(Json(Collection {
            user,
            name,
            cans: cans_of(&db, user)?,
        })),
        _ => Err(ApiError::not_found(
            "That person isn't here, or keeps their board hidden.",
        )),
    }
}
