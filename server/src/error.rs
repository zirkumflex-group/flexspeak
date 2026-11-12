//! Failed calls, answered as a [`Problem`], and extractors whose rejections are one too.

use axum::{
    Json,
    extract::{FromRequest, FromRequestParts, Request, rejection::JsonRejection},
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Response},
};
use monster_core::api::{Problem, problem};
use serde::de::DeserializeOwned;

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub problem: Problem,
}

impl ApiError {
    pub fn new(status: StatusCode, error: &str, message: impl Into<String>) -> Self {
        Self {
            status,
            problem: Problem {
                error: error.into(),
                message: message.into(),
                tries_left: None,
            },
        }
    }

    pub fn signed_out() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            problem::SIGNED_OUT,
            "You're signed out. Sign in again.",
        )
    }

    pub fn wrong_code(tries_left: u32) -> Self {
        let mut err = Self::new(
            StatusCode::UNAUTHORIZED,
            problem::WRONG_CODE,
            "That code doesn't match.",
        );
        err.problem.tries_left = Some(tries_left);
        err
    }

    pub fn expired(message: impl Into<String>) -> Self {
        Self::new(StatusCode::GONE, problem::EXPIRED, message)
    }

    pub fn slow_down(message: impl Into<String>) -> Self {
        Self::new(StatusCode::TOO_MANY_REQUESTS, problem::SLOW_DOWN, message)
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, problem::INVALID, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, problem::NOT_FOUND, message)
    }

    pub fn taken(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, problem::TAKEN, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, problem::CONFLICT, message)
    }

    pub fn server() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            problem::SERVER,
            "Something went wrong on the server. Try again in a moment.",
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.problem)).into_response()
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(err: rusqlite::Error) -> Self {
        tracing::error!("database: {err}");
        Self::server()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

/// A JSON body; a body that doesn't parse is answered with an `invalid` problem.
pub struct Body<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for Body<T> {
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, ApiError> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(Body(value)),
            Err(JsonRejection::MissingJsonContentType(_)) => Err(ApiError::invalid(
                "Send the body as JSON, with Content-Type: application/json.",
            )),
            Err(err) => Err(ApiError::invalid(err.body_text())),
        }
    }
}

/// A path parameter; one that doesn't parse is answered with a `not_found` problem.
pub struct Path<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned + Send> FromRequestParts<S> for Path<T> {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
        axum::extract::Path::<T>::from_request_parts(parts, state)
            .await
            .map(|axum::extract::Path(value)| Path(value))
            .map_err(|_| ApiError::not_found("There's nothing here."))
    }
}

/// A query string; one that doesn't parse is answered with an `invalid` problem.
pub struct Query<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequestParts<S> for Query<T> {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
        axum::extract::Query::<T>::from_request_parts(parts, state)
            .await
            .map(|axum::extract::Query(value)| Query(value))
            .map_err(|err| ApiError::invalid(err.body_text()))
    }
}
