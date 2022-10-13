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
    entities::{Lobby, Settings, User, UserRole},
    error::AppError,
    LobbyState, State,
};

// the input to our `create_user` handler
#[derive(Deserialize)]
pub struct CreateLobby {
    pub name: String,
    pub password: Option<String>,
    pub generate_connect_code: bool,
    pub code_use_times: i16,
    pub max_players: i16,
    pub settings: Option<Settings>,
}

#[derive(Deserialize)]
pub struct UpdateLobby {
    pub name: String,
    pub password: Option<String>,
    pub generate_connect_code: bool,
    pub code_use_times: i16,
    pub max_players: i16,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
pub struct LobbyUserUpdate {
    pub game_id: Uuid,
    pub user: User,
    pub users_count: usize,
    pub users: Vec<User>,
}

//TODO: better name
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
pub struct LobbyResponse {
    pub lobby: Lobby,
    pub players: Vec<User>,
    pub owner: User,
}

pub async fn create_lobby_endpoint(
    Extension(ref db): Extension<PgPool>,
    Json(payload): Json<CreateLobby>,
    Extension(state): Extension<Arc<State>>,
    auth: AuthGameAdmin,
) -> Result<Json<Lobby>, AppError> {
    let owner_id = auth.user_id; //TODO; if not given id of current user
    let mut code_option: Option<String> = None;

    if payload.generate_connect_code {
        let mut count: Option<i64> = Some(0);
        let mut code = generate_connect_code();

        while let Some(1) = count {
            count = sqlx::query_scalar!(
                // language=PostgreSQL
                r#"select COUNT(*) from "lobby"  where connect_code = $1"#,
                code
            )
            .fetch_one(db)
            .await
            .map_err(|e| AppError::DbErr(e.to_string()))?;

            code = generate_connect_code();
        }

        code_option = Some(code);
    }

    let settings = match payload.settings {
        Some(s) => s,
        None => Settings::default(),
    };

    let lobby = sqlx::query_as!(Lobby,
        // language=PostgreSQL
        r#"insert into "lobby" (name, password, connect_code, code_use_times, max_players, owner_id, started, settings) values ($1, $2, $3, $4, $5, $6, $7, $8) returning id, name, password, connect_code, code_use_times, max_players, started, owner_id, settings as "settings: sqlx::types::Json<Settings>""#,
        payload.name,
        payload.password,
        code_option,
        payload.code_use_times,
        payload.max_players,
        owner_id,
        false,
        sqlx::types::Json(settings) as _
    )
    .fetch_one(db)
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

    Ok(Json(lobby))
}

pub async fn get_lobby_endpoint(
    Path(id): Path<Uuid>,
    Extension(ref db): Extension<PgPool>,
    _auth: Auth,
) -> Result<Json<LobbyResponse>, AppError> {
    let response = get_lobby_response(id, db).await?;

    Ok(Json(response))
}

pub async fn get_lobby(id: Uuid, db: &PgPool) -> Result<Lobby, AppError> {
    let lobby = sqlx::query_as!(Lobby,
        // language=PostgreSQL
        r#"select id, name, password, connect_code, code_use_times, max_players, started, owner_id, settings as "settings: sqlx::types::Json<Settings>" from "lobby" where id = $1"#,
        id
    )
    .fetch_one(db)
    .await
    .map_err(|e| {
        AppError::NotFound(e.to_string())
    })?;

    return Ok(lobby);
}

//TODO: refactor name
async fn get_lobby_response(id: Uuid, db: &PgPool) -> Result<LobbyResponse, AppError> {
    let lobby = get_lobby(id, db).await?;

    let players = sqlx::query_as!(
        User,
        // language=PostgreSQL
        r#"select id, username, password, game_id, role as "role: UserRole" from "user" "#,
    )
    .fetch_all(db)
    .await
    .map_err(|e| AppError::DbErr(e.to_string()))?;

    let owner = sqlx::query_as!(User,
        // language=PostgreSQL
        r#"select id, username, password, game_id, role as "role: UserRole" from "user" where id = $1 "#,
        lobby.owner_id
    )
    .fetch_one(db)
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

pub async fn get_lobbies_endpoint(
    Extension(ref db): Extension<PgPool>,
    _auth: Auth,
) -> Result<Json<Vec<Lobby>>, AppError> {
    let lobbies = sqlx::query_as!(Lobby,
        // language=PostgreSQL
        r#"select id, name, password, connect_code, code_use_times, max_players, started, owner_id, settings as "settings: sqlx::types::Json<Settings>" from "lobby" "#,
    )
    .fetch_all(db)
    .await
    .map_err(|e| {
        AppError::NotFound(e.to_string())
    })?;

    Ok(Json(lobbies))
}

pub async fn delete_lobby_endpoint(
    Path(id): Path<Uuid>,
    Extension(ref db): Extension<PgPool>,
    auth: Auth,
) -> Result<(), AppError> {
    let lobby = get_lobby(id, db).await?;

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
    .execute(db)
    .await
    .map_err(|e| AppError::DbErr(e.to_string()))?;

    sqlx::query!(
        // language=PostgreSQL
        r#"update "user" set game_id = NULL where game_id = $1"#,
        id
    )
    .execute(db)
    .await
    .map_err(|e| AppError::DbErr(e.to_string()))?;

    Ok(())
}

fn generate_connect_code() -> String {
    let mut rng = rand::thread_rng();
    let letter: char = rng.gen_range(b'A'..b'Z') as char;
    let number: u32 = rng.gen_range(0..999999);
    format!("{}{:06}", letter, number)
}
