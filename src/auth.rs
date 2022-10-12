use argon2::{
    password_hash::{
        PasswordHash, PasswordVerifier
    },
    Argon2
};

use axum::{Json, Extension, async_trait, extract::{RequestParts, FromRequest, TypedHeader}};
use headers::{Authorization, authorization::Bearer};
use jsonwebtoken::{EncodingKey, DecodingKey, Header, encode, decode, Validation};
use serde::{Serialize, Deserialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{error::AppError, entities::{User, UserRole}, KEYS};

#[derive(Debug, Serialize, Deserialize)]
pub struct Auth {
    pub username: String,
    pub user_id: Uuid,
    pub role: UserRole,
    exp: usize,
}
 
#[derive(Debug, Serialize, Deserialize)]
pub struct AuthAdmin {
    pub username: String,
    pub user_id: Uuid,
    exp: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthUser {
    pub username: String,
    pub user_id: Uuid,
    exp: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthTemp {
    pub username: String,
    pub user_id: Uuid,
    exp: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthGameAdmin {
    pub username: String,
    pub user_id: Uuid,
    exp: usize,
}

#[derive(Debug, Serialize)]
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


#[derive(Debug, Deserialize)]
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

pub async fn authorize(Extension(ref db): Extension<PgPool>, Json(payload): Json<AuthPayload>) -> Result<Json<AuthBody>, AppError> {
    if payload.username.is_empty() || payload.password.is_empty() {
        return Err(AppError::MissingCredentials);
    }

    let user = sqlx::query_as!(User,
        r#"select id, username, password, game_id, role as "role: UserRole" from "user" where username = $1 "#,
        payload.username
    )
    .fetch_one(db)
    .await
    .map_err(|_| {
        AppError::WrongCredentials
    })?;

    let parsed_hash = PasswordHash::new(&user.password).map_err(|e| {
        eprint!("{}", e.to_string()); 
        AppError::WrongCredentials
    })?;

    Argon2::default().verify_password(payload.password.as_bytes(), &parsed_hash).map_err(|_|{
        AppError::WrongCredentials
    })?;

    let claims = Auth {
        username: user.username,
        user_id: user.id,
        // Mandatory expiry time as UTC timestamp
        exp: 2000000000,
        role: user.role, // May 2033
    };

    let token = encode(&Header::default(), &claims, &KEYS.encoding)
        .map_err(|_| AppError::TokenCreation)?;

    Ok(Json(AuthBody::new(token)))
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

        let auth = AuthAdmin{ username: token_data.claims.username, user_id: token_data.claims.user_id, exp: token_data.claims.exp };

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

        let auth = AuthTemp{ username: token_data.claims.username, user_id: token_data.claims.user_id, exp: token_data.claims.exp };

        Ok(auth)
    }
}


#[async_trait]
impl<B> FromRequest<B> for AuthGameAdmin
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

        let auth = AuthGameAdmin{ username: token_data.claims.username, user_id: token_data.claims.user_id, exp: token_data.claims.exp };

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

        let auth = AuthUser{ username: token_data.claims.username, user_id: token_data.claims.user_id, exp: token_data.claims.exp };

        Ok(auth)
    }
}