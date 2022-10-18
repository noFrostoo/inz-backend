use std::sync::Arc;

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
    Argon2,
};
use axum::{
    extract::{Extension, Path, Query},
    Json,
};
use futures::TryFutureExt;
use rand::Rng;
use serde::Deserialize;
use sqlx::{Executor, PgPool, Postgres, Transaction, TransactionManager};
use tracing::{event, Level};
use uuid::Uuid;

use crate::{
    auth::{Auth, AuthAdmin},
    entities::{GameEvents, Lobby, Settings, User, UserRole},
    error::AppError,
    websocets::EventMessages,
    State,
};

// the input to our `create_user` handler
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
    let user = create_user(db, payload).await?;

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

    let user = create_user(db, payload).await?;

    Ok(Json(user))
}

pub async fn create_user(db: &PgPool, user_data: CreateUser) -> Result<User, AppError> {
    let argon2 = Argon2::default();
    let salt = SaltString::generate(&mut OsRng);

    let mut tx = db
        .begin()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    sqlx::query!(r#"lock table "users" in Row Exclusive MODE "#)
        .execute(&mut tx)
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    let count = sqlx::query_scalar!(
        // language=PostgreSQL
        r#"select count(*) from "user" where username = $1"#,
        user_data.username
    )
    .fetch_one(&mut tx)
    .await
    .map_err(|e| AppError::DbErr(e.to_string()))?;

    if count.is_none() || (count.is_some() && count.unwrap() != 0) {
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
    .fetch_one(&mut tx)
    .await
    .map_err(|e| {
        AppError::NotCreated(e.to_string())
    })?;

    tx.commit()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

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

    Ok(Json(updated))
}

pub async fn connect_user_endpoint(
    Path(id): Path<Uuid>,
    Extension(ref db): Extension<PgPool>,
    Extension(state): Extension<Arc<State>>,
    params: Query<ConnectUser>,
    _auth: Auth,
) -> Result<Json<Uuid>, AppError> {
    let lobby_id = connect_user(id, db, state, params.game_id).await?;

    Ok(Json(lobby_id))
}

pub async fn connect_user(
    id: Uuid,
    db: &PgPool,
    state: Arc<State>,
    game_id: Uuid,
) -> Result<Uuid, AppError> {
    let mut tx = db
        .begin()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    let user = get_user(id, &mut tx).await?;

    if user.game_id.is_some() {
        return Err(AppError::AlreadyConnected(
            "Connected to other game".to_string(),
        ));
    }

    sqlx::query!(r#"lock table "users" in ACCESS EXCLUSIVE MODE "#)
        .execute(&mut tx)
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;
    sqlx::query!(r#"lock table "lobby" in ACCESS EXCLUSIVE MODE "#)
        .execute(&mut tx)
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    let max_players = sqlx::query_scalar!(
        // language=PostgreSQL
        r#"select max_players from "lobby" where id = $1"#,
        game_id
    )
    .fetch_one(&mut tx)
    .await
    .map_err(|e| AppError::NotFound(e.to_string()))?;

    let count = sqlx::query_scalar!(
        // language=PostgreSQL
        r#"select count(*) from "user" where game_id = $1"#,
        game_id
    )
    .fetch_one(&mut tx)
    .await
    .map_err(|e| AppError::InternalServerError(e.to_string()))?;

    if max_players == 0 {
        return Err(AppError::NotFound("No lobby with given id".to_string()));
    }

    if count.is_some() && count.unwrap() >= max_players as i64 {
        return Err(AppError::LobbyFull("Looby full".to_string()));
    }

    sqlx::query!(
        // language=PostgreSQL
        r#"update "user" set game_id = $1 where id = $2"#,
        game_id,
        id
    )
    .execute(&mut tx)
    .await
    .map_err(|e| AppError::DbErr(e.to_string()))?;

    let users = sqlx::query_as!(User,
        // language=PostgreSQL
        r#"select id, username, password, game_id, role as "role: UserRole" from "user" where game_id = $1 "#,
        game_id
    )
    .fetch_all(&mut tx)
    .await
    .map_err(|e| {
        AppError::DbErr(e.to_string())
    })?;

    tx.commit()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    match state.lobbies.read() {
        Ok(ctx) => match ctx.get(&game_id) {
            Some(lobby_state) => {
                lobby_state
                    .sender
                    .send(EventMessages::NewUserConnected(
                        crate::lobby::LobbyUserUpdate {
                            game_id: game_id,
                            user: user,
                            users_count: users.len(),
                            users: users,
                        },
                    ))
                    .map_err(|e| AppError::InternalServerError(e.to_string()))?;
            }
            None => {
                return Err(AppError::InternalServerError("Looby not found".to_string()));
            }
        },
        Err(e) => {
            return Err(AppError::InternalServerError(e.to_string()));
        }
    }

    Ok(game_id)
}

pub async fn disconnect_user_endpoint(
    Path(id): Path<Uuid>,
    Extension(ref db): Extension<PgPool>,
    _auth: Auth,
) -> Result<(), AppError> {
    sqlx::query!(
        // language=PostgreSQL
        r#"update "user" set game_id = NULL where id = $1"#,
        id
    )
    .execute(db)
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
    let user = get_user(auth.user_id, db).await?;

    let lobby_id = quick_connect(db, &params.connect_code, state, user).await?;

    Ok(Json(lobby_id))
}

pub async fn quick_connect_endpoint_no_user(
    Extension(ref db): Extension<PgPool>,
    params: Query<QuickConnect>,
    Extension(state): Extension<Arc<State>>,
) -> Result<Json<Uuid>, AppError> {
    let count = sqlx::query_scalar!(
        // language=PostgreSQL
        r#"select count(*) from "lobby" where connect_code = $1"#,
        params.connect_code
    )
    .fetch_one(db)
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

    let mut user = CreateUser {
        username: generate_username(),
        password: generate_password(),
        role: UserRole::Temp,
    };

    let mut creation_result = create_user(db, user).await;

    while let Err(AppError::AlreadyExists(_)) = creation_result {
        user = CreateUser {
            username: generate_username(),
            password: generate_password(),
            role: UserRole::Temp,
        };

        creation_result = create_user(db, user).await;
    }

    match creation_result {
        Ok(u) => {
            let lobby_id = quick_connect(db, &params.connect_code, state, u).await?;
            Ok(Json(lobby_id))
        }
        Err(e) => Err(e),
    }
}

pub async fn quick_connect(
    db: &PgPool,
    connect_code: &String,
    state: Arc<State>,
    user: User,
) -> Result<Uuid, AppError> {
    let lobby = sqlx::query_as!(Lobby,
        // language=PostgreSQL
        r#"select id, name, password, public, connect_code, code_use_times, max_players, started, owner_id, settings as "settings: sqlx::types::Json<Settings>", events as "events: sqlx::types::Json<GameEvents>" from "lobby" where connect_code = $1"#,
        connect_code
    )
    .fetch_one(db)
    .await
    .map_err(|e| {
        AppError::NotFound(e.to_string())
    })?;

    //TODO: not safe asynchronously
    if lobby.code_use_times <= 0 {
        return Err(AppError::LobbyFull(
            "Can't connect to lobby with this code".to_string(),
        ));
    }

    let game_id = connect_user(user.id, db, state, lobby.id).await?;

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
