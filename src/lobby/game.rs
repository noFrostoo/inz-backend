use std::{
    collections::{BTreeMap, HashMap},
    f64::consts::E,
    sync::Arc,
};

use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    entities::{
        ActionTarget, EventAction, EventCondition, Flow, GameState, Lobby, MetBy, Order, Resource,
        Settings, User, UserState,
    },
    error::AppError,
    websockets::EventMessages,
    LobbyState, State,
};

use super::{
    lobby::{get_lobby, send_broadcast_msg},
    stats::{get_player_stats, UserStats, UserStatsType},
};

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct GameUpdate {
    pub player_states: BTreeMap<Uuid, UserState>,
    pub round: i64,
    pub flow: Flow,
    pub settings: Settings,
    pub round_orders: BTreeMap<Uuid, Order>,
    pub send_orders: BTreeMap<Uuid, Order>,
    pub player_classes: BTreeMap<Uuid, u32>,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct GameEnd {
    pub player_states: BTreeMap<Uuid, UserState>,
    pub stats: HashMap<String, HashMap<Uuid, Vec<i64>>>,
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
    db: &PgPool,
) -> Result<(), AppError> {
    //TODO: rewrite to if with early exit
    match state.clone().lobbies.write().await.get_mut(&game_id) {
        Some(mut lobby_state) => {
            let player_class;
            match lobby_state.round_state.player_classes.get(&player) {
                Some(c) => player_class = c,
                None => return Err(AppError::BadRequest("class for player not found".to_string())),
            }

            let resource_price = match lobby_state
                .round_state
                .settings
                .resource_price
                .get(&player_class)
            {
                Some(c) => c,
                None => {
                    return Err(AppError::BadRequest(
                        "player-resource price not found".to_string(),
                    ))
                }
            };

            let fix_order_cost = match lobby_state
                .round_state
                .settings
                .fix_order_cost
                .get(&player_class)
            {
                Some(c) => c,
                None => {
                    return Err(AppError::BadRequest(
                        "player-fix_order_cost not found".to_string(),
                    ))
                }
            };

            let magazine_cost = match lobby_state
                .round_state
                .settings
                .magazine_cost
                .get(&player_class)
            {
                Some(c) => c,
                None => {
                    return Err(AppError::BadRequest(
                        "player-magazine_cost not found".to_string(),
                    ))
                }
            };

            match lobby_state.round_state.users_states.get_mut(&player) {
                Some(user_state) => {
                    if msg.placed_order.cost > user_state.money {
                        //TODO: send msg here ?
                        return Err(AppError::BadOrder(
                            "not enough money for placed order".to_string(),
                        ));
                    }

                    send_broadcast_msg(&state, game_id, EventMessages::Ack(player)).await?;

                    //for multiple recipients
                    msg.placed_order.recipient = player;
                    msg.placed_order.sender = lobby_state.round_state.flow.get_sender(&player)?;

                    user_state.money -= msg.placed_order.cost;
                    user_state.spent_money += msg.placed_order.cost;
                    user_state.placed_order = msg.placed_order.clone();

                    let magazine_cost = user_state.magazine_state * magazine_cost;
                    user_state.money -= magazine_cost;
                    user_state.spent_money += magazine_cost;

                    lobby_state
                        .round_state
                        .round_orders
                        .insert(player, msg.placed_order.clone());

                    if let Some(io) = user_state.incoming_orders.pop() {
                        user_state.magazine_state += io.value;
                        user_state.received_order = io;
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

                        let send_order_val_cost = send_order_val * resource_price + fix_order_cost;

                        let send_order = Order {
                            recipient: lobby_state.round_state.flow.get_recipient(&player)?,
                            sender: player,
                            value: send_order_val,
                            cost: send_order_val_cost,
                        };

                        lobby_state
                            .round_state
                            .send_orders
                            .insert(player, send_order.clone());

                        user_state.sent_orders.push(send_order);
                    } else {
                        return Err(AppError::InternalServerError(
                            "expected a incoming order".to_string(),
                        ));
                    }
                }
                None => {
                    return Err(AppError::InternalServerError(
                        "expected a user state".to_string(),
                    ))
                }
            }

            lobby_state.round_state.players_finished += 1;

            if lobby_state.round_state.players_finished == lobby_state.round_state.players {
                finish_round(game_id, &mut lobby_state, &state, db).await?;
            }
        }
        None => {
            return Err(AppError::InternalServerError(
                "expected a lobby state".to_string(),
            ))
        }
    }

    Ok(())
}

//TODO: FIXME: REFACTOR TO USE MACROS!!
pub async fn process_game_events(
    game_id: Uuid,
    lobby_state: &mut LobbyState,
    state: &Arc<State>,
    db: &PgPool,
) -> Result<(), AppError> {
    let lobby = get_lobby(game_id, db).await?;
    for event in lobby.events.0.events {
        let (cond_met, targets) = evaluate_cond(&event, lobby_state, db, game_id).await?;

        if !cond_met {
            continue;
        }

        for action in event.actions {
            match action {
                EventAction::ShowMessage { message, target } => {
                    execute_pop_up_action(target, &targets, state, game_id, message).await?
                }
                EventAction::ChangeSettings { new_settings } => {
                    execute_settings_change(db, lobby_state, new_settings, state, game_id).await?
                }
                EventAction::AddResource {
                    resource,
                    target,
                    value,
                } => {
                    execute_resource_action(
                        target,
                        &targets,
                        lobby_state,
                        resource,
                        value,
                        state,
                        game_id,
                    )
                    .await?
                }
            }
        }
    }

    Ok(())
}

async fn execute_resource_action(
    target: ActionTarget,
    players_targets: &Vec<Uuid>,
    lobby_state: &mut LobbyState,
    resource: Resource,
    value: i64,
    state: &Arc<State>,
    game_id: Uuid,
) -> Result<(), AppError> {
    Ok(match target {
        ActionTarget::EventTarget => {
            for u_id in players_targets {
                let mut player_state = match lobby_state.round_state.users_states.get_mut(u_id) {
                    Some(p) => p,
                    None => {
                        return Err(AppError::InternalServerError(
                            "expected user state".to_string(),
                        ))
                    }
                };
                match resource {
                    Resource::Money => player_state.money += value,
                    Resource::MagazineState => player_state.magazine_state += value,
                    Resource::Performance => player_state.performance += value,
                    Resource::BackOrderValue => player_state.back_order_sum += value,
                }
                send_broadcast_msg(
                    state,
                    game_id,
                    EventMessages::GameEventResourceAddedUser(*u_id, resource.clone(), value),
                )
                .await?
            }
        }
        ActionTarget::AllPlayers => {
            for (_, player_state) in &mut lobby_state.round_state.users_states {
                match resource {
                    Resource::Money => player_state.money += value,
                    Resource::MagazineState => player_state.magazine_state += value,
                    Resource::Performance => player_state.performance += value,
                    Resource::BackOrderValue => player_state.back_order_sum += value,
                }
            }
            send_broadcast_msg(
                state,
                game_id,
                EventMessages::GameEventResourceAddedAll(resource, value),
            )
            .await?
        }
    })
}

async fn execute_settings_change(
    db: &sqlx::Pool<Postgres>,
    lobby_state: &mut LobbyState,
    new_settings: Settings,
    state: &Arc<State>,
    game_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query!(
        // language=PostgreSQL
        r#"update "lobby" set settings = $1 where id = $2"#,
        sqlx::types::Json(&new_settings) as _,
        game_id
    )
    .execute(db)
    .await
    .map_err(|e| AppError::DbErr(e.to_string()))?;
    lobby_state.round_state.settings = new_settings.clone();
    Ok(send_broadcast_msg(
        state,
        game_id,
        EventMessages::GameEventSettingsChange(new_settings),
    )
    .await?)
}

async fn execute_pop_up_action(
    target: ActionTarget,
    players_targets: &Vec<Uuid>,
    state: &Arc<State>,
    game_id: Uuid,
    message: String,
) -> Result<(), AppError> {
    Ok(match target {
        ActionTarget::EventTarget => {
            for player_id in players_targets {
                send_broadcast_msg(
                    state,
                    game_id,
                    EventMessages::GameEventPopUpUser(*player_id, message.clone()),
                )
                .await?
            }
        }
        ActionTarget::AllPlayers => {
            send_broadcast_msg(
                state,
                game_id,
                EventMessages::GameEventPopUpAll(message.clone()),
            )
            .await?
        }
    })
}

async fn evaluate_cond(
    event: &crate::entities::GameEvent,
    lobby_state: &mut LobbyState,
    db: &sqlx::Pool<Postgres>,
    game_id: Uuid,
) -> Result<(bool, Vec<Uuid>), AppError> {
    let (met_by, players_targets) = match event.condition.clone() {
        EventCondition::RoundMet { round } => evaluate_round_cond(lobby_state, round),
        EventCondition::ValueExceed {
            resource,
            met_by,
            value,
        } => match resource {
            Resource::Money => evaluate_value_exceed(|us| us.money, met_by, lobby_state, value),
            Resource::MagazineState => {
                evaluate_value_exceed(|us| us.magazine_state, met_by, lobby_state, value)
            }
            Resource::Performance => {
                evaluate_value_exceed(|us| us.performance, met_by, lobby_state, value)
            }
            Resource::BackOrderValue => {
                evaluate_value_exceed(|us| us.back_order_sum, met_by, lobby_state, value)
            }
        },
        EventCondition::SingleChange { resource, value } => {
            let last_state = sqlx::query_as!(GameState,
                r#"
                    select id, round, user_states as "user_states: sqlx::types::Json<BTreeMap<Uuid, UserState>>", round_orders as "round_orders: sqlx::types::Json<BTreeMap<Uuid, Order>>", flow as "flow: sqlx::types::Json<Flow>", demand, send_orders as "send_orders: sqlx::types::Json<BTreeMap<Uuid, Order>>", players_classes as "players_classes: sqlx::types::Json<BTreeMap<Uuid, u32>>", game_id
                    from "game_state"
                    where game_id = $1 and round = $2"#,
                game_id,
                lobby_state.round_state.round - 1
            ).fetch_one(db)
            .await
            .map_err(|e| {
                AppError::DbErr(e.to_string())
            })?;

            match resource {
                Resource::Money => {
                    evaluate_single_change(|us| us.money, lobby_state, last_state, value)
                }
                Resource::MagazineState => {
                    evaluate_single_change(|us| us.magazine_state, lobby_state, last_state, value)
                }
                Resource::Performance => {
                    evaluate_single_change(|us| us.performance, lobby_state, last_state, value)
                }
                Resource::BackOrderValue => {
                    evaluate_single_change(|us| us.back_order_sum, lobby_state, last_state, value)
                }
            }
        }
    };
    Ok((met_by, players_targets))
}

fn evaluate_round_cond(lobby_state: &mut LobbyState, round: i64) -> (bool, Vec<Uuid>) {
    let mut players_id = Vec::new();
    if lobby_state.round_state.round == round {
        for (u_id, _) in &lobby_state.round_state.users_states {
            players_id.push(*u_id);
        }
    }
    (players_id.len() != 0, players_id)
}

//TODO: refactor name
fn evaluate_value_exceed(
    extractor: fn(&UserState) -> i64,
    met_by: MetBy,
    lobby_state: &mut LobbyState,
    value: i64,
) -> (bool, Vec<Uuid>) {
    let mut recipients = Vec::new();
    let met = match met_by {
        MetBy::SinglePlayer => {
            for (u_id, user_state) in &lobby_state.round_state.users_states {
                if extractor(user_state) > value {
                    recipients.push(*u_id)
                }
            }
            recipients.len() != 0
        }
        MetBy::Average => {
            let mut sum = 0;
            for (u_id, user_state) in &lobby_state.round_state.users_states {
                sum += extractor(user_state);
                recipients.push(*u_id);
            }
            (sum / lobby_state.round_state.users_states.len() as i64) > value
        }
        MetBy::AllPlayers => {
            let mut val_met = true;
            for (u_id, user_state) in &lobby_state.round_state.users_states {
                if extractor(user_state) < value {
                    val_met = false;
                    break;
                } else {
                    recipients.push(*u_id);
                }
            }
            val_met
        }
    };
    (met, recipients)
}

fn evaluate_single_change(
    extractor: fn(&UserState) -> i64,
    lobby_state: &mut LobbyState,
    last_state: GameState,
    value: i64,
) -> (bool, Vec<Uuid>) {
    let mut recipients = Vec::new();

    for (u_id, user_state) in &lobby_state.round_state.users_states {
        let last_user_state = match last_state.user_states.get(u_id) {
            Some(s) => s,
            None => continue, //user disconnected probably
        };

        if (extractor(user_state) - extractor(last_user_state)).abs() > value {
            recipients.push(*u_id)
        }
    }

    (recipients.len() != 0, recipients)
}

pub async fn finish_round(
    game_id: Uuid,
    lobby_state: &mut LobbyState,
    state: &Arc<State>,
    db: &PgPool,
) -> Result<(), AppError> {
    send_broadcast_msg(state, game_id, EventMessages::RoundEnd).await?;

    let next_demand = generate_demand(lobby_state);
    let next_demand_cost = lobby_state.round_state.settings.resource_basic_price * next_demand;
    let generated_order = Order {
        recipient: lobby_state.round_state.flow.last_player,
        sender: Uuid::nil(),
        value: next_demand,
        cost: next_demand_cost,
    };

    lobby_state
        .round_state
        .round_orders
        .insert(Uuid::nil(), generated_order);

    let last_player_placed_order = match lobby_state
        .round_state
        .round_orders
        .get(&lobby_state.round_state.flow.last_player)
    {
        Some(o) => o.clone(),
        None => {
            return Err(AppError::InternalServerError(
                "not found fist player order".to_string(),
            ))
        }
    };

    lobby_state
        .round_state
        .send_orders
        .insert(Uuid::nil(), last_player_placed_order);

    for (_, order) in &lobby_state.round_state.round_orders {
        match lobby_state
            .round_state
            .users_states
            .get_mut(&order.recipient)
        {
            Some(us) => us.requested_orders.push(order.clone()),
            None => {
                return Err(AppError::InternalServerError(format!(
                    "not found recipient for order {:?}",
                    order
                )))
            }
        }
    }

    for (_, order) in &lobby_state.round_state.send_orders {
        match lobby_state
            .round_state
            .users_states
            .get_mut(&order.recipient)
        {
            Some(us) => us.incoming_orders.push(order.clone()),
            None => {
                return Err(AppError::InternalServerError(format!(
                    "not found recipient for order {:?}",
                    order
                )))
            }
        }
    }

    lobby_state.round_state.round += 1;
    lobby_state.round_state.demand = next_demand;

    sqlx::query_as!(GameState,
        // language=PostgreSQL
        r#"insert into "game_state" 
        (round, user_states, round_orders, send_orders, flow, demand, game_id) 
        values ($1, $2, $3, $4, $5, $6, $7) 
        returning id, round, user_states as "user_states: sqlx::types::Json<BTreeMap<Uuid, UserState>>", round_orders as "round_orders: sqlx::types::Json<BTreeMap<Uuid, Order>>", flow as "flow: sqlx::types::Json<Flow>", demand, send_orders as "send_orders: sqlx::types::Json<BTreeMap<Uuid, Order>>", players_classes as "players_classes: sqlx::types::Json<BTreeMap<Uuid, u32>>", game_id "#,
        lobby_state.round_state.round,
        sqlx::types::Json(&lobby_state.round_state.users_states) as _,
        sqlx::types::Json(&lobby_state.round_state.round_orders) as _,
        sqlx::types::Json(&lobby_state.round_state.send_orders) as _,
        sqlx::types::Json(&lobby_state.round_state.flow) as _,
        lobby_state.round_state.demand,
        game_id
    )
    .fetch_one(db)
    .await
    .map_err(|e| {
        AppError::DbErr(e.to_string())
    })?;

    let lobby = get_lobby(game_id, db).await?;
    if lobby_state.round_state.round == lobby.settings.max_rounds {
        finish_game(game_id, lobby_state, state, db).await?;
    } else {
        new_round(game_id, lobby_state, state, db).await?;
    }

    Ok(())
}

pub async fn new_round(
    game_id: Uuid,
    lobby_state: &mut LobbyState,
    state: &Arc<State>,
    db: &PgPool,
) -> Result<(), AppError> {
    process_game_events(game_id, lobby_state, state, db).await?;

    let msg = GameUpdate {
        player_states: lobby_state.round_state.users_states.clone(),
        round: 0,
        flow: lobby_state.round_state.flow.clone(),
        settings: lobby_state.round_state.settings.clone(),
        round_orders: lobby_state.round_state.round_orders.clone(),
        send_orders: lobby_state.round_state.send_orders.clone(),
        player_classes: lobby_state.round_state.player_classes.clone(),
    };

    send_broadcast_msg(state, game_id, EventMessages::RoundStart(msg)).await?;

    Ok(())
}

pub async fn finish_game(
    game_id: Uuid,
    lobby_state: &mut LobbyState,
    state: &Arc<State>,
    db: &PgPool,
) -> Result<(), AppError> {
    let stats_types = UserStats {
        required_stats: vec![
            UserStatsType::Money,
            UserStatsType::MagazineState,
            UserStatsType::BackOrder,
            UserStatsType::PlacedOrder,
            UserStatsType::Performance,
            UserStatsType::SpentMoney,
        ],
    };
    let stats = get_player_stats(game_id, db, stats_types).await?;
    let msg = GameEnd {
        player_states: lobby_state.round_state.users_states.clone(),
        stats: stats,
    };

    send_broadcast_msg(state, game_id, EventMessages::GameEnd(msg)).await?;
    Ok(())
}

pub async fn start_new_game(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    lobby: Lobby,
    players: Vec<User>,
    players_classes: BTreeMap<Uuid, u32>,
    state: &Arc<State>,
) -> Result<(), AppError> {
    let init_orders: BTreeMap<Uuid, Order> = BTreeMap::new();
    let mut init_players_states: BTreeMap<Uuid, UserState> = BTreeMap::new();
    let players_count = players.len() as i64;
    let init_send_order: BTreeMap<Uuid, Order> = BTreeMap::new();
    let flow = redistribute_flow(&players)?;

    for player in &players {
        let player_class;
        match players_classes.get(&player.id) {
            Some(c) => player_class = c,
            None => return Err(AppError::BadRequest("Player not found".to_string())),
        };

        let start_money;
        match lobby.settings.start_money.get(&player_class) {
            Some(c) => start_money = c,
            None => return Err(AppError::BadRequest("Player not found start money".to_string())),
        }

        let start_magazine;
        match lobby.settings.start_magazine.get(&player_class) {
            Some(c) => start_magazine = c,
            None => return Err(AppError::BadRequest("Player not found start magazine".to_string())),
        }

        let incoming_orders_values;
        match lobby.settings.start_order_queue.get(&player_class) {
            Some(c) => incoming_orders_values = c.clone(),
            None => return Err(AppError::BadRequest("Player not found".to_string())),
        }

        let sender_id = flow.get_sender(&player.id)?;

        let mut incoming_orders: Vec<Order> = Vec::new();
        for incoming_order in incoming_orders_values {
            incoming_orders.push(Order {
                recipient: player.id,
                sender: sender_id,
                value: incoming_order,
                cost: lobby.settings.resource_basic_price * incoming_order,
            })
        }

        let requested_orders_values;
        match lobby.settings.start_order_queue.get(&player_class) {
            Some(c) => requested_orders_values = c.clone(),
            None => return Err(AppError::BadRequest("Player not found".to_string())),
        }

        let recipient = match flow.flow.get(&player.id) {
            Some(p) => p,
            None => {
                return Err(AppError::BadRequest(
                    "bad flow, no player recipient found".to_string(),
                ))
            }
        };

        let mut requested_orders: Vec<Order> = Vec::new();
        for requested_order in requested_orders_values {
            requested_orders.push(Order {
                recipient: *recipient,
                sender: player.id,
                value: requested_order,
                cost: lobby.settings.resource_basic_price * requested_order,
            })
        }

        let user_state = UserState {
            user_id: player.id,
            money: *start_money,
            spent_money: 0,
            magazine_state: *start_magazine,
            performance: 0, //TODO, fill with performance
            incoming_orders: incoming_orders,
            requested_orders: requested_orders,
            back_order_sum: 0,
            placed_order: Order::default(),
            received_order: Order::default(),
            sent_orders: Vec::new(),
        };

        init_players_states.insert(player.id, user_state);
    }

    let demand = match &lobby.settings.demand_style {
        crate::entities::GeneratedOrderStyle::Default => 10,
        crate::entities::GeneratedOrderStyle::Linear { start, increase: _ } => *start,
        crate::entities::GeneratedOrderStyle::Multiplication { start, increase: _ } => *start,
        crate::entities::GeneratedOrderStyle::Exponential {
            start,
            power: _,
            modulator: _,
        } => *start,
        crate::entities::GeneratedOrderStyle::List { list: demand } => match demand.first() {
            Some(d) => *d,
            None => return Err(AppError::BadRequest("bad list demand".to_string())),
        },
    };

    sqlx::query_as!(GameState,
        // language=PostgreSQL
        r#"insert into "game_state" 
        (round, user_states, round_orders, send_orders, flow, players_classes, demand, game_id) 
        values ($1, $2, $3, $4, $5, $6, $7, $8) 
        returning id, round, user_states as "user_states: sqlx::types::Json<BTreeMap<Uuid, UserState>>", round_orders as "round_orders: sqlx::types::Json<BTreeMap<Uuid, Order>>", flow as "flow: sqlx::types::Json<Flow>", demand, send_orders as "send_orders: sqlx::types::Json<BTreeMap<Uuid, Order>>", players_classes as "players_classes: sqlx::types::Json<BTreeMap<Uuid, u32>>",game_id "#,
        0,
        sqlx::types::Json(&init_players_states) as _,
        sqlx::types::Json(init_orders) as _,
        sqlx::types::Json(init_send_order) as _,
        sqlx::types::Json(&flow) as _,
        sqlx::types::Json(&players_classes) as _,
        demand,
        id
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        AppError::DbErr(e.to_string())
    })?;


    let msg;

    match state.lobbies.write().await.get_mut(&id) {
        Some(lobby_state) => {
            lobby_state.started = true;
            lobby_state.round_state.round = 0;
            lobby_state.round_state.players = players_count;
            lobby_state.round_state.players_finished = 0;
            lobby_state.round_state.users_states = init_players_states.clone();
            lobby_state.round_state.settings = lobby.settings.0.clone();
            lobby_state.round_state.player_classes = players_classes;

            msg = GameUpdate {
                player_states: init_players_states.clone(),
                round: 0,
                flow: flow.clone(),
                settings: lobby.settings.0.clone(),
                round_orders: lobby_state.round_state.round_orders.clone(),
                send_orders: lobby_state.round_state.send_orders.clone(),
                player_classes: lobby_state.round_state.player_classes.clone(),
            };
        }
        None => {
            return Err(AppError::InternalServerError(
                "expected a lobby state".to_string(),
            ))
        }
    }

    send_broadcast_msg(state, id, EventMessages::GameStart(msg)).await?;
    Ok(())
}

//TODO: make sure owner is not in players
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

    print!("players {:?}", players);
    print!("flow {:?}", flow_map);

    Ok(Flow {
        last_player: last_player.id,
        first_player: first_player.id,
        flow: flow_map,
    })
}

fn generate_demand(lobby_state: &LobbyState) -> i64 {
    match &lobby_state.round_state.settings.demand_style {
        crate::entities::GeneratedOrderStyle::Default => {
            (lobby_state.round_state.demand as f64 * 1.5) as i64
        }
        crate::entities::GeneratedOrderStyle::Linear { start: _, increase } => {
            lobby_state.round_state.demand + increase
        }
        crate::entities::GeneratedOrderStyle::Multiplication { start: _, increase } => {
            lobby_state.round_state.demand * increase
        }
        crate::entities::GeneratedOrderStyle::Exponential {
            start: _,
            power,
            modulator,
        } => lobby_state.round_state.demand * (modulator * (E.powi(*power as i32)) as i64),
        crate::entities::GeneratedOrderStyle::List { list: demand } => {
            let index = match demand
                .iter()
                .position(|&r| r == lobby_state.round_state.demand)
            {
                Some(i) => i,
                None => demand.len() - 1,
            };

            match demand.get(index) {
                Some(d) => *d,
                None => lobby_state.round_state.demand,
            }
        }
    }
}
