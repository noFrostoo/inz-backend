use serde::{Deserialize, Serialize};
use sqlx::types::{Json, Uuid};

#[derive(sqlx::Type, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[sqlx(type_name = "user_role")] // only for PostgreSQL to match a type definition
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
    pub public: bool,
    pub connect_code: Option<String>,
    pub code_use_times: i16,
    pub max_players: i16,
    pub started: bool,
    pub owner_id: Uuid,
    pub settings: Json<Settings>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct Settings {
    pub order_queue: i16,
    pub resource_price: i64,
    pub order_realization_time: i16, //TODO: Type ?
    pub start_money: i64,
    pub play_time: i64,
    pub round_time: i64,
    pub demand_style: String,
    pub transport_cost: i64,
    pub magazine_cost: i64,
    pub order_cost: i64,
    pub demand_cost: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct GameEvent {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct Game {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct Template {
    pub id: Uuid,
    pub name: String,
    pub max_players: i16,
    pub owner_id: Uuid,
    pub settings: Json<Settings>,
}
