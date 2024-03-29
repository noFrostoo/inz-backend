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
        ActionTarget, EventAction, EventCondition, Flow, GameState, GeneratedOrderStyle, Lobby,
        MetBy, Order, Resource, Settings, User, UserState,
    },
    error::AppError,
    websockets::EventMessages,
    LobbyState, RoundState, State,
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
    tracing::debug!("process_user_round_end_message: {}", game_id);
    let mut round_state;

    match state.lobbies.write().await.get(&game_id) {
        Some(lb) => {
            round_state = lb.round_state.clone();
        }
        None => {
            return Err(AppError::InternalServerError(
                "expected a lobby state".to_string(),
            ))
        }
    }

    tracing::debug!(
        "got lobby_state for process_user_round_end_message: {}",
        game_id
    );

    let player_class;
    match round_state.player_classes.get(&player) {
        Some(c) => player_class = c,
        None => {
            return Err(AppError::BadRequest(
                "class for player not found".to_string(),
            ))
        }
    }

    let resource_price = match round_state.settings.resource_price.get(&player_class) {
        Some(c) => c,
        None => {
            return Err(AppError::BadRequest(
                "player-resource price not found".to_string(),
            ))
        }
    };

    let fix_order_cost = match round_state.settings.fix_order_cost.get(&player_class) {
        Some(c) => c,
        None => {
            return Err(AppError::BadRequest(
                "player-fix_order_cost not found".to_string(),
            ))
        }
    };

    let magazine_cost = match round_state.settings.magazine_cost.get(&player_class) {
        Some(c) => c,
        None => {
            return Err(AppError::BadRequest(
                "player-magazine_cost not found".to_string(),
            ))
        }
    };

    tracing::debug!("about to process orders: {}", game_id);

    match round_state.users_states.get_mut(&player) {
        Some(user_state) => {
            if msg.placed_order.cost > user_state.money {
                //TODO: send msg here ?
                return Err(AppError::BadOrder(
                    "not enough money for placed order".to_string(),
                ));
            }

            tracing::debug!("sending ack: {}", game_id);
            send_broadcast_msg(&state, game_id, EventMessages::Ack(player)).await?;

            tracing::debug!("send ack, processing orders");
            //for multiple recipients
            msg.placed_order.recipient = player;
            msg.placed_order.sender = round_state.flow.get_sender(&player)?;

            user_state.money -= msg.placed_order.cost;
            user_state.spent_money += msg.placed_order.cost;
            user_state.placed_order = msg.placed_order.clone();

            let magazine_cost = user_state.magazine_state * magazine_cost;
            user_state.money -= magazine_cost;
            user_state.spent_money += magazine_cost;

            round_state
                .round_orders
                .insert(player, msg.placed_order.clone());

            tracing::debug!("about to process incoming orders: {}", game_id);
            if let Some(io) = user_state.incoming_orders.pop() {
                user_state.magazine_state += io.value;
                user_state.received_order = io;
            } else {
                return Err(AppError::InternalServerError(
                    "expected a incoming order".to_string(),
                ));
            }

            tracing::debug!("about to process requested orders: {}", game_id);
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
                    recipient: round_state.flow.get_recipient(&player)?,
                    sender: player,
                    value: send_order_val,
                    cost: send_order_val_cost,
                };

                round_state.send_orders.insert(player, send_order.clone());

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

    round_state.players_finished += 1;

    match state.lobbies.write().await.get_mut(&game_id) {
        Some(lobby_state) => {
            lobby_state.round_state = round_state.clone();
        }
        None => {
            return Err(AppError::InternalServerError(
                "expected a lobby state".to_string(),
            ))
        }
    }

    if round_state.players_finished == round_state.players {
        tracing::debug!("finishing rounds: {}", game_id);
        finish_round(game_id, &mut round_state, &state, db).await?;
    }

    Ok(())
}

//TODO: FIXME: REFACTOR TO USE MACROS!!
pub async fn process_game_events(
    game_id: Uuid,
    round_state: &mut RoundState,
    state: &Arc<State>,
    db: &PgPool,
) -> Result<(), AppError> {
    let lobby = get_lobby(game_id, db).await?;
    tracing::debug!("processing events, count: {}", lobby.events.0.events.len());
    for event in lobby.events.0.events {
        tracing::debug!("processing event: {}", event.name);
        let (cond_met, targets) = evaluate_cond(&event, round_state, db, game_id).await?;

        if !cond_met {
            continue;
        }

        for action in event.actions {
            match action {
                EventAction::ShowMessage { message, target } => {
                    execute_pop_up_action(target, &targets, state, game_id, message).await?
                }
                EventAction::ChangeSettings { new_settings } => {
                    execute_settings_change(db, round_state, new_settings, state, game_id).await?
                }
                EventAction::AddResource {
                    resource,
                    target,
                    value,
                } => {
                    execute_resource_action(
                        target,
                        &targets,
                        round_state,
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
    round_state: &mut RoundState,
    resource: Resource,
    value: i64,
    state: &Arc<State>,
    game_id: Uuid,
) -> Result<(), AppError> {
    Ok(match target {
        ActionTarget::EventTarget => {
            for u_id in players_targets {
                let mut player_state = match round_state.users_states.get_mut(u_id) {
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
            for (_, player_state) in &mut round_state.users_states {
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
    round_state: &mut RoundState,
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

    round_state.settings = new_settings.clone();

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
    round_state: &mut RoundState,
    db: &sqlx::Pool<Postgres>,
    game_id: Uuid,
) -> Result<(bool, Vec<Uuid>), AppError> {
    let (met_by, players_targets) = match event.condition.clone() {
        EventCondition::RoundMet { round } => evaluate_round_cond(round_state, round),
        EventCondition::ValueExceed {
            resource,
            met_by,
            value,
        } => match resource {
            Resource::Money => evaluate_value_exceed(|us| us.money, met_by, round_state, value),
            Resource::MagazineState => {
                evaluate_value_exceed(|us| us.magazine_state, met_by, round_state, value)
            }
            Resource::Performance => {
                evaluate_value_exceed(|us| us.performance, met_by, round_state, value)
            }
            Resource::BackOrderValue => {
                evaluate_value_exceed(|us| us.back_order_sum, met_by, round_state, value)
            }
        },
        EventCondition::SingleChange { resource, value } => {
            let last_state = sqlx::query_as!(GameState,
                r#"
                    select id, round, user_states as "user_states: sqlx::types::Json<BTreeMap<Uuid, UserState>>", round_orders as "round_orders: sqlx::types::Json<BTreeMap<Uuid, Order>>", flow as "flow: sqlx::types::Json<Flow>", demand, supply, send_orders as "send_orders: sqlx::types::Json<BTreeMap<Uuid, Order>>", players_classes as "players_classes: sqlx::types::Json<BTreeMap<Uuid, u32>>", game_id
                    from "game_state"
                    where game_id = $1 and round = $2"#,
                game_id,
                round_state.round - 1
            ).fetch_one(db)
            .await
            .map_err(|e| {
                AppError::DbErr(e.to_string())
            })?;

            match resource {
                Resource::Money => {
                    evaluate_single_change(|us| us.money, round_state, last_state, value)
                }
                Resource::MagazineState => {
                    evaluate_single_change(|us| us.magazine_state, round_state, last_state, value)
                }
                Resource::Performance => {
                    evaluate_single_change(|us| us.performance, round_state, last_state, value)
                }
                Resource::BackOrderValue => {
                    evaluate_single_change(|us| us.back_order_sum, round_state, last_state, value)
                }
            }
        }
    };
    Ok((met_by, players_targets))
}

fn evaluate_round_cond(round_state: &mut RoundState, round: i64) -> (bool, Vec<Uuid>) {
    tracing::debug!("assesing round cond, {}, {}", round_state.round, round);
    let mut players_id = Vec::new();
    if round_state.round == round {
        tracing::debug!("round event evaluation");
        for (u_id, _) in &round_state.users_states {
            players_id.push(*u_id);
        }
    }
    tracing::debug!("returning: {} {:?}", players_id.len() != 0, players_id);
    (players_id.len() != 0, players_id)
}

//TODO: refactor name
fn evaluate_value_exceed(
    extractor: fn(&UserState) -> i64,
    met_by: MetBy,
    round_state: &mut RoundState,
    value: i64,
) -> (bool, Vec<Uuid>) {
    let mut recipients = Vec::new();
    let met = match met_by {
        MetBy::SinglePlayer => {
            for (u_id, user_state) in &round_state.users_states {
                if extractor(user_state) > value {
                    recipients.push(*u_id)
                }
            }
            recipients.len() != 0
        }
        MetBy::Average => {
            let mut sum = 0;
            for (u_id, user_state) in &round_state.users_states {
                sum += extractor(user_state);
                recipients.push(*u_id);
            }
            (sum / round_state.users_states.len() as i64) > value
        }
        MetBy::AllPlayers => {
            let mut val_met = true;
            for (u_id, user_state) in &round_state.users_states {
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
    round_state: &mut RoundState,
    last_state: GameState,
    value: i64,
) -> (bool, Vec<Uuid>) {
    let mut recipients = Vec::new();

    for (u_id, user_state) in &round_state.users_states {
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
    round_state: &mut RoundState,
    state: &Arc<State>,
    db: &PgPool,
) -> Result<(), AppError> {
    send_broadcast_msg(state, game_id, EventMessages::RoundEnd).await?;

    tracing::debug!("finishing round, generating demand");
    let next_demand = generate_demand(round_state.demand, &round_state.settings.demand_style);
    let next_demand_cost = round_state.settings.resource_basic_price * next_demand;
    let generated_order = Order {
        recipient: Uuid::nil(),
        sender: round_state.flow.last_player,
        value: next_demand,
        cost: next_demand_cost,
    };

    round_state
        .round_orders
        .insert(Uuid::nil(), generated_order);

    tracing::debug!("round orders {:?}", round_state.round_orders);

    let mut generated_order_supply =
        match round_state.round_orders.get(&round_state.flow.first_player) {
            Some(o) => o.clone(),
            None => {
                return Err(AppError::InternalServerError(
                    "not found fist player order".to_string(),
                ))
            }
        };

    let next_supply = generate_demand(round_state.supply, &round_state.settings.supply_style);
    if next_supply < generated_order_supply.value {
        let next_supply_cost = round_state.settings.resource_basic_price * next_supply;
        generated_order_supply = Order {
            recipient: round_state.flow.first_player,
            sender: Uuid::nil(),
            value: next_supply,
            cost: next_supply_cost,
        };
    }

    round_state
        .send_orders
        .insert(Uuid::nil(), generated_order_supply);

    for (_, order) in &round_state.round_orders {
        if order.sender.is_nil() {
            continue;
        }

        tracing::debug!(
            "trying to push order for sender {}, recipient: {}",
            order.sender,
            order.recipient
        );
        match round_state.users_states.get_mut(&order.sender) {
            Some(us) => us.requested_orders.push(order.clone()),
            None => {
                return Err(AppError::InternalServerError(format!(
                    "not found placed recipient for order {:?}",
                    order
                )))
            }
        }
    }

    for (_, order) in &round_state.send_orders {
        if order.recipient.is_nil() {
            continue;
        }

        tracing::debug!("trying to push order for recipient {}", order.recipient);
        match round_state.users_states.get_mut(&order.recipient) {
            Some(us) => us.incoming_orders.push(order.clone()),
            None => {
                return Err(AppError::InternalServerError(format!(
                    "not found sent recipient for order {:?}",
                    order
                )))
            }
        }
    }

    round_state.round += 1;
    round_state.demand = next_demand;

    match state.lobbies.write().await.get_mut(&game_id) {
        Some(lobby_state) => {
            lobby_state.round_state = round_state.clone();
        }
        None => {
            return Err(AppError::InternalServerError(
                "expected a lobby state".to_string(),
            ))
        }
    }

    sqlx::query_as!(GameState,
        // language=PostgreSQL
        r#"insert into "game_state" 
        (round, user_states, round_orders, send_orders, players_classes, flow, demand, supply, game_id) 
        values ($1, $2, $3, $4, $5, $6, $7, $8, $9) 
        returning id, round, user_states as "user_states: sqlx::types::Json<BTreeMap<Uuid, UserState>>", round_orders as "round_orders: sqlx::types::Json<BTreeMap<Uuid, Order>>", flow as "flow: sqlx::types::Json<Flow>", demand, supply, send_orders as "send_orders: sqlx::types::Json<BTreeMap<Uuid, Order>>", players_classes as "players_classes: sqlx::types::Json<BTreeMap<Uuid, u32>>", game_id "#,
        round_state.round,
        sqlx::types::Json(&round_state.users_states) as _,
        sqlx::types::Json(&round_state.round_orders) as _,
        sqlx::types::Json(&round_state.send_orders) as _,
        sqlx::types::Json(&round_state.player_classes) as _,
        sqlx::types::Json(&round_state.flow) as _,
        round_state.demand,
        round_state.supply,
        game_id
    )
    .fetch_one(db)
    .await
    .map_err(|e| {
        AppError::DbErr(e.to_string())
    })?;

    let lobby = get_lobby(game_id, db).await?;
    if round_state.round == lobby.settings.max_rounds {
        finish_game(game_id, round_state, state, db).await?;
    } else {
        new_round(game_id, round_state, state, db).await?;
    }

    Ok(())
}

pub async fn new_round(
    game_id: Uuid,
    round_state: &mut RoundState,
    state: &Arc<State>,
    db: &PgPool,
) -> Result<(), AppError> {
    process_game_events(game_id, round_state, state, db).await?;

    let send_orders = round_state.send_orders.clone();
    let round_orders = round_state.round_orders.clone();

    round_state.players_finished = 0;
    round_state.round_orders.clear();
    round_state.send_orders.clear();

    match state.lobbies.write().await.get_mut(&game_id) {
        Some(lobby_state) => {
            lobby_state.round_state = round_state.clone();
        }
        None => {
            return Err(AppError::InternalServerError(
                "expected a lobby state".to_string(),
            ))
        }
    }

    let msg = GameUpdate {
        player_states: round_state.users_states.clone(),
        round: round_state.round,
        flow: round_state.flow.clone(),
        settings: round_state.settings.clone(),
        round_orders: round_orders,
        send_orders: send_orders,
        player_classes: round_state.player_classes.clone(),
    };

    send_broadcast_msg(state, game_id, EventMessages::RoundStart(msg)).await?;

    Ok(())
}

pub async fn finish_game(
    game_id: Uuid,
    round_state: &mut RoundState,
    state: &Arc<State>,
    db: &PgPool,
) -> Result<(), AppError> {
    let stats_types = vec![
        UserStatsType::Money,
        UserStatsType::MagazineState,
        UserStatsType::BackOrder,
        UserStatsType::PlacedOrder,
        UserStatsType::ReceivedOrder,
        UserStatsType::SpentMoney,
    ];

    let stats = get_player_stats(game_id, db, stats_types).await?;
    let msg = GameEnd {
        player_states: round_state.users_states.clone(),
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
            None => {
                return Err(AppError::BadRequest(
                    "Player not found start money".to_string(),
                ))
            }
        }

        let start_magazine;
        match lobby.settings.start_magazine.get(&player_class) {
            Some(c) => start_magazine = c,
            None => {
                return Err(AppError::BadRequest(
                    "Player not found start magazine".to_string(),
                ))
            }
        }

        let incoming_orders_values;
        match lobby.settings.incoming_start_queue.get(&player_class) {
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
        match lobby.settings.requested_start_queue.get(&player_class) {
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

    let supply = match &lobby.settings.supply_style {
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
        (round, user_states, round_orders, send_orders, flow, players_classes, demand, supply, game_id) 
        values ($1, $2, $3, $4, $5, $6, $7, $8, $9) 
        returning id, round, user_states as "user_states: sqlx::types::Json<BTreeMap<Uuid, UserState>>", round_orders as "round_orders: sqlx::types::Json<BTreeMap<Uuid, Order>>", flow as "flow: sqlx::types::Json<Flow>", demand, supply, send_orders as "send_orders: sqlx::types::Json<BTreeMap<Uuid, Order>>", players_classes as "players_classes: sqlx::types::Json<BTreeMap<Uuid, u32>>",game_id "#,
        0,
        sqlx::types::Json(&init_players_states) as _,
        sqlx::types::Json(init_orders) as _,
        sqlx::types::Json(init_send_order) as _,
        sqlx::types::Json(&flow) as _,
        sqlx::types::Json(&players_classes) as _,
        demand,
        supply,
        id
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        AppError::DbErr(e.to_string())
    })?;

    let msg;

    tracing::debug!(
        "initing game, players_count, {} players; {:?}",
        players_count,
        players
    );

    match state.lobbies.write().await.get_mut(&id) {
        Some(lobby_state) => {
            lobby_state.started = true;
            lobby_state.round_state.flow = flow.clone();
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

fn generate_demand(last_demand: i64, demand_style: &GeneratedOrderStyle) -> i64 {
    match &demand_style {
        crate::entities::GeneratedOrderStyle::Default => (last_demand as f64 * 1.5) as i64,
        crate::entities::GeneratedOrderStyle::Linear { start: _, increase } => {
            last_demand + increase
        }
        crate::entities::GeneratedOrderStyle::Multiplication { start: _, increase } => {
            last_demand * increase
        }
        crate::entities::GeneratedOrderStyle::Exponential {
            start: _,
            power,
            modulator,
        } => last_demand * (modulator * (E.powi(*power as i32)) as i64),
        crate::entities::GeneratedOrderStyle::List { list: demand } => {
            let index = match demand.iter().position(|&r| r == last_demand) {
                Some(i) => i,
                None => demand.len() - 1,
            };

            match demand.get(index) {
                Some(d) => *d,
                None => last_demand,
            }
        }
    }
}
