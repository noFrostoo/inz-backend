use std::sync::Arc;

use axum::{
    extract::{Extension, Path},
    Json,
};
use serde::Deserialize;
use sqlx::{Executor, PgPool, Postgres};
use uuid::Uuid;

use crate::{
    auth::{Auth, AuthGameAdmin},
    entities::{GameEvents, Lobby, Settings, Template, UserRole},
    error::AppError,
    lobby::lobby::{create_lobby, get_lobby, CreateLobby},
    State,
};

#[derive(Deserialize)]
pub struct CreateTemplateFromLobby {
    pub name: String,
    pub lobby_id: Uuid,
}

#[derive(Deserialize)]
pub struct CreateLobbyFromTemplate {
    pub name: String,
    pub password: String,
    pub generate_connect_code: bool,
    pub code_use_times: i16,
    pub public: bool,
}

#[derive(Deserialize)]
pub struct CreateTemplate {
    pub name: String,
    pub max_players: i16,
    pub settings: Settings,
    pub events: GameEvents,
}

pub async fn create_template_from_lobby_endpoint(
    Extension(ref db): Extension<PgPool>,
    Json(payload): Json<CreateTemplateFromLobby>,
    auth: AuthGameAdmin,
) -> Result<Json<Template>, AppError> {
    let lobby = get_lobby(payload.lobby_id, db).await?;

    let template = sqlx::query_as!(Template,
        // language=PostgreSQL
        r#"insert into "template" (name, max_players, owner_id, settings, events) values ($1, $2, $3, $4, $5) returning id, name, max_players, owner_id, settings as "settings: sqlx::types::Json<Settings>", events as "events: sqlx::types::Json<GameEvents>""#,
        payload.name,
        lobby.max_players,
        auth.user_id,
        sqlx::types::Json(lobby.settings) as _,
        sqlx::types::Json(lobby.events) as _
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
    Json(payload): Json<CreateTemplate>,
    auth: AuthGameAdmin,
) -> Result<Json<Template>, AppError> {
    let template = create_template(
        db,
        auth.user_id,
        payload.max_players,
        payload.name,
        payload.settings,
        payload.events,
    )
    .await?;
    // language=PostgreSQL

    Ok(Json(template))
}

pub async fn create_template<'a, E>(
    db: E,
    owner_id: Uuid,
    max_players: i16,
    name: String,
    settings: Settings,
    events: GameEvents,
) -> Result<Template, AppError>
where
    E: Executor<'a, Database = Postgres>,
{
    let template = sqlx::query_as!(Template,
        // language=PostgreSQL
        r#"insert into "template" (name, max_players, owner_id, settings, events) values ($1, $2, $3, $4, $5) returning id, name, max_players, owner_id, settings as "settings: sqlx::types::Json<Settings>", events as "events: sqlx::types::Json<GameEvents>""#,
        name,
        max_players,
        owner_id,
        sqlx::types::Json(settings) as _,
        sqlx::types::Json(events) as _
    )
    .fetch_one(db)
    .await
    .map_err(|e| {
        AppError::DbErr(e.to_string())
    })?;

    Ok(template)
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
        r#"select id, name, max_players, owner_id, settings as "settings: sqlx::types::Json<Settings>", events as "events: sqlx::types::Json<GameEvents>" from "template" where id = $1  "#,
        id
    )
    .fetch_one(db)
    .await
    .map_err(|e| {
        AppError::DbErr(e.to_string())
    })?;

    Ok(template)
}

pub async fn get_templates_endpoint(
    Extension(ref db): Extension<PgPool>,
    auth: AuthGameAdmin,
) -> Result<Json<Vec<Template>>, AppError> {
    let templates = sqlx::query_as!(Template,
        // language=PostgreSQL
        r#"select id, name, max_players, owner_id, settings as "settings: sqlx::types::Json<Settings>", events as "events: sqlx::types::Json<GameEvents>" from "template" where owner_id = $1  "#,
        auth.user_id
    )
    .fetch_all(db)
    .await
    .map_err(|e| {
        AppError::DbErr(e.to_string())
    })?;

    Ok(Json(templates))
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

pub async fn update_template_endpoint(
    Path(id): Path<Uuid>,
    Extension(ref db): Extension<PgPool>,
    Json(payload): Json<CreateTemplate>,
    auth: AuthGameAdmin,
) -> Result<Json<Template>, AppError> {
    let old = get_lobby(id, db).await?;

    if old.owner_id != auth.user_id && auth.role != UserRole::Admin {
        return Err(AppError::Unauthorized(
            "can't edit this template".to_string(),
        ));
    }

    let template = sqlx::query_as!(Template,
        // language=PostgreSQL
        r#"update "template" set name = $1, max_players = $2, settings = $3, events = $4 returning id, name, max_players, owner_id, settings as "settings: sqlx::types::Json<Settings>", events as "events: sqlx::types::Json<GameEvents>""#,
        payload.name,
        payload.max_players,
        sqlx::types::Json(payload.settings) as _,
        sqlx::types::Json(payload.events) as _
    )
    .fetch_one(db)
    .await
    .map_err(|e| {
        AppError::DbErr(e.to_string())
    })?;

    Ok(Json(template))
}

pub async fn create_lobby_from_template(
    Path(id): Path<Uuid>,
    Json(payload): Json<CreateLobbyFromTemplate>,
    Extension(ref db): Extension<PgPool>,
    Extension(state): Extension<Arc<State>>,
    auth: AuthGameAdmin,
) -> Result<Json<Lobby>, AppError> {
    let template = get_template(id, db).await?;

    let mut tx = db
        .begin()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    let lobby = create_lobby(
        &mut tx,
        CreateLobby {
            name: payload.name,
            password: Some(payload.password),
            generate_connect_code: payload.generate_connect_code,
            code_use_times: payload.code_use_times,
            max_players: template.max_players,
            settings: Some(template.settings.0),
            public: payload.public,
            events: Some(template.events.0),
        },
        state,
        auth,
    )
    .await?;

    tx.commit()
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

    Ok(Json(lobby))
}
