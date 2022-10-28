use std::{sync::Arc, collections::BTreeMap};

use sqlx::{Transaction, Postgres};
use uuid::Uuid;

use crate::{State, entities::{User, Lobby, UserState, GameState, Order}, error::AppError};

use super::lobby::send_broadcast_msg;

pub struct GameUpdate {
    pub recipient: Uuid,
    pub user_state: UserState,
    pub player_states: BTreeMap<Uuid, UserState>
}


pub async fn process_user_round_end_message() {

}

pub async fn process_game_events() {

}

pub async fn finish_round() {

}

pub async fn new_round() {
    
}

pub async fn start_game(tx: &mut Transaction<'_, Postgres>, id: Uuid, lobby: Lobby, players: Vec<User>,state: &Arc<State>) -> Result<(), AppError>{
    let init_orders: BTreeMap<Uuid, Order> = BTreeMap::new();
    let mut init_players_states: BTreeMap<Uuid, UserState> = BTreeMap::new();
    
    for player in players{
        let user_state = UserState{ 
            user_id: player.id,
            money: lobby.settings.start_money,
            magazine_state: lobby.settings.start_magazine,
            performance: 0, //TODO, fill with performance 
            back_order: Vec::new(),
            current_order: lobby.settings.start_order.clone(),
            user_order: Order::default()
        };

        init_players_states.insert(player.id, user_state);
    }
    
    let init_game_state = sqlx::query_as!(GameState,
        // language=PostgreSQL
        r#"insert into "game_state" (round, user_states, round_orders) values ($1, $2, $3) returning id, round, user_states as "user_states: sqlx::types::Json<BTreeMap<Uuid, UserState>>", round_orders as "round_orders: sqlx::types::Json<BTreeMap<Uuid, Order>>""#,
        0,
        sqlx::types::Json(init_players_states) as _,
        sqlx::types::Json(init_orders) as _
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        AppError::DbErr(e.to_string())
    })?;

    match state.lobbies.write() {
        Ok(wg) => match wg.get(&id) {
            Some(lobby_state) => {
                lobby_state.started = true;
                lobby_state.round_state.round = 0;
                lobby_state.round_state.players = players.len() as i64;
                lobby_state.round_state.players_finished = 0;
            },
            None => return Err(AppError::InternalServerError("expected a lobby state".to_string())),
        },
        Err(e) => return Err(AppError::InternalServerError(e.to_string())),
    }

    for player in players {
        let msg = GameUpdate{ 
            recipient: todo!(), 
            user_state: , 
            player_states: init_players_states
        };
        send_broadcast_msg(state, id, msg);
    }
    Ok(())
}