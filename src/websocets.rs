use std::sync::Arc;

use axum_typed_websockets::WebSocket;
use serde::{Serialize, Deserialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{lobby::{LobbyUserUpdate, LobbyUserUpdateResponse}, State, auth::Auth, entities::User, error::AppError};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventMessages {
    NewUserConnected,
    SettingChanged,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerMessage {
    LobbyUserUpdate(LobbyUserUpdate),
    Error(AppError)
}


#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientMessage {
    Auth{user_id: Uuid, lobby_id: Uuid},
    LobbyUserUpdateResponse(LobbyUserUpdateResponse),
    Error(String)//TODO: 
}



pub async fn process_message(mut socket: WebSocket<ServerMessage, ClientMessage>, mut state: Arc<State>, ref db: PgPool, user: User) {
    
    

    let tx;
    let rx;

    match state.lobbies.read() {
        Ok(lobbies) => {
            match lobbies.get(&lobby) {
                Some(lobby_state) => {
                    tx = lobby_state.sender.clone();
                    rx = tx.subscribe();
                },
                None => todo!(),
            }
        },
        Err(_) => todo!(),
    }

    //let mut send_task = tokio::spawn(future);


    
}

pub fn send_update() {

}

pub fn create_connection() {

}

