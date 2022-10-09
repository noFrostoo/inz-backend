mod entities;
mod error;
mod user;
mod lobby;
mod websocets;
mod auth;

use axum::{
    routing::{get, post, put},
    Router,
    extract::{Extension}, response::IntoResponse,
};
use lobby::{LobbyUserUpdate, LobbyUserUpdateResponse};
use tokio::sync;
use uuid::Uuid;
use std::{net::SocketAddr, env, collections::{HashMap}, sync::{Arc, Mutex, RwLock}};
use once_cell::sync::Lazy;
use tower::ServiceBuilder;
use sqlx::{postgres::PgPoolOptions, PgPool};
use axum_typed_websockets::{WebSocket, WebSocketUpgrade};
use websocets::{EventMessages, ClientMessage, process_message, ServerMessage};
use argon2::{
    password_hash::{
        rand_core::OsRng,
        PasswordHash, PasswordHasher, PasswordVerifier, SaltString
    },
    Argon2
};

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::{user::{create_user, get_users, get_user, delete_user, update_user, connect_user, disconnect_user, get_me}, auth::{Keys, authorize}};
use crate::lobby::{create_lobby, get_lobbies, get_lobby, delete_lobby};

pub struct LobbyState {
    //TODO: not nice
    sender: sync::broadcast::Sender<axum_typed_websockets::Message<EventMessages>>
}

pub struct State {
    lobbies: RwLock<HashMap<Uuid, LobbyState>>,
}

static KEYS: Lazy<Keys> = Lazy::new(|| {
    let secret = std::env::var("JWT_SECRET").expect("JWT_SECRET must be set");
    Keys::new(secret.as_bytes())
});

#[tokio::main]
async fn main(){
    dotenv::dotenv().ok();

    tracing_subscriber::registry()
    .with(tracing_subscriber::EnvFilter::new(
        std::env::var("RUST_LOG").unwrap_or_else(|_| "inz=trace".into()),
    ))
    .with(tracing_subscriber::fmt::layer())
    .init();

    let db = PgPoolOptions::new()
    .max_connections(50)
    .connect(env::var("DATABASE_URL").unwrap().as_str())
    .await
    .expect("could not connect to db");

    sqlx::migrate!().run(&db).await.unwrap();

    let state = Arc::new(State{ lobbies: RwLock::new(HashMap::new()) });

    let app = Router::new()
        .route("/", get(root))
        .route("/users", post(create_user).get(get_users))
        .route("/users/:id", get(get_user).delete(delete_user).put(update_user))
        .route("/users/:id/connect", put(connect_user))
        .route("/users/:id/disconnect", put(disconnect_user))
        .route("/users/me", get(get_me))
        .route("/lobby", post(create_lobby).get(get_lobbies))
        .route("/lobby/:id", get(get_lobby).delete(delete_lobby))
        .route("/websocket", get(websocket_handler))
        .route("/authorize", post(authorize))
        .layer(
            ServiceBuilder::new()
                .layer(Extension(db))
                .layer(Extension(state))
                //can add cookie managment here
        );

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::debug!("listening on {}", addr);

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn root() -> &'static str {
    "Hello, World!"
}

async fn websocket_handler(
    ws: WebSocketUpgrade<ServerMessage, ClientMessage>,
    Extension(state): Extension<Arc<State>>,
    Extension(ref db): Extension<PgPool>
) -> impl IntoResponse {
    let db_clone = db.clone();
    ws.on_upgrade(|socket| process_message(socket, state, db_clone))
}