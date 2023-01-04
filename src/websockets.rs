use std::{borrow::BorrowMut, sync::Arc, collections::BTreeMap};

use axum_typed_websockets::{Message, WebSocket};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    auth::{Auth, WebSocketAuth},
    entities::{Resource, Settings},
    error::AppError,
    lobby::{
        game::{process_user_round_end_message, GameEnd, GameUpdate, UserEndRound},
        lobby::{send_broadcast_msg, LobbyUpdate, LobbyUserUpdate, update_lobby_classes},
    },
    user::user::{get_user, disconnect_user},
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
    GameStart(GameUpdate),
    GameEventSettingsChange(Settings),
    GameEventPopUpUser(Uuid, String),
    GameEventPopUpAll(String),
    GameEventResourceAddedAll(Resource, i64),
    GameEventResourceAddedUser(Uuid, Resource, i64),
    RoundStart(GameUpdate),
    RoundEnd,
    KickAll,
    GameEnd(GameEnd),
    UpdateClasses(BTreeMap<Uuid, u32>),
    Ack(Uuid),
    ErrorUser(Uuid, AppError),
    Error(AppError),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerMessage {
    NewUserConnected(LobbyUserUpdate),
    UserDisconnected(LobbyUserUpdate),
    LobbyUpdate(LobbyUpdate),
    Error(AppError),
    RoundStart(GameUpdate),
    RoundFinish,
    GameStart(GameUpdate),
    GameEventSettingsChange(Settings),
    GameEventPopUp(String),
    GameEventResource(Resource, i64),
    KickAll,
    GameEnd(GameEnd),
    UpdateClasses(BTreeMap<Uuid, u32>),
    Ack,
    Ping(Vec<u8>),
    Pong(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientMessage {
    Error(String), //TODO:
    RoundEnd(UserEndRound),
    UpdateClasses(BTreeMap<Uuid, u32>)
}

async fn send_err(
    socket: &mut SplitSink<WebSocket<ServerMessage, ClientMessage>, Message<ServerMessage>>,
    err: AppError,
) {
    if let Err(e) = socket.send(Message::Item(ServerMessage::Error(err))).await {
        tracing::error!("error sending error message {}", e.to_string())
    }
}

pub async fn game_process(
    socket: WebSocket<ServerMessage, ClientMessage>,
    state: Arc<State>,
    db: PgPool,
    auth: WebSocketAuth,
) {
    let (mut sender, mut receiver) = socket.split();
    let tx;
    let mut rx;
    let db = db;

    let user = match get_user(auth.user_id, &db).await {
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

    match state.lobbies.read().await.get(&game_id) {
        Some(lobby_state) => {
            tx = lobby_state.sender.clone();
            rx = tx.subscribe();
        }
        None => todo!(),
    }

    let mut send_task = tokio::spawn(async move {
        while let Ok(event_msg) = rx.recv().await {
            let message = match event_msg {
                EventMessages::NewUserConnected(l) => ServerMessage::NewUserConnected(l),
                EventMessages::LobbyUpdate(u) => ServerMessage::LobbyUpdate(u),
                EventMessages::UserDisconnected(l) => ServerMessage::UserDisconnected(l),
                EventMessages::GameStart(u) => ServerMessage::GameStart(u),
                EventMessages::RoundStart(s) => ServerMessage::RoundStart(s),
                EventMessages::KickAll => ServerMessage::KickAll,
                EventMessages::GameEnd(ge) => ServerMessage::GameEnd(ge),
                EventMessages::Ack(id) => {
                    if id != user.id {
                        continue;
                    }

                    ServerMessage::Ack
                }
                EventMessages::ErrorUser(id, e) => {
                    if id != user.id {
                        continue;
                    }
                    ServerMessage::Error(e)
                }
                EventMessages::Error(e) => ServerMessage::Error(e),
                EventMessages::GameEventSettingsChange(s) => {
                    ServerMessage::GameEventSettingsChange(s)
                }
                EventMessages::GameEventResourceAddedAll(s, v) => {
                    ServerMessage::GameEventResource(s, v)
                }
                EventMessages::GameEventResourceAddedUser(id, s, v) => {
                    if id != user.id {
                        continue;
                    }
                    ServerMessage::GameEventResource(s, v)
                }
                EventMessages::GameEventPopUpUser(id, s) => {
                    if id != user.id {
                        continue;
                    }
                    ServerMessage::GameEventPopUp(s)
                }
                EventMessages::GameEventPopUpAll(s) => ServerMessage::GameEventPopUp(s),
                EventMessages::RoundEnd => ServerMessage::RoundFinish,
                EventMessages::UpdateClasses(c) => ServerMessage::UpdateClasses(c),
                EventMessages::Ping(m) => ServerMessage::Ping(m),
                EventMessages::Pong(m) => ServerMessage::Pong(m),
            };

            send_msg(&mut sender, message).await;
        }
    });

    let mut recv_task = tokio::spawn(async move {
        while let Some(result_msg) = receiver.next().await {
            match result_msg {
                Ok(msg) => match msg {
                    Message::Item(i) => {
                        match process_user_msg(game_id, user.id, i, &state, &db).await {
                            Ok(_) => tracing::info!("processed user info: {}", game_id),
                            Err(e) => {
                                let res = send_broadcast_msg(
                                    &state,
                                    game_id,
                                    EventMessages::ErrorUser(user.id, e),
                                )
                                .await;

                                if let Err(_) = res {
                                    tracing::error!("cos sie zesralo");
                                }
                            }
                        };
                    }
                    Message::Ping(m) => { 
                        match send_broadcast_msg(&state, game_id, EventMessages::Ping(m)).await {
                            Ok(()) => tracing::info!("disconnect  {}", user.id),
                            Err(e) => tracing::error!("error while receiving client  {}", e.to_string())
                        };
                    },
                    Message::Pong(m) => {
                        match send_broadcast_msg(&state, game_id, EventMessages::Pong(m)).await {
                            Ok(()) => tracing::info!("disconnect  {}", user.id),
                            Err(e) => tracing::error!("error while receiving client  {}", e.to_string())
                        };
                    },
                    Message::Close(_) => { 
                        match disconnect_user(user.id, &db, &state).await {
                            Ok(_) => tracing::info!("disconnect  {}", user.id),
                            Err(e) => tracing::error!("error while receiving client  {}", e.to_string())
                        }

                        break;
                    },
                },
                Err(e) => {
                    tracing::error!("p: {} error while receiving client  {}", user.username, e.to_string());
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

async fn send_msg(
    sender: &mut SplitSink<WebSocket<ServerMessage, ClientMessage>, Message<ServerMessage>>,
    message: ServerMessage,
) {
    tracing::debug!("sending websocket msg: {:?}", message);
    if let Err(e) = sender.send(Message::Item(message)).await {
        send_err(
            sender.borrow_mut(),
            AppError::InternalServerError(e.to_string()),
        )
        .await;
    }
}

async fn process_user_msg(
    game_id: Uuid,
    player: Uuid,
    msg: ClientMessage,
    state: &Arc<State>,
    db: &PgPool,
) -> Result<(), AppError> {
    tracing::info!("Processing user msg {:?}", msg);
    match msg {
        ClientMessage::Error(e) => todo!(),
        ClientMessage::RoundEnd(m) => {
            process_user_round_end_message(game_id, player, m, state.clone(), db).await
        }
        ClientMessage::UpdateClasses(c) => {
            update_lobby_classes(state, game_id, c).await
        }
    }
}
