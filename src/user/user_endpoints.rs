use std::sync::Arc;

use axum::{
    extract::{Extension, Path, Query},
    Json,
};
use sqlx::PgPool;
use tracing::{event, Level};
use uuid::Uuid;

use crate::{
    auth::{Auth, AuthAdmin},
    entities::{Lobby, User, UserRole},
    error::AppError,
    lobby::lobby::{get_lobby_players, get_lobby_transaction, send_broadcast_msg, LobbyUserUpdate},
    user::user::{
        connect_user, create_user, generate_password, generate_username, lock_lobby_tables,
        lock_user_tables, quick_connect, update_user_password,
    },
    websockets::EventMessages,
    State,
};

use super::user::{get_user, ConnectUser, CreateUser, QuickConnect, UpdateUser};

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

    let mut tx = db
        .begin()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    event!(Level::DEBUG, "Updating a user: {}", id);

    let updated = update_user_password(id, &mut tx, payload.password).await?;

    event!(Level::INFO, "Updated a user: {}", id);

    tx.commit()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

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
        .map_err(|e| AppError::DbErr(format!("xd3 {}", e)))?;

    event!(
        Level::INFO,
        "Connecting user: {} to game: {}",
        id,
        params.game_id
    );

    lock_user_tables(&mut tx).await?;
    lock_lobby_tables(&mut tx).await?;

    event!(Level::DEBUG, "Tables locked");

    let lobby_id = connect_user(id, &mut tx, state, params.0, true).await?;

    event!(Level::INFO, "User: {} connected, committing...", id);

    tx.commit()
        .await
        .map_err(|e| AppError::DbErr(format!("xd5 {}", e)))?;

    Ok(Json(lobby_id))
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

    if user.game_id.is_none() {
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
            game_id,
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
) -> Result<Json<Lobby>, AppError> {
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

    let lobby = get_lobby_transaction(lobby_id, &mut tx).await?;

    event!(
        Level::INFO,
        "User: {}, quick connected, committing...",
        auth.user_id
    );

    tx.commit()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    Ok(Json(lobby))
}

pub async fn quick_connect_endpoint_no_user(
    Extension(ref db): Extension<PgPool>,
    params: Query<QuickConnect>,
    Extension(state): Extension<Arc<State>>,
) -> Result<Json<Lobby>, AppError> {
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

    let lobby = get_lobby_transaction(lobby_id, &mut tx).await?;

    tx.commit()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    Ok(Json(lobby))
}
