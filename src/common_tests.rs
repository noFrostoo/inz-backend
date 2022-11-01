use axum::{body::Body, http::Request, Router};
use serde::Serialize;
use sqlx::PgPool;
use std::str;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};
use tower::Service;
use tower::ServiceExt;
use uuid::Uuid;

use crate::auth::AuthGameAdmin;
use crate::entities::{GameEvents, Lobby, Settings};
use crate::lobby::lobby::{create_lobby, CreateLobby};
use crate::{
    auth::{AuthBody, AuthPayload},
    create_app, State,
};

pub async fn create_test_app(db: PgPool) -> (Router, Arc<State>) {
    let state = Arc::new(State {
        lobbies: tokio::sync::RwLock::new(HashMap::new()),
    });

    (create_app(db, state.clone()), state)
}

pub async fn authorize_admin(mut app: Router) -> (AuthBody, Router) {
    let response = app
        .ready()
        .await
        .unwrap()
        .call(build_request(
            "POST",
            "/authorize",
            Some(&AuthPayload {
                password: "alice".to_string(),
                username: "alice".to_string(),
            }),
            None,
        ))
        .await
        .unwrap();

    (
        serde_json::from_slice(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..])
            .unwrap(),
        app,
    )
}

pub async fn authorize_user(mut app: Router) -> (AuthBody, Router) {
    let response = app
        .ready()
        .await
        .unwrap()
        .call(build_request(
            "POST",
            "/authorize",
            Some(&AuthPayload {
                password: "bob".to_string(),
                username: "bob".to_string(),
            }),
            None,
        ))
        .await
        .unwrap();

    (
        serde_json::from_slice(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..])
            .unwrap(),
        app,
    )
}

pub fn build_request<T>(
    method: &str,
    uri: &str,
    body: Option<&T>,
    auth: Option<&AuthBody>,
) -> Request<Body>
where
    T: Serialize,
{
    let request_body = match body {
        Some(s) => Body::from(serde_json::to_string(s).unwrap()),
        None => Body::empty(),
    };

    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header("Content-Type", "application/json");

    let req = match auth {
        Some(auth_body) => req.header(
            "Authorization",
            format!("Bearer {}", auth_body.access_token),
        ),
        None => req,
    };

    req.body(request_body).unwrap()
}

pub async fn create_test_lobbies(
    db: PgPool,
    state: Arc<State>,
    user: &str,
    user_id: &str,
) -> (Lobby, Lobby) {
    let mut tx = db.begin().await.unwrap();

    let lobby_params = CreateLobby {
        name: "temp-1".to_string(),
        password: Some("temp".to_string()),
        public: true,
        generate_connect_code: false,
        code_use_times: 0,
        max_players: 3,
        settings: Some(Settings::default()),
        events: Some(GameEvents::new()),
    };

    let lobby_1 = create_lobby(
        &mut tx,
        lobby_params,
        state.clone(),
        AuthGameAdmin {
            username: user.to_string(),
            user_id: Uuid::parse_str(user_id).unwrap(),
            role: crate::entities::UserRole::Admin,
            exp: 20000000,
        },
    )
    .await
    .unwrap();

    let lobby_params = CreateLobby {
        name: "temp-2".to_string(),
        password: None,
        public: false,
        generate_connect_code: true,
        code_use_times: 2,
        max_players: 3,
        settings: Some(Settings::default()),
        events: Some(GameEvents::new()),
    };

    let lobby_2 = create_lobby(
        &mut tx,
        lobby_params,
        state,
        AuthGameAdmin {
            username: user.to_string(),
            user_id: Uuid::parse_str(user_id).unwrap(),
            role: crate::entities::UserRole::Admin,
            exp: 20000000,
        },
    )
    .await
    .unwrap();

    tx.commit().await.unwrap();
    (lobby_1, lobby_2)
}
