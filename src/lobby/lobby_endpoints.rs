use std::{collections::BTreeMap, sync::Arc};

use axum::{
    extract::{Extension, Path, Query},
    Json,
};
use sqlx::{PgPool, QueryBuilder};
use uuid::Uuid;

use crate::{
    auth::{Auth, AuthAdmin},
    entities::{Lobby, UserRole},
    error::AppError,
    user::user::lock_lobby_tables,
    websockets::EventMessages,
    State,
};

use super::{
    game::{start_new_game, GameEnd},
    lobby::{
        create_lobby, get_lobby, get_lobby_players, get_lobby_response, get_lobby_transaction,
        send_broadcast_msg, update_lobby, CreateLobby, LobbiesQuery, LobbiesType, LobbyResponse,
    },
};

pub async fn create_lobby_endpoint(
    Extension(ref db): Extension<PgPool>,
    Json(payload): Json<CreateLobby>,
    Extension(state): Extension<Arc<State>>,
    auth: AuthAdmin,
) -> Result<Json<Lobby>, AppError> {
    let mut tx = db
        .begin()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

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

pub async fn get_lobbies_endpoint(
    Extension(ref db): Extension<PgPool>,
    Query(lobby_query): Query<LobbiesQuery>,
    _auth: Auth,
) -> Result<Json<Vec<Lobby>>, AppError> {
    let mut builder = QueryBuilder::new("select * from \"lobby\" ");

    match lobby_query.lobby_type {
        LobbiesType::Public => {
            builder.push("where public = true");
        }
        LobbiesType::Private => {
            builder.push("where public = false");
        }
        LobbiesType::All => {}
    }

    print!("{}", builder.sql());

    let query = builder.build_query_as::<Lobby>();

    let lobbies = query
        .fetch_all(db)
        .await
        .map_err(|e| AppError::NotFound(e.to_string()))?;

    Ok(Json(lobbies))
}

pub async fn delete_lobby_endpoint(
    Path(id): Path<Uuid>,
    Extension(ref db): Extension<PgPool>,
    Extension(state): Extension<Arc<State>>,
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

    if lobby.started {
        return Err(AppError::BadRequest("Game started".to_string()))
    }
    
    // just to be sure
    send_broadcast_msg(&state, id, EventMessages::KickAll).await?;

    sqlx::query!(
        // language=PostgreSQL
        r#"update "user" set game_id = NULL where game_id = $1"#,
        id
    )
    .execute(&mut tx)
    .await
    .map_err(|e| AppError::DbErr(e.to_string()))?;

    sqlx::query!(
        // language=PostgreSQL
        r#"delete from "lobby" where id = $1 "#,
        id
    )
    .execute(&mut tx)
    .await
    .map_err(|e| AppError::DbErr(e.to_string()))?;

    state.lobbies.write().await.remove(&id);

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
    auth: AuthAdmin,
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
    Json(player_classes): Json<BTreeMap<Uuid, u32>>,
    _auth: AuthAdmin,
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

    start_new_game(&mut tx, id, lobby, players, player_classes, &state).await?;

    tx.commit()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    Ok(())
}

pub async fn stop_game_endpoint(
    Path(id): Path<Uuid>,
    Extension(ref db): Extension<PgPool>,
    Extension(state): Extension<Arc<State>>,
    _auth: AuthAdmin,
) -> Result<(), AppError> {
    let lobby = get_lobby(id, db).await?;

    if !lobby.started {
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
        false,
        id
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| AppError::DbErr(e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    Ok(())
}
