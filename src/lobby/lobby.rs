use std::sync::Arc;

use axum::{
    extract::{Extension, Query},
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, QueryBuilder, Transaction};
use tokio::sync;
use uuid::Uuid;

use rand::Rng;

use crate::{
    auth::{Auth, AuthGameAdmin},
    entities::{GameEvents, Lobby, Settings, User, UserRole},
    error::AppError,
    websockets::EventMessages,
    LobbyState, State,
};

#[derive(Deserialize)]
pub struct CreateLobby {
    pub name: String,
    pub password: Option<String>,
    pub public: bool,
    pub generate_connect_code: bool,
    pub code_use_times: i16,
    pub max_players: i16,
    pub settings: Option<Settings>,
    pub events: Option<GameEvents>,
}

#[derive(Deserialize)]
pub enum LobbiesType {
    Public,
    Private,
    All,
}

#[derive(Deserialize)]
pub struct LobbiesQuery {
    pub lobby_type: LobbiesType,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
pub struct LobbyUserUpdate {
    pub game_id: Uuid,
    pub user: User,
    pub users_count: usize,
    pub users: Vec<User>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
pub struct LobbyUpdate {
    pub id: Uuid,
    pub users: Vec<User>,
    pub lobby: Lobby,
}

//TODO: better name
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
pub struct LobbyResponse {
    pub lobby: Lobby,
    pub players: Vec<User>,
    pub owner: User,
}

pub async fn create_lobby(
    tx: &mut Transaction<'_, Postgres>,
    payload: CreateLobby,
    state: Arc<State>,
    auth: AuthGameAdmin,
) -> Result<Lobby, AppError> {
    let owner_id = auth.user_id;
    let mut connect_code: Option<String> = None;

    if payload.generate_connect_code {
        let code = generate_valid_code(&mut *tx).await?;

        connect_code = Some(code);
    }

    let settings = match payload.settings {
        Some(s) => s,
        None => Settings::default(),
    };

    let events = GameEvents {};

    let lobby = sqlx::query_as!(Lobby,
        // language=PostgreSQL
        r#"insert into "lobby" (name, password, public, connect_code, code_use_times, max_players, owner_id, started, settings, events) values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) returning id, name, password, public, connect_code, code_use_times, max_players, started, owner_id, settings as "settings: sqlx::types::Json<Settings>", events as "events: sqlx::types::Json<GameEvents>""#,
        payload.name,
        payload.password,
        payload.public,
        connect_code,
        payload.code_use_times,
        payload.max_players,
        owner_id,
        false,
        sqlx::types::Json(settings) as _,
        sqlx::types::Json(events) as _
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        AppError::DbErr(e.to_string())
    })?;

    match state.lobbies.write() {
        Ok(mut ctx) => {
            match ctx.get(&lobby.id) {
                Some(_) => {
                    return Err(AppError::NotFound("Looby already created".to_string()));
                }
                None => {
                    //TODO: fix magic number
                    let (tx, _rx) = sync::broadcast::channel(33);
                    ctx.insert(lobby.id, LobbyState { sender: tx });
                }
            }
        }
        Err(e) => {
            return Err(AppError::InternalServerError(e.to_string()));
        }
    }

    Ok(lobby)
}

async fn generate_valid_code(tx: &mut Transaction<'_, Postgres>) -> Result<String, AppError> {
    let mut count: Option<i64> = Some(0);
    let mut code = generate_connect_code();
    while let Some(1) = count {
        count = sqlx::query_scalar!(
            // language=PostgreSQL
            r#"select COUNT(*) from "lobby"  where connect_code = $1"#,
            code
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| AppError::DbErr(e.to_string()))?;

        code = generate_connect_code();
    }
    Ok(code)
}

pub async fn get_lobby(id: Uuid, db: &PgPool) -> Result<Lobby, AppError> {
    let lobby = sqlx::query_as!(Lobby,
        // language=PostgreSQL
        r#"select id, name, password, public, connect_code, code_use_times, max_players, started, owner_id, settings as "settings: sqlx::types::Json<Settings>", events as "events: sqlx::types::Json<GameEvents>" from "lobby" where id = $1"#,
        id
    )
    .fetch_one(db)
    .await
    .map_err(|e| {
        AppError::NotFound(e.to_string())
    })?;

    Ok(lobby)
}

pub async fn get_lobby_transaction(
    id: Uuid,
    tx: &mut Transaction<'_, Postgres>,
) -> Result<Lobby, AppError> {
    let lobby = sqlx::query_as!(Lobby,
        // language=PostgreSQL
        r#"select id, name, password, public, connect_code, code_use_times, max_players, started, owner_id, settings as "settings: sqlx::types::Json<Settings>", events as "events: sqlx::types::Json<GameEvents>" from "lobby" where id = $1"#,
        id
    )
    .fetch_one(tx)
    .await
    .map_err(|e| {
        AppError::NotFound(e.to_string())
    })?;

    Ok(lobby)
}

//TODO: refactor name
pub async fn get_lobby_response(
    id: Uuid,
    tx: &mut Transaction<'_, Postgres>,
) -> Result<LobbyResponse, AppError> {
    let lobby = get_lobby_transaction(id, tx).await?;

    let players = get_lobby_players(id, tx).await?;

    let owner = sqlx::query_as!(User,
        // language=PostgreSQL
        r#"select id, username, password, game_id, role as "role: UserRole" from "user" where id = $1 "#,
        lobby.owner_id
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        AppError::DbErr(e.to_string())
    })?;

    Ok(LobbyResponse {
        lobby,
        players,
        owner,
    })
}

pub async fn get_lobby_players(
    id: Uuid,
    tx: &mut Transaction<'_, Postgres>,
) -> Result<Vec<User>, AppError> {
    let players = sqlx::query_as!(
        User,
        // language=PostgreSQL
        r#"select id, username, password, game_id, role as "role: UserRole" from "user" where game_id = $1 "#,
        id
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| AppError::DbErr(e.to_string()))?;
    Ok(players)
}

pub async fn get_lobbies_endpoint(
    Extension(ref db): Extension<PgPool>,
    Query(lobby_query): Query<LobbiesQuery>,
    _auth: Auth,
) -> Result<Json<Vec<Lobby>>, AppError> {
    let mut builder = QueryBuilder::new("select id, name, password, public, connect_code, code_use_times, max_players, started, owner_id, settings as \"settings: sqlx::types::Json<Settings>\", events as \"events: sqlx::types::Json<GameEvent>\" from \"lobby\" ");

    match lobby_query.lobby_type {
        LobbiesType::Public => {
            builder.push("where public = $1").push_bind(true);
        }
        LobbiesType::Private => {
            builder.push("where public = $1").push_bind(false);
        }
        LobbiesType::All => {}
    }

    let query = builder.build_query_as::<Lobby>();

    let lobbies = query
        .fetch_all(db)
        .await
        .map_err(|e| AppError::NotFound(e.to_string()))?;

    Ok(Json(lobbies))
}

pub async fn update_lobby(
    id: Uuid,
    tx: &mut Transaction<'_, Postgres>,
    payload: CreateLobby,
    state: Arc<State>,
    auth: AuthGameAdmin,
) -> Result<LobbyResponse, AppError> {
    let old = get_lobby_response(id, &mut *tx).await?;

    if old.lobby.owner_id != auth.user_id && auth.role != UserRole::Admin {
        return Err(AppError::Unauthorized(
            "can't edit this template".to_string(),
        ));
    }

    let connect_code: Option<String>;
    let mut connect_code_use_times = payload.code_use_times;

    if payload.generate_connect_code {
        connect_code = Some(generate_valid_code(tx).await?);
    } else {
        connect_code = old.lobby.connect_code;
        connect_code_use_times = old.lobby.code_use_times;
    }

    let settings = match payload.settings {
        Some(s) => s,
        None => old.lobby.settings.0,
    };

    let events = match payload.events {
        Some(e) => e,
        None => old.lobby.events.0,
    };

    sqlx::query_as!(Lobby,
        // language=PostgreSQL
        r#"update "lobby" set name = $1, password = $2, connect_code = $3, code_use_times = $4, max_players = $5, settings = $6, public = $7, events = $8 where id = $9  returning id, name, password, public, connect_code, code_use_times, max_players, started, owner_id, settings as "settings: sqlx::types::Json<Settings>", events as "events: sqlx::types::Json<GameEvents>""#,
        payload.name,
        payload.password,
        connect_code,
        connect_code_use_times,
        payload.max_players,
        sqlx::types::Json(settings) as _,
        payload.public,
        sqlx::types::Json(events) as _,
        id
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        AppError::DbErr(e.to_string())
    })?;

    let lobby = get_lobby_response(id, &mut *tx).await?;

    send_broadcast_msg(
        state,
        id,
        EventMessages::LobbyUpdate(LobbyUpdate {
            id,
            users: lobby.players.clone(),
            lobby: lobby.lobby.clone(),
        }),
    )?;

    Ok(lobby)
}

pub fn send_broadcast_msg(state: Arc<State>, id: Uuid, msg: EventMessages) -> Result<(), AppError> {
    match state.lobbies.read() {
        Ok(ctx) => match ctx.get(&id) {
            Some(lobby_state) => {
                lobby_state
                    .sender
                    .send(msg)
                    .map_err(|e| AppError::InternalServerError(e.to_string()))?;
            }
            None => {
                return Err(AppError::InternalServerError("Looby not found".to_string()));
            }
        },
        Err(e) => {
            return Err(AppError::InternalServerError(e.to_string()));
        }
    };
    Ok(())
}

fn generate_connect_code() -> String {
    let mut rng = rand::thread_rng();
    let letter: char = rng.gen_range(b'A'..=b'Z') as char;
    let number: u32 = rng.gen_range(0..999999);
    format!("{}{:06}", letter, number)
}
