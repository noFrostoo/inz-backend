use std::fmt::Display;

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
    AlreadyExists(String)
}

impl AppError {
    fn get_error_status_str(&self) -> (StatusCode, String) {
        match self {
            AppError::NotFound(s) => (StatusCode::NOT_FOUND, s.clone()),
            AppError::NotCreated(s) => (StatusCode::BAD_REQUEST, s.clone()),
            AppError::UnprocessableEntity(s) => (StatusCode::UNPROCESSABLE_ENTITY, s.clone()),
            AppError::DbErr(s) => (StatusCode::INTERNAL_SERVER_ERROR, s.clone()),
            AppError::AlreadyConnected(s) => (StatusCode::BAD_REQUEST, s.clone()),
            AppError::LobbyFull(s) => (StatusCode::NOT_MODIFIED, s.clone()),//TODO: good code ?
            AppError::InternalServerError(s) => (StatusCode::INTERNAL_SERVER_ERROR, s.clone()),
            AppError::WrongCredentials => (StatusCode::UNAUTHORIZED, "Wronge credentials".to_string()),
            AppError::MissingCredentials => (StatusCode::UNAUTHORIZED, "missing credentials".to_string()),
            AppError::TokenCreation => (StatusCode::INTERNAL_SERVER_ERROR, "bad token".to_string()),
            AppError::InvalidToken => (StatusCode::UNAUTHORIZED, "invalid credentials".to_string()),
            AppError::AlreadyExists(_) => todo!(), 
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_message) = self.get_error_status_str();

        let body = Json(json!({
            "error": error_message,
        }));

        (status, body).into_response()
    }
}

impl Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (_, error_message) = self.get_error_status_str();
        write!(f, "{}", error_message)
    }
}