use argon2::{
    password_hash::{PasswordHash, PasswordVerifier},
    Argon2,
};

use axum::{
    async_trait,
    extract::{FromRequest, RequestParts, TypedHeader},
    Extension, Json,
};
use headers::{authorization::Bearer, Authorization};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    entities::{User, UserRole},
    error::AppError,
    KEYS,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct Auth {
    pub username: String,
    pub user_id: Uuid,
    pub role: UserRole,
    pub exp: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct WebSocketAuthInner {
    pub username: String,
    pub user_id: Uuid,
    pub role: UserRole,
    pub exp: usize,
}

#[derive(Debug, Serialize, Clone, Deserialize)]
pub struct WebSocketAuth {
    pub username: String,
    pub user_id: Uuid,
    pub role: UserRole,
    pub exp: usize,
    pub token: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthAdmin {
    pub username: String,
    pub user_id: Uuid,
    pub role: UserRole,
    pub exp: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthUser {
    pub username: String,
    pub user_id: Uuid,
    pub role: UserRole,
    pub exp: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthTemp {
    pub username: String,
    pub user_id: Uuid,
    pub role: UserRole,
    pub exp: usize,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AuthBody {
    pub access_token: String,
    pub token_type: String,
}

impl AuthBody {
    pub fn new(access_token: String) -> Self {
        Self {
            access_token,
            token_type: "Bearer".to_string(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthPayload {
    pub username: String,
    pub password: String,
}

#[derive(Clone)]
pub struct Keys {
    encoding: EncodingKey,
    decoding: DecodingKey,
}

impl Keys {
    pub fn new(secret: &[u8]) -> Self {
        Self {
            encoding: EncodingKey::from_secret(secret),
            decoding: DecodingKey::from_secret(secret),
        }
    }
}

pub async fn authorize_endpoint(
    Extension(ref db): Extension<PgPool>,
    Json(payload): Json<AuthPayload>,
) -> Result<Json<AuthBody>, AppError> {
    let token = authorize(payload, db).await?;

    Ok(Json(AuthBody::new(token)))
}

pub async fn authorize(payload: AuthPayload, db: &PgPool) -> Result<String, AppError> {
    if payload.username.is_empty() || payload.password.is_empty() {
        return Err(AppError::MissingCredentials);
    }

    let user = sqlx::query_as!(
        User,
        r#"select id, username, password, game_id, role as "role: UserRole" 
        from "user" where username = $1 "#,
        payload.username
    )
    .fetch_one(db)
    .await
    .map_err(|e| AppError::WrongCredentials(e.to_string()))?;

    let parsed_hash =
        PasswordHash::new(&user.password).map_err(|e| AppError::WrongCredentials(e.to_string()))?;

    Argon2::default()
        .verify_password(payload.password.as_bytes(), &parsed_hash)
        .map_err(|e| AppError::WrongCredentials(e.to_string()))?;

    let claims = Auth {
        username: user.username,
        user_id: user.id,
        // Mandatory expiry time as UTC timestamp
        exp: 2000000000, // May 2033
        role: user.role,
    };

    let token =
        encode(&Header::default(), &claims, &KEYS.encoding).map_err(|_| AppError::TokenCreation)?;

    Ok(token)
}

#[async_trait]
impl<B> FromRequest<B> for Auth
where
    B: Send,
{
    type Rejection = AppError;

    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        // Extract the token from the authorization header
        let TypedHeader(Authorization(bearer)) =
            TypedHeader::<Authorization<Bearer>>::from_request(req)
                .await
                .map_err(|_| AppError::InvalidToken)?;

        let token_data = decode::<Auth>(bearer.token(), &KEYS.decoding, &Validation::default())
            .map_err(|_| AppError::InvalidToken)?;

        Ok(token_data.claims)
    }
}

#[async_trait]
impl<B> FromRequest<B> for WebSocketAuth
where
    B: Send,
{
    type Rejection = AppError;

    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        let header = match req.headers().get("Sec-WebSocket-Protocol") {
            Some(h) => h,
            None => return Err(AppError::InvalidToken),
        };

        let string_val = header.to_str().map_err(|_| AppError::InvalidToken)?;
        let mut splitted = string_val.split(",");

        if splitted.next() != Some("access_token") {
            return Err(AppError::InvalidToken);
        }

        let token = match splitted.next() {
            Some(t) => t.trim(),
            None => return Err(AppError::InvalidToken),
        };

        tracing::info!("token: {}", token);

        let token_data =
            decode::<WebSocketAuthInner>(token, &KEYS.decoding, &Validation::default())
                .map_err(|_| AppError::InvalidToken)?;

        tracing::info!("Websocet connected");

        let auth = WebSocketAuth {
            token: token.to_string(),
            username: token_data.claims.username,
            user_id: token_data.claims.user_id,
            role: token_data.claims.role,
            exp: token_data.claims.exp,
        };

        Ok(auth)
    }
}

#[async_trait]
impl<B> FromRequest<B> for AuthAdmin
where
    B: Send,
{
    type Rejection = AppError;

    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        // Extract the token from the authorization header
        let TypedHeader(Authorization(bearer)) =
            TypedHeader::<Authorization<Bearer>>::from_request(req)
                .await
                .map_err(|_| AppError::InvalidToken)?;

        let token_data = decode::<Auth>(bearer.token(), &KEYS.decoding, &Validation::default())
            .map_err(|_| AppError::InvalidToken)?;

        if token_data.claims.role != UserRole::Admin {
            //TODO: pass correct string
            return Err(AppError::Unauthorized("".to_string()));
        }

        let auth = AuthAdmin {
            username: token_data.claims.username,
            user_id: token_data.claims.user_id,
            exp: token_data.claims.exp,
            role: token_data.claims.role,
        };

        Ok(auth)
    }
}

#[async_trait]
impl<B> FromRequest<B> for AuthTemp
where
    B: Send,
{
    type Rejection = AppError;

    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        // Extract the token from the authorization header
        let TypedHeader(Authorization(bearer)) =
            TypedHeader::<Authorization<Bearer>>::from_request(req)
                .await
                .map_err(|_| AppError::InvalidToken)?;

        let token_data = decode::<Auth>(bearer.token(), &KEYS.decoding, &Validation::default())
            .map_err(|_| AppError::InvalidToken)?;

        let auth = AuthTemp {
            username: token_data.claims.username,
            user_id: token_data.claims.user_id,
            exp: token_data.claims.exp,
            role: token_data.claims.role,
        };

        Ok(auth)
    }
}

#[async_trait]
impl<B> FromRequest<B> for AuthUser
where
    B: Send,
{
    type Rejection = AppError;

    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        // Extract the token from the authorization header
        let TypedHeader(Authorization(bearer)) =
            TypedHeader::<Authorization<Bearer>>::from_request(req)
                .await
                .map_err(|_| AppError::InvalidToken)?;

        let token_data = decode::<Auth>(bearer.token(), &KEYS.decoding, &Validation::default())
            .map_err(|_| AppError::InvalidToken)?;

        if token_data.claims.role == UserRole::Temp {
            //TODO: pass correct string
            return Err(AppError::Unauthorized("".to_string()));
        }

        let auth = AuthUser {
            username: token_data.claims.username,
            user_id: token_data.claims.user_id,
            exp: token_data.claims.exp,
            role: token_data.claims.role,
        };

        Ok(auth)
    }
}
