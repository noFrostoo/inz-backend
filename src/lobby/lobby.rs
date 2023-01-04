use std::{sync::Arc, collections::BTreeMap};

use serde::{Deserialize, Serialize};
use sqlx::{Executor, PgPool, Postgres, Transaction};
use tokio::sync;
use uuid::Uuid;

use rand::Rng;

use crate::{
    auth::AuthAdmin,
    entities::{GameEvents, Lobby, Settings, User, UserRole},
    error::AppError,
    websockets::EventMessages,
    LobbyState, State, user::user::get_user,
};

const  MAX_PLAYERS:usize = 33; 

#[derive(Serialize, Deserialize)]
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

#[derive(Serialize, Deserialize)]
pub enum LobbiesType {
    Public,
    Private,
    All,
}

#[derive(Serialize, Deserialize)]
pub struct LobbiesQuery {
    pub lobby_type: LobbiesType,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
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
    auth: AuthAdmin,
) -> Result<LobbyResponse, AppError> {
    let owner_id = auth.user_id;
    let mut connect_code: Option<String> = None;
    let owner = get_user(owner_id, &mut *tx).await?;

    if payload.generate_connect_code {
        let code = generate_valid_code(&mut *tx).await?;

        connect_code = Some(code);
    }

    let settings = match payload.settings {
        Some(s) => s,
        None => Settings::default(),
    };

    let events = GameEvents::new();

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

    if let Some(_) = state.lobbies.read().await.get(&lobby.id) {
        return Err(AppError::NotFound("Looby already created".to_string()));
    }

    let (tx, rx) = sync::broadcast::channel(MAX_PLAYERS);
    state.lobbies.write().await.insert(
        lobby.id,
        LobbyState {
            sender: Arc::new(tx),
            _receiver: Arc::new(rx),
            started: false,
            round_state: crate::RoundState::new(),
        },
    );

    Ok(LobbyResponse{ lobby, players: Vec::new(), owner: owner })
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

pub async fn get_lobby<'a, E>(id: Uuid, db: E) -> Result<Lobby, AppError>
where
    E: Executor<'a, Database = Postgres>,
{
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

    let players = get_lobby_users_transaction(id, tx).await?;

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

pub async fn get_lobby_users_transaction(
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

pub async fn get_lobby_users(id: Uuid, db: &PgPool) -> Result<Vec<User>, AppError> {
    let players = sqlx::query_as!(
        User,
        // language=PostgreSQL
        r#"select id, username, password, game_id, role as "role: UserRole" from "user" where game_id = $1 "#,
        id
    )
    .fetch_all(db)
    .await
    .map_err(|e| AppError::DbErr(e.to_string()))?;
    Ok(players)
}

pub async fn get_lobby_players(
    id: Uuid,
    tx: &mut Transaction<'_, Postgres>,
) -> Result<Vec<User>, AppError> {
    let lobby = get_lobby(id, &mut *tx).await?;

    let users = sqlx::query_as!(
        User,
        // language=PostgreSQL
        r#"select id, username, password, game_id, role as "role: UserRole" from "user" where game_id = $1 "#,
        id
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| AppError::DbErr(e.to_string()))?;

    let mut players: Vec<User> = Vec::new();
    for user in users {
        if user.id != lobby.owner_id {
            players.push(user);
        }
    }

    Ok(players)
}

pub async fn update_lobby(
    id: Uuid,
    tx: &mut Transaction<'_, Postgres>,
    payload: CreateLobby,
    state: Arc<State>,
    auth: AuthAdmin,
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
        &state,
        id,
        EventMessages::LobbyUpdate(LobbyUpdate {
            id,
            users: lobby.players.clone(),
            lobby: lobby.lobby.clone(),
        }),
    )
    .await?;

    Ok(lobby)
}

pub async fn update_lobby_classes(
    state: &Arc<State>,
    game_id: Uuid,
    classes: BTreeMap<Uuid, u32>
) -> Result<(), AppError> {
    //TODO: check classes
    match state.lobbies.write().await.get_mut(&game_id) {
        Some(lobby) => lobby.round_state.player_classes = classes.clone(),
        None => return Err(AppError::BadRequest("game not found with this id".to_string())),
    }

    send_broadcast_msg(state, game_id, EventMessages::UpdateClasses(classes)).await?;

    Ok(())
}


pub async fn send_broadcast_msg(
    state: &Arc<State>,
    id: Uuid,
    msg: EventMessages,
) -> Result<(), AppError> {
    print!("lobbies: {:?}", state.lobbies.read().await);

    match state.lobbies.read().await.get(&id) {
        Some(lobby_state) => {
            lobby_state
                .sender
                .send(msg)
                .map_err(|e| AppError::InternalServerError(e.to_string()))?;
        }
        None => {
            return Err(AppError::InternalServerError("Looby not found".to_string()));
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
