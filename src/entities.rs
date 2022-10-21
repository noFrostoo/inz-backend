use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sqlx::{
    types::{Json, Uuid},
    FromRow,
};

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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash, FromRow)]
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
    pub events: Json<GameEvents>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct Settings {
    pub order_queue: i16,
    pub resource_price: i64,
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
pub struct GameEvents {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct Game {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct GameState {
    pub id: Uuid,
    pub round: i64,
    pub user_states: Json<BTreeMap<Uuid, UserState>>,
    pub orders: Json<BTreeMap<Uuid, Order>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct Order {
    pub recipient: Uuid,
    pub sender: Uuid,
    pub value: i64,
    pub cost: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct UserState {
    pub user_id: Uuid,
    pub money: i64,
    pub magazine_state: i64,
    pub performance: i64,
    pub back_order: Vec<Order>,
    pub current_order: Order,
    pub user_order: Order, //TODO bad name , what user orders
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct Template {
    pub id: Uuid,
    pub name: String,
    pub max_players: i16,
    pub owner_id: Uuid,
    pub settings: Json<Settings>,
    pub events: Json<GameEvents>,
}
