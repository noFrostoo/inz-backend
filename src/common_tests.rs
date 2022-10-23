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

use crate::{
    auth::{AuthBody, AuthPayload},
    create_app, State,
};

pub async fn create_test_app(db: PgPool) -> (Router, Arc<State>) {
    let state = Arc::new(State {
        lobbies: RwLock::new(HashMap::new()),
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
