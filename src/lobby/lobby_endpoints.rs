use std::sync::Arc;

use axum::{
    extract::{Extension, Path},
    Json,
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    auth::{Auth, AuthGameAdmin},
    entities::{Lobby, UserRole},
    error::AppError,
    user::user::lock_lobby_tables,
    websockets::EventMessages,
    State,
};

use super::lobby::{
    create_lobby, get_lobby, get_lobby_players, get_lobby_response, get_lobby_transaction,
    send_broadcast_msg, update_lobby, CreateLobby, LobbyResponse, LobbyUpdate,
};

pub async fn create_lobby_endpoint(
    Extension(ref db): Extension<PgPool>,
    Json(payload): Json<CreateLobby>,
    Extension(state): Extension<Arc<State>>,
    auth: AuthGameAdmin,
) -> Result<Json<Lobby>, AppError> {
    let mut tx = db
        .begin()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    //TODO: lock lobby table ?

    let lobby = create_lobby(&mut tx, payload, state, auth).await?;

    tx.commit()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    Ok(Json(lobby))
}

pub async fn get_lobby_endpoint(
    Path(id): Path<Uuid>,
    Extension(ref db): Extension<PgPool>,
    _auth: Auth,
) -> Result<Json<LobbyResponse>, AppError> {
    let mut tx = db
        .begin()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    let response = get_lobby_response(id, &mut tx).await?;

    tx.commit()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    Ok(Json(response))
}

pub async fn delete_lobby_endpoint(
    Path(id): Path<Uuid>,
    Extension(ref db): Extension<PgPool>,
    auth: Auth,
) -> Result<(), AppError> {
    let mut tx = db
        .begin()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    lock_lobby_tables(&mut tx).await?;

    let lobby = get_lobby_transaction(id, &mut tx).await?;

    if lobby.owner_id != auth.user_id || auth.role != UserRole::Admin {
        return Err(AppError::Unauthorized(
            "Can't delete this lobby with this role".to_string(),
        ));
    }

    sqlx::query!(
        // language=PostgreSQL
        r#"delete from "lobby" where id = $1 "#,
        id
    )
    .execute(&mut tx)
    .await
    .map_err(|e| AppError::DbErr(e.to_string()))?;

    sqlx::query!(
        // language=PostgreSQL
        r#"update "user" set game_id = NULL where game_id = $1"#,
        id
    )
    .execute(&mut tx)
    .await
    .map_err(|e| AppError::DbErr(e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    Ok(())
}

pub async fn update_lobby_endpoint(
    Path(id): Path<Uuid>,
    Extension(ref db): Extension<PgPool>,
    Extension(state): Extension<Arc<State>>,
    Json(payload): Json<CreateLobby>,
    auth: AuthGameAdmin,
) -> Result<Json<LobbyResponse>, AppError> {
    let mut tx = db
        .begin()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    lock_lobby_tables(&mut tx).await?;

    let lobby = update_lobby(id, &mut tx, payload, state, auth).await?;

    tx.commit()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    Ok(Json(lobby))
}

pub async fn start_game_endpoint(
    Path(id): Path<Uuid>,
    Extension(ref db): Extension<PgPool>,
    Extension(state): Extension<Arc<State>>,
    _auth: AuthGameAdmin,
) -> Result<(), AppError> {
    let lobby = get_lobby(id, db).await?;

    if lobby.started {
        return Err(AppError::GameStarted(lobby.name));
    }

    let mut tx = db
        .begin()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    lock_lobby_tables(&mut tx).await?;

    sqlx::query!(
        // language=PostgreSQL
        r#"update "lobby" set started = $1 where id = $2 "#,
        true,
        id
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| AppError::DbErr(e.to_string()))?;

    let players = get_lobby_players(id, &mut tx).await?;

    send_broadcast_msg(
        state,
        id,
        EventMessages::GameStart(LobbyUpdate {
            id,
            users: players,
            lobby,
        }),
    )?;

    tx.commit()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    Ok(())
}
