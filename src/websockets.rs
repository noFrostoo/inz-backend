use std::{borrow::BorrowMut, sync::Arc};

use axum_typed_websockets::{Message, WebSocket};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::{
    auth::Auth,
    error::AppError,
    lobby::lobby::{LobbyUpdate, LobbyUserUpdate},
    user::user::get_user,
    State,
};
//TODO: learn more about it
use futures::{
    sink::SinkExt,
    stream::{SplitSink, StreamExt},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventMessages {
    NewUserConnected(LobbyUserUpdate),
    LobbyUpdate(LobbyUpdate),
    UserDisconnected(LobbyUserUpdate),
    GameStart(LobbyUpdate),
    RoundEnd,
    RoundStart,
    GameEvent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerMessage {
    NewUserConnected(LobbyUserUpdate),
    UserDisconnected(LobbyUserUpdate),
    LobbyUpdate(LobbyUpdate),
    Error(AppError),
    RoundEnd,
    RoundStart,
    GameEvent,
    GameStart(LobbyUpdate),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientMessage {
    Error(String), //TODO:
    RoundEnd,
}

async fn send_err(
    socket: &mut SplitSink<WebSocket<ServerMessage, ClientMessage>, Message<ServerMessage>>,
    err: AppError,
) {
    if let Err(e) = socket.send(Message::Item(ServerMessage::Error(err))).await {
        tracing::error!("error sending error message {}", e.to_string())
    }
}

pub async fn process_message(
    socket: WebSocket<ServerMessage, ClientMessage>,
    state: Arc<State>,
    ref db: PgPool,
    auth: Auth,
) {
    let (mut sender, mut receiver) = socket.split();
    let tx;
    let mut rx;

    let user = match get_user(auth.user_id, db).await {
        Ok(u) => u,
        Err(e) => {
            send_err(
                sender.borrow_mut(),
                AppError::InternalServerError(format!("error looking for user: {}", e)),
            )
            .await;
            return;
        }
    };

    let game_id = match user.game_id {
        Some(g) => g,
        None => {
            send_err(
                sender.borrow_mut(),
                AppError::InternalServerError("user not connected to a game".to_string()),
            )
            .await;
            return;
        }
    };

    match state.lobbies.read() {
        Ok(lobbies) => match lobbies.get(&game_id) {
            Some(lobby_state) => {
                tx = lobby_state.sender.clone();
                rx = tx.subscribe();
            }
            None => todo!(),
        },
        Err(_) => todo!(),
    }

    let mut send_task = tokio::spawn(async move {
        while let Ok(event_msg) = rx.recv().await {
            let message = match event_msg {
                EventMessages::NewUserConnected(l) => ServerMessage::NewUserConnected(l),
                EventMessages::LobbyUpdate(u) => ServerMessage::LobbyUpdate(u),
                EventMessages::UserDisconnected(l) => ServerMessage::UserDisconnected(l),
                EventMessages::GameStart(u) => ServerMessage::GameStart(u),
                EventMessages::RoundEnd => todo!(),
                EventMessages::RoundStart => todo!(),
                EventMessages::GameEvent => todo!(),
            };

            if let Err(e) = sender.send(Message::Item(message)).await {
                send_err(
                    sender.borrow_mut(),
                    AppError::InternalServerError(e.to_string()),
                )
                .await;
            }
        }
    });

    let mut recv_task = tokio::spawn(async move {
        while let Some(result_msg) = receiver.next().await {
            match result_msg {
                Ok(_) => todo!(),
                Err(e) => {
                    tracing::error!("error while receiving client  {}", e.to_string())
                }
            }
        }
    });

    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    };

    //TODO: how to do disconnect ????
}