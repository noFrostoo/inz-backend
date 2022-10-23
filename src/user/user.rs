use std::sync::Arc;

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
    Argon2,
};

use rand::Rng;
use serde::{Deserialize, Serialize};
use sqlx::{Executor, Postgres, Transaction};
use tracing::{event, Level};
use uuid::Uuid;

use crate::{
    entities::{GameEvents, Lobby, Settings, User, UserRole},
    error::AppError,
    lobby::lobby::{get_lobby_transaction, send_broadcast_msg, LobbyUserUpdate},
    websockets::EventMessages,
    State,
};

#[derive(Deserialize, Serialize)]
pub struct CreateUser {
    pub username: String,
    pub password: String,
    pub role: UserRole,
}

#[derive(Deserialize, Serialize)]
pub struct UpdateUser {
    pub password: String,
}

#[derive(Deserialize, Serialize)]
pub struct ConnectUser {
    pub game_id: Uuid,
}

#[derive(Deserialize, Serialize)]
pub struct QuickConnect {
    pub connect_code: String,
}

pub async fn create_user(
    tx: &mut Transaction<'_, Postgres>,
    user_data: CreateUser,
) -> Result<User, AppError> {
    let argon2 = Argon2::default();
    let salt = SaltString::generate(&mut OsRng);

    let count = sqlx::query_scalar!(
        // language=PostgreSQL
        r#"select count(*) from "user" where username = $1"#,
        user_data.username
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| AppError::DbErr(e.to_string()))?;

    event!(
        Level::TRACE,
        "SELECT for user count with username: {}",
        user_data.username
    );

    if count.is_none() || (count.is_some() && count.unwrap() != 0) {
        event!(
            Level::ERROR,
            "user with username: {} exists",
            user_data.username
        );

        return Err(AppError::AlreadyExists(
            "User exists with this username".to_string(),
        ));
    }

    if user_data.username.is_empty() || user_data.password.is_empty() {
        return Err(AppError::EmptyData(format!(
            "user_data: {}, {}",
            user_data.username, user_data.password
        )));
    }

    let user = sqlx::query_as!(User,
        // language=PostgreSQL
        r#"insert into "user" (username,password,role) values ($1, $2, $3) returning id, username, password, game_id, role as "role: UserRole" "#,
        user_data.username,
        argon2.hash_password(user_data.password.as_bytes(), &salt).map_err(|e| {
            AppError::InternalServerError(e.to_string()) //TODO: refactor error
        })?.to_string(),
        user_data.role as _
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        AppError::NotCreated(e.to_string())
    })?;

    event!(Level::INFO, "created user: {}", user.username);

    Ok(user)
}

pub async fn update_user_password(
    id: Uuid,
    tx: &mut Transaction<'_, Postgres>,
    password: String,
) -> Result<User, AppError> {
    let argon2 = Argon2::default();
    let salt = SaltString::generate(&mut OsRng);
    let updated = sqlx::query_as!(User,
        // language=PostgreSQL
        r#"update "user" set password = $1 where id = $2 returning id, username, password, game_id, role as "role: UserRole" "#,
        argon2.hash_password(password.as_bytes(), &salt).map_err(|e| {
            AppError::InternalServerError(e.to_string()) //TODO: refactor error
        })?.to_string(),
        id
    )
    .fetch_one(tx)
    .await
    .map_err(|e| {
        AppError::DbErr(e.to_string())
    })?;
    Ok(updated)
}

pub async fn get_user<'a, E>(id: Uuid, db: E) -> Result<User, AppError>
where
    E: Executor<'a, Database = Postgres>,
{
    sqlx::query_as!(User,
        // language=PostgreSQL
        r#"select id, username, password, game_id, role as "role: UserRole" from "user" where id = $1"#,
        id
    )
    .fetch_one(db)
    .await
    .map_err(|e| {
        AppError::NotFound(e.to_string())
    })
}

pub async fn connect_user(
    id: Uuid,
    tx: &mut Transaction<'_, Postgres>,
    state: Arc<State>,
    game_id: Uuid,
) -> Result<Uuid, AppError> {
    let user = get_user(id, &mut *tx).await?;

    if let Some(id) = user.game_id {
        return Err(AppError::UserConnected(id.to_string()));
    }

    let lobby = get_lobby_transaction(game_id, &mut *tx).await?;

    let count = sqlx::query_scalar!(
        // language=PostgreSQL
        r#"select count(*) from "user" where game_id = $1"#,
        game_id
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| AppError::InternalServerError(e.to_string()))?;

    if count.is_some() && count.unwrap() >= lobby.max_players as i64 {
        return Err(AppError::LobbyFull("Looby full".to_string()));
    }

    event!(
        Level::DEBUG,
        "Lobby: {} not full, ready to connect user: {}",
        game_id,
        id
    );

    sqlx::query!(
        // language=PostgreSQL
        r#"update "user" set game_id = $1 where id = $2"#,
        game_id,
        id
    )
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::DbErr(e.to_string()))?;

    let users = sqlx::query_as!(User,
        // language=PostgreSQL
        r#"select id, username, password, game_id, role as "role: UserRole" from "user" where game_id = $1 "#,
        game_id
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| {
        AppError::DbErr(e.to_string())
    })?;

    event!(
        Level::DEBUG,
        "User: {} connected, sending a broadcast msg",
        id
    );

    send_broadcast_msg(
        state,
        game_id,
        EventMessages::NewUserConnected(LobbyUserUpdate {
            game_id,
            user,
            users_count: users.len(),
            users,
        }),
    )?;

    Ok(game_id)
}

pub async fn lock_user_tables<'a, E>(db: E) -> Result<(), AppError>
where
    E: Executor<'a, Database = Postgres>,
{
    event!(Level::DEBUG, "Locking users table");

    sqlx::query!(r#"lock table "users" in ACCESS EXCLUSIVE MODE "#)
        .execute(db)
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;
    Ok(())
}

pub async fn lock_lobby_tables<'a, E>(db: E) -> Result<(), AppError>
where
    E: Executor<'a, Database = Postgres>,
{
    event!(Level::DEBUG, "Locking lobby table");

    sqlx::query!(r#"lock table "lobby" in ACCESS EXCLUSIVE MODE "#)
        .execute(db)
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;
    Ok(())
}

pub async fn quick_connect(
    tx: &mut Transaction<'_, Postgres>,
    connect_code: &String,
    state: Arc<State>,
    user: User,
) -> Result<Uuid, AppError> {
    let lobby = sqlx::query_as!(Lobby,
        // language=PostgreSQL
        r#"select id, name, password, public, connect_code, code_use_times, max_players, started, owner_id, settings as "settings: sqlx::types::Json<Settings>", events as "events: sqlx::types::Json<GameEvents>" from "lobby" where connect_code = $1"#,
        connect_code
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        AppError::NotFound(e.to_string())
    })?;

    if lobby.code_use_times <= 0 {
        return Err(AppError::LobbyFull(
            "Can't connect to lobby with this code".to_string(),
        ));
    }

    let game_id = connect_user(user.id, &mut *tx, state, lobby.id).await?;

    Ok(game_id)
}

pub fn generate_username() -> String {
    let mut rng = rand::thread_rng();
    let number: u64 = rng.gen_range(0..9999999999999);
    format!("temp-{:013}", number)
}

pub fn generate_password() -> String {
    let mut rng = rand::thread_rng();
    let mut s = Vec::new();
    for _ in 1..10 {
        let letter: char = rng.gen_range(b'A'..=b'Z') as char;
        s.push(letter);
    }
    let number: u64 = rng.gen_range(0..9999999999999);
    format!("{}{:013}", String::from_iter(s), number)
}
