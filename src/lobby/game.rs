use std::{collections::BTreeMap, f64::consts::E, sync::Arc};

use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{
    entities::{Flow, GameState, Lobby, Order, User, UserState},
    error::AppError,
    websockets::EventMessages,
    LobbyState, State,
};

use super::lobby::send_broadcast_msg;

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct GameUpdate {
    pub recipient: Uuid,
    pub user_state: UserState,
    pub player_states: BTreeMap<Uuid, UserState>,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct UserEndRound {
    pub placed_order: Order,
}

pub async fn process_user_round_end_message(
    game_id: Uuid,
    player: Uuid,
    mut msg: UserEndRound,
    state: Arc<State>,
) -> Result<(), AppError> {
    //TODO: rewrite to if with early exit
    match state.clone().lobbies.write() {
        Ok(mut wg) => match wg.get_mut(&game_id) {
            Some(lobby_state) => {
                if msg.placed_order.value != 0 {
                    msg.placed_order.cost = msg.placed_order.value
                        * lobby_state.round_state.settings.resource_price
                        + lobby_state.round_state.settings.fix_order_cost;
                } else {
                    msg.placed_order.cost = 0
                }

                lobby_state
                    .round_state
                    .round_orders
                    .insert(player, msg.placed_order.clone());

                match lobby_state.round_state.users_states.get_mut(&player) {
                    Some(user_state) => {
                        if let Some(io) = user_state.incoming_orders.pop() {
                            user_state.magazine_state += io.value;
                        } else {
                            return Err(AppError::InternalServerError(
                                "expected a incoming order".to_string(),
                            ));
                        }

                        if let Some(ro) = user_state.requested_orders.pop() {
                            let mut send_order_val = 0;

                            if user_state.back_order_sum > user_state.magazine_state {
                                send_order_val = user_state.magazine_state;
                                user_state.back_order_sum -= user_state.magazine_state;
                                user_state.magazine_state = 0;
                            } else if user_state.back_order_sum > 0 {
                                user_state.magazine_state -= user_state.back_order_sum;
                                send_order_val = user_state.back_order_sum;
                                user_state.back_order_sum = 0;
                            }

                            if user_state.magazine_state > ro.value {
                                user_state.magazine_state -= ro.value;
                                send_order_val += ro.value;
                            } else {
                                let diff = ro.value - user_state.magazine_state;
                                user_state.magazine_state = 0;
                                user_state.back_order_sum += diff;
                                send_order_val += diff;
                            }

                            let recipient = match lobby_state.round_state.flow.flow.get(&player) {
                                Some(i) => i,
                                None => {
                                    return Err(AppError::InternalServerError(
                                        "incorrect flow".to_string(),
                                    ))
                                }
                            };

                            let send_order_val_cost = send_order_val
                                * lobby_state.round_state.settings.resource_price
                                + lobby_state.round_state.settings.fix_order_cost;

                            let send_order = Order {
                                recipient: *recipient,
                                sender: player,
                                value: send_order_val,
                                cost: send_order_val_cost,
                            };
                        } else {
                            return Err(AppError::InternalServerError(
                                "expected a incoming order".to_string(),
                            ));
                        }

                        user_state.money -= msg.placed_order.cost;
                        user_state.placed_order = msg.placed_order;

                        user_state.money -= user_state.magazine_state
                            * lobby_state.round_state.settings.magazine_cost;
                    }
                    None => {
                        return Err(AppError::InternalServerError(
                            "expected a user state".to_string(),
                        ))
                    }
                }

                lobby_state.round_state.players_finished += 1;

                if lobby_state.round_state.players_finished == lobby_state.round_state.players {
                    finish_round(game_id, &lobby_state).await?;
                }
            }
            None => {
                return Err(AppError::InternalServerError(
                    "expected a lobby state".to_string(),
                ))
            }
        },
        Err(e) => return Err(AppError::InternalServerError(e.to_string())),
    }

    Ok(())
}

pub async fn process_game_events() {}

pub async fn finish_round(game_id: Uuid, lobby_state: &LobbyState) -> Result<(), AppError> {
    let last_player = match lobby_state.round_state.flow.last() {
        Some(l) => l,
        None => {
            return Err(AppError::InternalServerError(
                "expected last player".to_string(),
            ))
        }
    };

    let next_demand = generate_demand(lobby_state);
    let next_demand_cost = lobby_state.round_state.settings.resource_price * next_demand
        + lobby_state.round_state.settings.fix_order_cost;
    let generated_order = Order {
        recipient: *last_player,
        sender: Uuid::nil(),
        value: next_demand,
        cost: next_demand_cost,
    };

    Ok(())
}

pub async fn new_round() {}

pub async fn start_game(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    lobby: Lobby,
    players: Vec<User>,
    state: &Arc<State>,
) -> Result<(), AppError> {
    let init_orders: BTreeMap<Uuid, Order> = BTreeMap::new();
    let mut init_players_states: BTreeMap<Uuid, UserState> = BTreeMap::new();
    let players_count = players.len() as i64;

    for player in &players {
        let user_state = UserState {
            user_id: player.id,
            money: lobby.settings.start_money,
            magazine_state: lobby.settings.start_magazine,
            performance: 0, //TODO, fill with performance
            incoming_orders: lobby.settings.start_order_queue.clone(),
            requested_orders: lobby.settings.start_order_queue.clone(),
            back_order_sum: 0,
            placed_order: Order::default(),
        };

        init_players_states.insert(player.id, user_state);
    }

    let flow = redistribute_flow(&players)?;
    let demand = match lobby.settings.demand_style {
        crate::entities::DemandStyle::Default => 10,
        crate::entities::DemandStyle::Linear { start, increase: _ } => start,
        crate::entities::DemandStyle::Multiplication { start, increase: _ } => start,
        crate::entities::DemandStyle::Exponential {
            start,
            power: _,
            modulator: _,
        } => start,
    };

    sqlx::query_as!(GameState,
        // language=PostgreSQL
        r#"insert into "game_state" (round, user_states, round_orders, send_orders, flow, demand) values ($1, $2, $3, $4, $5, $6) returning id, round, user_states as "user_states: sqlx::types::Json<BTreeMap<Uuid, UserState>>", round_orders as "round_orders: sqlx::types::Json<BTreeMap<Uuid, Order>>", flow as "flow: sqlx::types::Json<Flow>", demand"#,
        0,
        sqlx::types::Json(&init_players_states) as _,
        sqlx::types::Json(init_orders) as _,
        sqlx::types::Json(flow) as _,
        demand,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        AppError::DbErr(e.to_string())
    })?;

    match state.lobbies.write() {
        Ok(mut wg) => match wg.get_mut(&id) {
            Some(lobby_state) => {
                lobby_state.started = true;
                lobby_state.round_state.round = 0;
                lobby_state.round_state.players = players_count;
                lobby_state.round_state.players_finished = 0;
                lobby_state.round_state.users_states = init_players_states.clone();
                lobby_state.round_state.settings = lobby.settings.0;
            }
            None => {
                return Err(AppError::InternalServerError(
                    "expected a lobby state".to_string(),
                ))
            }
        },
        Err(e) => return Err(AppError::InternalServerError(e.to_string())),
    }

    for player in &players {
        let user_state = match init_players_states.get(&player.id) {
            Some(state) => state,
            None => {
                return Err(AppError::InternalServerError(
                    "expected a lobby state".to_string(),
                ))
            }
        };

        let msg = GameUpdate {
            recipient: player.id,
            user_state: user_state.clone(),
            player_states: init_players_states.clone(),
        };

        send_broadcast_msg(state, id, EventMessages::GameStart(msg))?;
    }
    Ok(())
}

//TODO: enforce min players number?
fn redistribute_flow(players: &Vec<User>) -> Result<Flow, AppError> {
    let last_player = match players.last() {
        Some(p) => p,
        None => {
            return Err(AppError::InternalServerError(
                "expected last player for flow redistribute".to_string(),
            ))
        }
    };

    let first_player = match players.first() {
        Some(p) => p,
        None => {
            return Err(AppError::InternalServerError(
                "expected first player for flow redistribute".to_string(),
            ))
        }
    };

    let mut flow_map = BTreeMap::new();

    for i in 0..players.len() {
        let cur_player = players[i].id;
        let next_player = match players.get(i + 1) {
            Some(p) => p.id,
            None => Uuid::nil(),
        };

        flow_map.insert(cur_player, next_player);
    }

    Ok(Flow {
        last_player: last_player.id,
        first_player: first_player.id,
        flow: flow_map,
    })
}

fn generate_demand(lobby_state: &LobbyState) -> i64 {
    match lobby_state.round_state.settings.demand_style {
        crate::entities::DemandStyle::Default => {
            (lobby_state.round_state.demand as f64 * 1.5) as i64
        }
        crate::entities::DemandStyle::Linear { start: _, increase } => {
            lobby_state.round_state.demand + increase
        }
        crate::entities::DemandStyle::Multiplication { start: _, increase } => {
            lobby_state.round_state.demand * increase
        }
        crate::entities::DemandStyle::Exponential {
            start: _,
            power,
            modulator,
        } => lobby_state.round_state.demand * (modulator * (E.powi(power as i32)) as i64),
    }
}
