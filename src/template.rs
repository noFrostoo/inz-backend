use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Path},
};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool};
use tokio::sync;
use uuid::Uuid;

use rand::{Rng};

use crate::{entities::{Lobby, User, Settings, Template}, error::AppError, State, LobbyState, auth::Auth};

#[derive(Deserialize)]
pub struct CreateLobby {
    pub lobby_id: Uuid
}

pub async fn create_from_lobby(
    Extension(ref db): Extension<PgPool>,
    payload: CreateLobby
) {}

pub async fn create_template() {}

pub async fn get_template() {}

pub async fn delete_template() {}

pub async fn update_template() {}

pub async fn create_lobby_from_template() {}

