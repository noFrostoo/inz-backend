use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use axum::{extract::Path, Extension, Json};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    auth::Auth,
    entities::{Flow, GameState, Order, UserState},
    error::AppError,
    State, user,
};

use super::lobby::{get_lobby, get_lobby_users_transaction, get_lobby_users};

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
pub struct GameStats {
    required_stats: Vec<()>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
pub struct UserStats {
    required_stats: Vec<UserStatsType>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
pub enum UserStatsType {
    Money,
    Performance,
    MagazineState,
    PlacedOrder,
    ReceivedOrder,
    BackOrder,
    SpentMoney,
}

pub async fn game_stats(
    Extension(ref db): Extension<PgPool>,
    Extension(state): Extension<Arc<State>>,
    auth: Auth,
) -> Result<Json<HashMap<String, Vec<i64>>>, AppError> {
    Ok(Json(HashMap::new()))
}

pub async fn players_stats(
    Path(game_id): Path<Uuid>,
    Extension(ref db): Extension<PgPool>,
    Json(stats_types): Json<UserStats>,
    auth: Auth,
) -> Result<Json<HashMap<String, HashMap<Uuid, Vec<i64>>>>, AppError> {
    let lobby = get_lobby(game_id, db).await?;
    if !lobby.started {
        return Err(AppError::GameNotStarted("can't get stats for game not started".to_string()));
    }
    let users = get_lobby_users(game_id, db).await?;
    if let None = users.iter().position(|x| x.id == auth.user_id) {
        return Err(AppError::Unauthorized("not connected to the game".to_string()));
    }

    Ok(Json(get_player_stats(game_id, db, stats_types).await?))
}

pub async fn get_player_stats(game_id: Uuid, db: &PgPool, stats_types: UserStats) -> Result<HashMap<String, HashMap<Uuid, Vec<i64>>>, AppError> {
    let games_states = sqlx::query_as!(GameState,
        r#"
        select id, round, user_states as "user_states: sqlx::types::Json<BTreeMap<Uuid, UserState>>", round_orders as "round_orders: sqlx::types::Json<BTreeMap<Uuid, Order>>", flow as "flow: sqlx::types::Json<Flow>", demand, send_orders as "send_orders: sqlx::types::Json<BTreeMap<Uuid, Order>>", game_id
        from "game_state"
        where game_id = $1
        order by round"#,
        game_id
    ).fetch_all(db)
    .await
    .map_err(|e| AppError::DbErr(e.to_string()))?;
    let mut stats = HashMap::new();
    for stats_type in stats_types.required_stats {
        match stats_type {
            UserStatsType::Money => {
                get_stats_for_type(|u| u.money, "money".to_string(), &games_states, &mut stats)
            }
            UserStatsType::Performance => get_stats_for_type(
                |u| u.performance,
                "performance".to_string(),
                &games_states,
                &mut stats,
            ),
            UserStatsType::MagazineState => get_stats_for_type(
                |u| u.magazine_state,
                "magazine_state".to_string(),
                &games_states,
                &mut stats,
            ),
            UserStatsType::PlacedOrder => get_stats_for_type(
                |u| u.placed_order.cost,
                "placed_order".to_string(),
                &games_states,
                &mut stats,
            ),
            UserStatsType::ReceivedOrder => get_stats_for_type(
                |u| u.received_order.cost,
                "received_order".to_string(),
                &games_states,
                &mut stats,
            ),
            UserStatsType::BackOrder => get_stats_for_type(
                |u| u.back_order_sum,
                "back_order".to_string(),
                &games_states,
                &mut stats,
            ),
            UserStatsType::SpentMoney => get_stats_for_type(
                |u| u.spent_money,
                "spent_money".to_string(),
                &games_states,
                &mut stats,
            ),
        }
    }
    Ok(stats)
}

fn get_stats_for_type(
    extractor: fn(&UserState) -> i64,
    stat_name: String,
    games_states: &Vec<GameState>,
    stats: &mut HashMap<String, HashMap<Uuid, Vec<i64>>>,
) {
    let mut type_stats: HashMap<Uuid, Vec<i64>> = HashMap::new();
    for round_state in games_states {
        for (user_id, user_state) in &round_state.user_states.0 {
            if let Some(data) = type_stats.get_mut(user_id) {
                data.push(extractor(user_state));
            } else {
                type_stats.insert(*user_id, vec![extractor(user_state)]);
            }
        }
    }
    stats.insert(stat_name, type_stats);
}
