use axum::{response::{IntoResponse, Response}, http::StatusCode, Json};
use serde::{Serialize, Deserialize};
use serde_json::json;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AppError {
    UnprocessableEntity(String),
    NotFound(String),
    NotCreated(String),
    DbErr(String),
    AlreadyConnected(String),
    LobbyFull(String),
    InternalServerError(String),
    WrongCredentials,
    MissingCredentials,
    TokenCreation,
    InvalidToken,
}


impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_message) = match self {
            AppError::NotFound(s) => (StatusCode::NOT_FOUND, s),
            AppError::NotCreated(s) => (StatusCode::BAD_REQUEST, s),
            AppError::UnprocessableEntity(s) => (StatusCode::UNPROCESSABLE_ENTITY, s),
            AppError::DbErr(s) => (StatusCode::INTERNAL_SERVER_ERROR, s),
            AppError::AlreadyConnected(s) => (StatusCode::BAD_REQUEST, s),
            AppError::LobbyFull(s) => (StatusCode::NOT_MODIFIED, s),//TODO: good code ?
            AppError::InternalServerError(s) => (StatusCode::INTERNAL_SERVER_ERROR, s),
            AppError::WrongCredentials => (StatusCode::UNAUTHORIZED, "Wronge credentials".to_string()),
            AppError::MissingCredentials => (StatusCode::UNAUTHORIZED, "missing credentials".to_string()),
            AppError::TokenCreation => (StatusCode::INTERNAL_SERVER_ERROR, "bad token".to_string()),
            AppError::InvalidToken => (StatusCode::UNAUTHORIZED, "invalid credentials".to_string()), 
        };

        let body = Json(json!({
            "error": error_message,
        }));

        (status, body).into_response()
    }
}