use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Path, Query},
};
use serde::{Deserialize};
use sqlx::{PgPool};
use tracing::{event, Level};
use uuid::Uuid;
use argon2::{
    password_hash::{
        rand_core::OsRng,
        PasswordHasher, SaltString
    },
    Argon2
};

use crate::{entities::User, error::AppError, State, auth::Auth, websocets::EventMessages};


pub async fn create_user(
    Extension(ref db): Extension<PgPool>,
    Json(payload): Json<CreateUser>,
) -> Result<Json<User>, AppError> {
    
    let argon2 = Argon2::default();
    let salt = SaltString::generate(&mut OsRng);

    let user = sqlx::query_as!(User,
        // language=PostgreSQL
        r#"insert into "user" (username,password,temp) values ($1, $2, $3) returning id, username, password, game_id, temp"#,
        payload.username,
        argon2.hash_password(payload.password.as_bytes(), &salt).map_err(|e| {
            AppError::InternalServerError(e.to_string()) //TODO: refactor error
        })?.to_string(),
        false
    )
    .fetch_one(db)
    .await
    .map_err(|e| {
        AppError::NotCreated(e.to_string())
    })?;

    Ok(Json(user))
}


pub async fn get_user(id: Uuid, db: &PgPool) -> Result<User, AppError> {
    Ok(sqlx::query_as!(User,
        // language=PostgreSQL
        r#"select id, username, password, game_id, temp from "user" where id = $1"#,
        id
    )
    .fetch_one(db)
    .await
    .map_err(|e| {
        AppError::NotFound(e.to_string())
    })?)
}

pub async fn get_user_endpoint(
    Path(id): Path<Uuid>,
    Extension(ref db): Extension<PgPool>,
    _auth: Auth
) -> Result<Json<User>, AppError> {
    
    let user = get_user(id, db).await?;

    Ok(Json(user))
}

pub async fn get_me(
    Extension(ref db): Extension<PgPool>,
    auth: Auth
) -> Result<Json<User>, AppError> {

    let user = get_user(auth.user_id, db).await?;

    Ok(Json(user))
}

pub async fn get_users(
    Extension(ref db): Extension<PgPool>,
    _auth: Auth
) -> Result<Json<Vec<User>>, AppError> {
    
    let users = sqlx::query_as!(User,
        // language=PostgreSQL
        r#"select id, username, password, game_id, temp from "user" "#,
    )
    .fetch_all(db)
    .await
    .map_err(|e| {
        AppError::DbErr(e.to_string())
    })?;

    Ok(Json(users))
}

pub async fn delete_user(
    Path(id): Path<Uuid>,
    Extension(ref db): Extension<PgPool>,
    _auth: Auth
) -> Result<(), AppError> {
    
    let res = sqlx::query!(
        // language=PostgreSQL
        r#"delete from "user" where id = $1 "#,
        id
    )
    .execute(db)
    .await;

    match res {
        Ok(_) => Ok(()),
        Err(e) => Err(AppError::NotFound(e.to_string())),
    }
}

pub async fn update_user(
    Path(id): Path<Uuid>,
    Extension(ref db): Extension<PgPool>,
    Json(payload): Json<UpdateUser>,
    _auth: Auth
) -> Result<Json<User>, AppError> {
    
    let old = sqlx::query_as!(User,
        // language=PostgreSQL
        r#"select id, username, password, game_id, temp from "user" where id = $1 "#,
        id
    )
    .fetch_one(db)
    .await
    .map_err(|e| {
        AppError::NotFound(e.to_string())
    })?;

    let mut username = old.username;
    if let Some(new_username) = payload.username {
        username = new_username;
    }

    let mut password = old.password;
    if let Some(new_password) = payload.password {
        password = new_password;
    }

    let updated = sqlx::query_as!(User,
        // language=PostgreSQL
        r#"update "user" set username = $1, password = $2 where id = $3 returning id, username, password, game_id, temp"#,
        username,
        password,
        id
    )
    .fetch_one(db)
    .await
    .map_err(|e| {
        AppError::DbErr(e.to_string())
    })?;

    Ok(Json(updated))
}

pub async fn connect_user(
    Path(id): Path<Uuid>,
    Extension(ref db): Extension<PgPool>,
    Extension(state): Extension<Arc<State>>,
    params: Query<ConnectUser>,
    _auth: Auth
) -> Result<(), AppError> {
    let user = sqlx::query_as!(User,
        // language=PostgreSQL
        r#"select id, username, password, game_id, temp from "user" where id = $1 "#,
        id
    )
    .fetch_one(db)
    .await
    .map_err(|e| {
        AppError::NotFound(e.to_string())
    })?;

    if user.game_id.is_some() {
        return Err(AppError::AlreadyConnected("Connected to other game".to_string()))
    }

    let max_players = sqlx::query_scalar!(
        // language=PostgreSQL
        r#"select max_players from "lobby" where id = $1"#,
        id
    )
    .fetch_one(db)
    .await
    .map_err(|e| {
        AppError::NotFound(e.to_string())
    })?;

    let count = sqlx::query_scalar!(
        // language=PostgreSQL
        r#"select count(*) from "user" where game_id = $1"#,
        id
    )
    .fetch_one(db)
    .await
    .map_err(|e| {
        AppError::NotFound(e.to_string())
    })?;

    if max_players == 0 {
        return Err(AppError::NotFound("No lobby with given id".to_string()));
    }

    if count.is_some() && count.unwrap() >= max_players as i64 {
        return Err(AppError::LobbyFull("Looby full".to_string()));
    }

    sqlx::query!(
        // language=PostgreSQL
        r#"update "user" set game_id = $1 where id = $2"#,
        params.game_id,
        id
    )
    .execute(db)
    .await
    .map_err(|e| {
        AppError::DbErr(e.to_string())
    })?;

    match state.lobbies.read() {
        Ok(ctx) => {
            match ctx.get(&params.game_id) {
                Some(lobby_state) => {
                    lobby_state.sender.send(EventMessages::NewUserConnected).map_err(|e| {AppError::InternalServerError(e.to_string())})?;
                },
                None => {
                    return Err(AppError::InternalServerError("Looby not found".to_string()));
                },
            }
        },
        Err(e) => {
            return Err(AppError::InternalServerError(e.to_string()));
        },
    }

    Ok(())
}

pub async fn disconnect_user(
    Path(id): Path<Uuid>,
    Extension(ref db): Extension<PgPool>,
    _auth: Auth
) -> Result<(), AppError> {
    sqlx::query!(
        // language=PostgreSQL
        r#"update "user" set game_id = NULL where id = $1"#,
        id
    )
    .execute(db)
    .await
    .map_err(|e| {
        AppError::DbErr(e.to_string())
    })?;

    Ok(())
}

pub async fn quick_connect(
    Extension(ref db): Extension<PgPool>,
    params: Query<QuickConnect>,
) -> Result<Json<Uuid>, AppError> {
    /*
    1. 
    
    */
    Ok(Json(Uuid::new_v4()))
}

// the input to our `create_user` handler
#[derive(Deserialize)]
pub struct CreateUser {
    username: String,
    password: String,
}


#[derive(Deserialize)]
pub struct UpdateUser {
    username: Option<String>,
    password: Option<String>,
}

#[derive(Deserialize)]
pub struct ConnectUser {
    game_id: Uuid,
}

#[derive(Deserialize)]
pub struct QuickConnect {
    connect_code: String,
}