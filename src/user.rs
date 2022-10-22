use std::sync::Arc;

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
    Argon2,
};
use axum::{
    extract::{Extension, Path, Query},
    Json,
};
use rand::Rng;
use serde::Deserialize;
use sqlx::{Executor, PgPool, Postgres, Transaction};
use tracing::{event, Level};
use uuid::Uuid;

use crate::{
    auth::{Auth, AuthAdmin},
    entities::{GameEvents, Lobby, Settings, User, UserRole},
    error::AppError,
    lobby::{get_lobby_players, get_lobby_transaction, send_broadcast_msg, LobbyUserUpdate},
    websockets::EventMessages,
    State,
};

#[derive(Deserialize)]
pub struct CreateUser {
    username: String,
    password: String,
    role: UserRole,
}

#[derive(Deserialize)]
pub struct UpdateUser {
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

pub async fn create_user_endpoint(
    Extension(ref db): Extension<PgPool>,
    Json(payload): Json<CreateUser>,
    _auth: AuthAdmin,
) -> Result<Json<User>, AppError> {
    let mut tx = db
        .begin()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    event!(Level::INFO, "creating user: {}", payload.username);

    let user = create_user(&mut tx, payload).await?;

    tx.commit()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    Ok(Json(user))
}

pub async fn register_endpoint(
    Extension(ref db): Extension<PgPool>,
    Json(payload): Json<CreateUser>,
) -> Result<Json<User>, AppError> {
    if payload.role != UserRole::User {
        return Err(AppError::Unauthorized(
            "can't create user with this role".to_string(),
        ));
    }

    let mut tx = db
        .begin()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    let user = create_user(&mut tx, payload).await?;

    tx.commit()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    Ok(Json(user))
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

pub async fn get_user<'a, E>(id: Uuid, db: E) -> Result<User, AppError>
where
    E: Executor<'a, Database = Postgres>,
{
    Ok(sqlx::query_as!(User,
        // language=PostgreSQL
        r#"select id, username, password, game_id, role as "role: UserRole" from "user" where id = $1"#,
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
    _auth: Auth,
) -> Result<Json<User>, AppError> {
    let user = get_user(id, db).await?;

    Ok(Json(user))
}

pub async fn get_me_endpoint(
    Extension(ref db): Extension<PgPool>,
    auth: Auth,
) -> Result<Json<User>, AppError> {
    let user = get_user(auth.user_id, db).await?;

    Ok(Json(user))
}

pub async fn get_users_endpoint(
    Extension(ref db): Extension<PgPool>,
    _auth: Auth,
) -> Result<Json<Vec<User>>, AppError> {
    let users = sqlx::query_as!(
        User,
        // language=PostgreSQL
        r#"select id, username, password, game_id, role as "role: UserRole" from "user" "#,
    )
    .fetch_all(db)
    .await
    .map_err(|e| AppError::DbErr(e.to_string()))?;

    Ok(Json(users))
}

pub async fn delete_user_endpoint(
    Path(id): Path<Uuid>,
    Extension(ref db): Extension<PgPool>,
    auth: Auth,
) -> Result<(), AppError> {
    if auth.user_id != id && auth.role != UserRole::Admin {
        return Err(AppError::Unauthorized("Can't delete a user".to_string()));
    }

    let user = get_user(id, db).await?;

    if let Some(id) = user.game_id {
        return Err(AppError::UserConnected(id.to_string()));
    }

    event!(Level::DEBUG, "Deleting a user: {}", user.id);

    sqlx::query!(
        // language=PostgreSQL
        r#"delete from "user" where id = $1 "#,
        id
    )
    .execute(db)
    .await
    .map_err(|e| AppError::DbErr(e.to_string()))?;

    event!(Level::INFO, "User deleted: {}", user.id);

    Ok(())
}

pub async fn update_user_endpoint(
    Path(id): Path<Uuid>,
    Extension(ref db): Extension<PgPool>,
    Json(payload): Json<UpdateUser>,
    auth: Auth,
) -> Result<Json<User>, AppError> {
    if auth.user_id != id && auth.role != UserRole::Admin {
        return Err(AppError::Unauthorized("Can't update a user".to_string()));
    }

    let old = get_user(id, db).await?;

    let mut password = old.password;
    if let Some(new_password) = payload.password {
        password = new_password;
    }

    event!(Level::DEBUG, "Updating a user: {}", id);

    let updated = sqlx::query_as!(User,
        // language=PostgreSQL
        r#"update "user" set password = $1 where id = $2 returning id, username, password, game_id, role as "role: UserRole" "#,
        password,
        id
    )
    .fetch_one(db)
    .await
    .map_err(|e| {
        AppError::DbErr(e.to_string())
    })?;

    event!(Level::INFO, "Updated a user: {}", id);

    Ok(Json(updated))
}

pub async fn connect_user_endpoint(
    Path(id): Path<Uuid>,
    Extension(ref db): Extension<PgPool>,
    Extension(state): Extension<Arc<State>>,
    params: Query<ConnectUser>,
    _auth: Auth,
) -> Result<Json<Uuid>, AppError> {
    let mut tx = db
        .begin()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    event!(
        Level::INFO,
        "Connecting user: {} to game: {}",
        id,
        params.game_id
    );

    lock_user_tables(&mut tx).await?;
    lock_lobby_tables(&mut tx).await?;

    event!(Level::DEBUG, "Tables locked");

    let lobby_id = connect_user(id, &mut tx, state, params.game_id).await?;

    event!(Level::INFO, "User: {} connected, committing...", id);

    tx.commit()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    Ok(Json(lobby_id))
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
        EventMessages::NewUserConnected(crate::lobby::LobbyUserUpdate {
            game_id: game_id,
            user: user,
            users_count: users.len(),
            users: users,
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

pub async fn disconnect_user_endpoint(
    Path(id): Path<Uuid>,
    Extension(ref db): Extension<PgPool>,
    Extension(state): Extension<Arc<State>>,
    _auth: Auth,
) -> Result<(), AppError> {
    let mut tx = db
        .begin()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    let user = get_user(id, &mut tx).await?;

    if let None = user.game_id {
        return Err(AppError::NotConnected);
    }

    let game_id = user.game_id.unwrap();

    let users = get_lobby_players(game_id, &mut tx).await?;

    event!(Level::INFO, "Disconnecting user: {}", id);

    sqlx::query!(
        // language=PostgreSQL
        r#"update "user" set game_id = NULL where id = $1"#,
        id
    )
    .execute(db)
    .await
    .map_err(|e| AppError::DbErr(e.to_string()))?;

    event!(Level::DEBUG, "Disconnected user: {}, sending msg", id);

    send_broadcast_msg(
        state,
        id,
        EventMessages::UserDisconnected(LobbyUserUpdate {
            game_id: game_id,
            user,
            users_count: users.len(),
            users,
        }),
    )?;

    event!(Level::DEBUG, "Disconnected user: {}, committing...", id);

    tx.commit()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    Ok(())
}

pub async fn quick_connect_endpoint(
    Extension(ref db): Extension<PgPool>,
    params: Query<QuickConnect>,
    Extension(state): Extension<Arc<State>>,
    auth: Auth,
) -> Result<Json<Uuid>, AppError> {
    let mut tx = db
        .begin()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    event!(
        Level::INFO,
        "Quick connecting code: {}, user: {}",
        params.connect_code,
        auth.user_id
    );

    lock_user_tables(&mut tx).await?;
    lock_lobby_tables(&mut tx).await?;

    event!(Level::DEBUG, "Tables locked");

    let user = get_user(auth.user_id, &mut tx).await?;

    let lobby_id = quick_connect(&mut tx, &params.connect_code, state, user).await?;

    event!(
        Level::INFO,
        "User: {}, quick connected, committing...",
        auth.user_id
    );

    tx.commit()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    Ok(Json(lobby_id))
}

pub async fn quick_connect_endpoint_no_user(
    Extension(ref db): Extension<PgPool>,
    params: Query<QuickConnect>,
    Extension(state): Extension<Arc<State>>,
) -> Result<Json<Uuid>, AppError> {
    let mut tx = db
        .begin()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    let count = sqlx::query_scalar!(
        // language=PostgreSQL
        r#"select count(*) from "lobby" where connect_code = $1"#,
        params.connect_code
    )
    .fetch_one(&mut tx)
    .await
    .map_err(|e| AppError::InternalServerError(e.to_string()))?;

    match count {
        Some(c) => {
            if c != 1 {
                return Err(AppError::NotFound(
                    "No lobby with this connect code".to_string(),
                ));
            }
        }
        None => {
            //TODO: WTF XD idk to do
            tracing::info!("NONE WHILE looking for count of lobbies with this connect code")
        }
    }

    event!(
        Level::INFO,
        "Quick connecting code: {}, with no user",
        params.connect_code,
    );

    lock_user_tables(&mut tx).await?;
    lock_lobby_tables(&mut tx).await?;

    event!(Level::DEBUG, "Tables locked");

    let mut user = CreateUser {
        username: generate_username(),
        password: generate_password(),
        role: UserRole::Temp,
    };

    event!(Level::TRACE, "Creating temp user");

    let mut creation_result = create_user(&mut tx, user).await;

    while let Err(AppError::AlreadyExists(_)) = creation_result {
        user = CreateUser {
            username: generate_username(),
            password: generate_password(),
            role: UserRole::Temp,
        };

        creation_result = create_user(&mut tx, user).await;
    }

    let temp_usr = creation_result?;

    event!(
        Level::DEBUG,
        "Temp user created: {}, username: {}",
        temp_usr.id,
        temp_usr.username
    );

    let lobby_id = quick_connect(&mut tx, &params.connect_code, state, temp_usr).await?;

    tx.commit()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    Ok(Json(lobby_id))
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

fn generate_username() -> String {
    let mut rng = rand::thread_rng();
    let number: u64 = rng.gen_range(0..9999999999999);
    format!("temp-{:013}", number)
}

fn generate_password() -> String {
    let mut rng = rand::thread_rng();
    let mut s = Vec::new();
    for _ in 1..10 {
        let letter: char = rng.gen_range(b'A'..b'Z') as char;
        s.push(letter);
    }
    let number: u64 = rng.gen_range(0..9999999999999);
    format!("{}{:013}", String::from_iter(s), number)
}
