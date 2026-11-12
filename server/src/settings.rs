//! The account's settings, kept the same on every computer. The server stores them as the
//! JSON text they came as and never looks inside, beyond checking that they are an object and
//! not too large. They are never logged.

use axum::{
    Json,
    extract::State,
    http::header,
    response::{IntoResponse, Response},
};
use monster_core::api::{SETTINGS_BYTES, SettingsSaved};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

use crate::{
    AppState,
    error::{ApiError, ApiResult, Body},
    hit, hits,
    me::Caller,
};

/// Saves one account may make in a minute. A computer saves a few times a minute at most,
/// and someone may have several, so this only stops a runaway loop.
pub const SAVES_PER_MINUTE: i64 = 30;

/// The rate-limit bucket for an account's saves.
pub fn saves_bucket(user: i64) -> String {
    format!("settings-put:{user}")
}

/// `monster_core::api::PutSettings`, with `data` kept as the text it came as.
#[derive(Deserialize)]
pub struct PutSettings {
    base: i64,
    data: Box<RawValue>,
}

/// `monster_core::api::SyncedSettings`, with `data` sent back as the text it was stored as.
#[derive(Serialize)]
struct SyncedSettings {
    version: i64,
    updated_at: i64,
    data: Box<RawValue>,
}

pub async fn get(State(state): State<AppState>, caller: Caller) -> ApiResult<Response> {
    let row: Option<(i64, i64, String)> = state
        .db()
        .query_row(
            "SELECT version, updated_at, data FROM settings WHERE user_id = ?1",
            [caller.user],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((version, updated_at, data)) = row else {
        return Err(ApiError::not_found("No settings were saved yet."));
    };
    let data = RawValue::from_string(data).map_err(|_| {
        tracing::error!("stored settings of account {} aren't JSON", caller.user);
        ApiError::server()
    })?;
    let body = serde_json::to_string(&SyncedSettings {
        version,
        updated_at,
        data,
    })
    .map_err(|_| ApiError::server())?;
    Ok(([(header::CONTENT_TYPE, "application/json")], body).into_response())
}

pub async fn put(
    State(state): State<AppState>,
    caller: Caller,
    Body(req): Body<PutSettings>,
) -> ApiResult<Json<SettingsSaved>> {
    let now = state.now();
    let bucket = saves_bucket(caller.user);
    {
        let db = state.db();
        // Old hits of this bucket go now, since a busy account would otherwise pile them up
        // between the sweeps sign-ins do.
        db.execute(
            "DELETE FROM hits WHERE bucket = ?1 AND at <= ?2",
            params![bucket, now - 60],
        )?;
        if hits(&db, &bucket, now, 60)? >= SAVES_PER_MINUTE {
            return Err(ApiError::slow_down(
                "Settings were saved too often in the last minute. Wait a moment.",
            ));
        }
        hit(&db, &bucket, now)?;
    }

    let data = req.data.get();
    if !data.trim_start().starts_with('{') {
        return Err(ApiError::invalid("The settings must be a JSON object."));
    }
    if data.len() > SETTINGS_BYTES {
        return Err(ApiError::invalid(format!(
            "The settings are too large: at most {} KB.",
            SETTINGS_BYTES / 1024
        )));
    }

    let mut db = state.db();
    let tx = db.transaction()?;
    let current: i64 = tx
        .query_row(
            "SELECT version FROM settings WHERE user_id = ?1",
            [caller.user],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0);
    if req.base != current {
        return Err(ApiError::conflict(
            "Another computer saved the settings first. Fetch them and start from those.",
        ));
    }
    let version = current + 1;
    tx.execute(
        "INSERT INTO settings (user_id, version, updated_at, data) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (user_id) DO UPDATE
         SET version = excluded.version, updated_at = excluded.updated_at, data = excluded.data",
        params![caller.user, version, now, data],
    )?;
    tx.commit()?;
    Ok(Json(SettingsSaved {
        version,
        updated_at: now,
    }))
}
