use std::{collections::BTreeMap, default};

use serde::{Deserialize, Serialize};
use sqlx::{
    types::{Json, Uuid},
    FromRow,
};

#[derive(sqlx::Type, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[sqlx(type_name = "user_role")]
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
    pub resource_price: i64,
    pub start_money: i64,
    pub start_magazine: i64,
    pub start_order_queue: Vec<Order>,
    pub play_time: i64,
    pub round_time: i64,
    pub demand_style: DemandStyle,
    pub transport_cost: i64,
    pub magazine_cost: i64,
    pub fix_order_cost: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum DemandStyle {
    #[default]
    Default,
    Linear {
        start: i64,
        increase: i64,
    },
    Multiplication {
        start: i64,
        increase: i64,
    }, //TODO: refactor name
    Exponential {
        start: i64,
        power: i64,
        modulator: i64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct GameEvents {
    events: Vec<GameEvent>,
}

impl GameEvents {
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct GameEvent {
    condition: EventCondition,
    actions: Vec<EventAction>,
    run_once: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum EventCondition {
    RoundMet { round: i64 },
    ValueExceed { resource: Resource, met_by: MetBy },
    SingleChange { resource: Resource, value: i64 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum Resource {
    Money,
    MagazineState,
    Performance,
    BackOrderValue,
    BackOrder,
    UserOrder,
    ReceivedOrder,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum EventAction {
    ShowMessage {
        message: String,
    },
    ChangeSettings {
        new_settings: Settings,
    },
    AddResource {
        resource: Resource,
        target: ActionTarget,
    },
}

//TODO: refactor name
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum MetBy {
    SinglePlayer,
    PlayerPercent(i64),
    Average,
    AllPlayers,
}

//TODO: refactor name
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum ActionTarget {
    EventTarget,
    AllPlayers,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct GameState {
    pub id: Uuid,
    pub round: i64,
    pub user_states: Json<BTreeMap<Uuid, UserState>>,
    pub round_orders: Json<BTreeMap<Uuid, Order>>,
    pub send_orders: Json<BTreeMap<Uuid, Order>>,
    pub flow: Json<Flow>,
    pub demand: i64,
}

#[derive(Clone, Debug, PartialEq, Default, Eq, Serialize, Deserialize, Hash)]
pub struct Flow {
    pub last_player: Uuid,
    pub first_player: Uuid,
    pub flow: BTreeMap<Uuid, Uuid>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Hash)]
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
    pub back_order_sum: i64,
    pub incoming_orders: Vec<Order>,
    pub requested_orders: Vec<Order>,
    pub placed_order: Order,
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
