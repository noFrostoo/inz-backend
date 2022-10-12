use serde::{Deserialize, Serialize};
use sqlx::{types::{Uuid, Json}};

#[derive(sqlx::Type, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[sqlx(type_name = "color")] // only for PostgreSQL to match a type definition
#[sqlx(rename_all = "lowercase")]
pub enum UserRole {
    User,
    GameAdmin,
    Admin,
    Temp,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub password: String,
    pub game_id: Option<Uuid>,
    pub role: UserRole,
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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct Settings {

}


#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct Template {
    pub id: Uuid,
    pub name: String,
    pub max_players: i16,
    pub owner_id: Uuid,
    pub settings: Json<Settings>
}


