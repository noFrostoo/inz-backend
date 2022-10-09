use serde::{Deserialize, Serialize};
use sqlx::{types::{Uuid, Json}};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub password: String,
    pub game_id: Option<Uuid>,
    pub temp: bool
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct Lobby {
    pub id: Uuid,
    pub name: String,
    pub password: Option<String>,
    pub connect_code: Option<String>,
    pub code_use_times: Option<i16>,
    pub max_players: i16,
    pub started: bool,
    pub owner_id: Uuid,
    pub settings: Json<Settings>
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct Settings {

}


