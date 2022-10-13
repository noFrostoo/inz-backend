use std::sync::Arc;

use axum::{
    extract::{Extension, Path},
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio::sync;
use uuid::Uuid;

use rand::Rng;

use crate::{
    auth::{Auth, AuthGameAdmin},
    entities::{Lobby, Settings, Template, User, UserRole},
    error::AppError,
    lobby::get_lobby,
    LobbyState, State,
};

#[derive(Deserialize)]
pub struct CreateTemplateFromLobby {
    pub name: String,
    pub lobby_id: Uuid,
}

#[derive(Deserialize)]
pub struct CreateTemplate {
    pub name: String,
    pub max_players: i16,
    pub settings: Settings,
}

pub async fn create_from_lobby_endpoint(
    Extension(ref db): Extension<PgPool>,
    payload: CreateTemplateFromLobby,
    auth: AuthGameAdmin,
) -> Result<Json<Template>, AppError> {
    let lobby = get_lobby(payload.lobby_id, db).await?;

    let template = sqlx::query_as!(Template,
        // language=PostgreSQL
        r#"insert into "template" (name, max_players, owner_id, settings) values ($1, $2, $3, $4) returning id, name, max_players, owner_id, settings as "settings: sqlx::types::Json<Settings>""#,
        payload.name,
        lobby.max_players,
        auth.user_id,
        sqlx::types::Json(lobby.settings) as _
    )
    .fetch_one(db)
    .await
    .map_err(|e| {
        AppError::DbErr(e.to_string())
    })?;

    Ok(Json(template))
}

pub async fn create_template_endpoint(
    Extension(ref db): Extension<PgPool>,
    payload: CreateTemplate,
    auth: AuthGameAdmin,
) -> Result<Json<Template>, AppError> {
    let template = sqlx::query_as!(Template,
        // language=PostgreSQL
        r#"insert into "template" (name, max_players, owner_id, settings) values ($1, $2, $3, $4) returning id, name, max_players, owner_id, settings as "settings: sqlx::types::Json<Settings>""#,
        payload.name,
        payload.max_players,
        auth.user_id,
        sqlx::types::Json(payload.settings) as _
    )
    .fetch_one(db)
    .await
    .map_err(|e| {
        AppError::DbErr(e.to_string())
    })?;

    Ok(Json(template))
}

pub async fn get_template_endpoint(
    Path(id): Path<Uuid>,
    Extension(ref db): Extension<PgPool>,
    _auth: AuthGameAdmin,
) -> Result<Json<Template>, AppError> {
    let template = get_template(id, db).await?;

    Ok(Json(template))
}

pub async fn get_template(id: Uuid, db: &PgPool) -> Result<Template, AppError> {
    let template = sqlx::query_as!(Template,
        // language=PostgreSQL
        r#"select id, name, max_players, owner_id, settings as "settings: sqlx::types::Json<Settings>" from "template" where id = $1  "#,
        id
    )
    .fetch_one(db)
    .await
    .map_err(|e| {
        AppError::DbErr(e.to_string())
    })?;

    Ok(template)
}

pub async fn delete_template_endpoint(
    Path(id): Path<Uuid>,
    Extension(ref db): Extension<PgPool>,
    auth: Auth,
) -> Result<(), AppError> {
    let template = get_template(id, db).await?;

    if template.owner_id != auth.user_id || auth.role != UserRole::Admin {
        return Err(AppError::Unauthorized(
            "Can't delete this lobby with this role".to_string(),
        ));
    }

    sqlx::query!(
        // language=PostgreSQL
        r#"delete from "lobby" where id = $1 "#,
        id
    )
    .execute(db)
    .await
    .map_err(|e| AppError::DbErr(e.to_string()))?;

    Ok(())
}

pub async fn update_template_endpoint() {}

pub async fn create_lobby_from_template() {}
