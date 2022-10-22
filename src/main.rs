mod auth;
mod entities;
mod error;
mod game;
mod lobby;
mod template;
mod user;
mod websockets;

use auth::Auth;
use axum::{
    extract::Extension,
    response::IntoResponse,
    routing::{get, post, put},
    Router,
};
use axum_typed_websockets::WebSocketUpgrade;
use once_cell::sync::Lazy;
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::{
    collections::HashMap,
    env,
    net::SocketAddr,
    sync::{Arc, RwLock},
};
use tokio::sync;
use tower::ServiceBuilder;
use uuid::Uuid;
use websockets::{process_message, ClientMessage, EventMessages, ServerMessage};

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::{
    auth::{authorize_endpoint, Keys},
    lobby::{
        lobby::get_lobbies_endpoint,
        lobby_endpoints::{
            create_lobby_endpoint, delete_lobby_endpoint, get_lobby_endpoint, start_game_endpoint,
            update_lobby_endpoint,
        },
    },
    template::{create_lobby_from_template, create_template_from_lobby_endpoint},
    user::user_endpoints::{
        connect_user_endpoint, create_user_endpoint, delete_user_endpoint,
        disconnect_user_endpoint, get_me_endpoint, get_user_endpoint, get_users_endpoint,
        quick_connect_endpoint, quick_connect_endpoint_no_user, register_endpoint,
        update_user_endpoint,
    },
};

use crate::template::{
    create_template_endpoint, delete_template_endpoint, get_template_endpoint,
    get_templates_endpoint, update_template_endpoint,
};

pub struct LobbyState {
    sender: sync::broadcast::Sender<EventMessages>,
}

pub struct State {
    lobbies: RwLock<HashMap<Uuid, LobbyState>>,
}

static KEYS: Lazy<Keys> = Lazy::new(|| {
    let secret = std::env::var("JWT_SECRET").expect("JWT_SECRET must be set");
    Keys::new(secret.as_bytes())
});

#[tokio::main]
async fn main() {
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

    let state = Arc::new(State {
        lobbies: RwLock::new(HashMap::new()),
    });

    let app = Router::new()
        .route("/", get(root))
        .route("/users", post(create_user_endpoint).get(get_users_endpoint))
        .route(
            "/users/:id",
            get(get_user_endpoint)
                .delete(delete_user_endpoint)
                .put(update_user_endpoint),
        )
        .route("/users/:id/connect", put(connect_user_endpoint))
        .route("/users/:id/disconnect", put(disconnect_user_endpoint))
        .route("/users/me", get(get_me_endpoint))
        .route("/users/me/quick_connect", put(quick_connect_endpoint))
        .route("/users/quick_connect", put(quick_connect_endpoint_no_user))
        .route(
            "/lobby",
            post(create_lobby_endpoint).get(get_lobbies_endpoint),
        )
        .route(
            "/lobby/:id",
            get(get_lobby_endpoint)
                .delete(delete_lobby_endpoint)
                .put(update_lobby_endpoint),
        )
        .route("/lobby/:id/start", post(start_game_endpoint))
        .route("/lobby/websocket", get(websocket_handler))
        .route(
            "/template",
            post(create_template_endpoint).get(get_templates_endpoint),
        )
        .route(
            "/template/from_lobby",
            post(create_template_from_lobby_endpoint),
        )
        .route(
            "/template/:id",
            get(get_template_endpoint)
                .delete(delete_template_endpoint)
                .put(update_template_endpoint),
        )
        .route(
            "/template/:id/lobby_create",
            post(create_lobby_from_template),
        )
        .route("/authorize", post(authorize_endpoint))
        .route("/register", post(register_endpoint))
        .layer(
            ServiceBuilder::new()
                .layer(Extension(db))
                .layer(Extension(state)), //can add cookie management here
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
    Extension(ref db): Extension<PgPool>,
    auth: Auth,
) -> impl IntoResponse {
    let db_clone = db.clone();
    ws.on_upgrade(|socket| process_message(socket, state, db_clone, auth))
}
