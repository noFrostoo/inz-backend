use std::sync::Arc;

use axum_typed_websockets::WebSocket;
use serde::{Serialize, Deserialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{lobby::{LobbyUserUpdate, LobbyUserUpdateResponse}, State};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventMessages {
    LobbyUserUpdate,
    LobbyUserUpdateResponse
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerMessage {
    LobbyUserUpdate(LobbyUserUpdate),
}


#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientMessage {
    Auth{user_id: Uuid, lobby_id: Uuid},
    LobbyUserUpdateResponse(LobbyUserUpdateResponse)
}



pub async fn process_message(mut socket: WebSocket<ServerMessage, ClientMessage>, mut state: Arc<State>, ref db: PgPool) {
    let user: Uuid;
    let lobby: Uuid;
    
    if let Some(msg) = socket.recv().await {
        //TODO max retries
        match msg {
            Ok(axum_typed_websockets::Message::Item(ClientMessage::Auth { user_id, lobby_id })) => {
                //TODO: auth user
                user = user_id;
                lobby = lobby_id;
            }
            Ok(_) => {return ;},
            Err(_) => todo!(),
        }
    } else {
        //TODO; send error
        return 
    }
    
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

