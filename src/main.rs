mod auth;
#[cfg(test)]
mod common_tests;
mod entities;
mod error;
mod lobby;
mod template;
mod user;
mod websockets;

use auth::{Auth, WebSocketAuth};
use axum::{
    extract::Extension,
    response::IntoResponse,
    routing::{get, post, put},
    Router,
};
use axum_server::tls_rustls::RustlsConfig;
use axum_typed_websockets::WebSocketUpgrade;
use entities::{Flow, Order, Settings, UserState, Lobby, GameState};
use hyper::{Method, header};
use lobby::{
    lobby_endpoints::{get_lobbies_endpoint, stop_game_endpoint},
    stats::{game_stats, players_stats},
};
use once_cell::sync::Lazy;
use sqlx::{postgres::PgPoolOptions, PgPool, QueryBuilder};
use std::{
    collections::{BTreeMap, HashMap},
    env,
    net::SocketAddr,
    sync::Arc, path::PathBuf,
};
use tokio::sync;
use tower::ServiceBuilder;
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;
use websockets::{game_process, ClientMessage, EventMessages, ServerMessage};

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::{
    auth::{authorize_endpoint, Keys},
    lobby::lobby_endpoints::{
        create_lobby_endpoint, delete_lobby_endpoint, get_lobby_endpoint, start_game_endpoint,
        update_lobby_endpoint,
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

#[derive(Debug)]
pub struct LobbyState {
    sender: Arc<sync::broadcast::Sender<EventMessages>>,
    _receiver: Arc<sync::broadcast::Receiver<EventMessages>>,
    started: bool,
    round_state: RoundState,
}

#[derive(Debug)]
pub struct RoundState {
    round: i64,
    players: i64,
    players_finished: i64,
    users_states: BTreeMap<Uuid, UserState>,
    round_orders: BTreeMap<Uuid, Order>,
    send_orders: BTreeMap<Uuid, Order>,
    player_classes: BTreeMap<Uuid, u32>,
    settings: Settings,
    flow: Flow,
    demand: i64,
}

impl RoundState {
    pub fn new() -> Self {
        Self {
            round: 0,
            players: 0,
            players_finished: 0,
            users_states: BTreeMap::new(),
            round_orders: BTreeMap::new(),
            send_orders: BTreeMap::new(),
            player_classes: BTreeMap::new(),
            settings: Settings::default(),
            flow: Flow::default(),
            demand: 0,
        }
    }
}

pub struct State {
    lobbies: tokio::sync::RwLock<HashMap<Uuid, LobbyState>>,
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
        lobbies: tokio::sync::RwLock::new(HashMap::new()),
    });

    restore_lobbies(&state, &db).await;

    let config = RustlsConfig::from_pem_file(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("certs")
            .join("cert.pem"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("certs")
            .join("key.pem"),
    )
    .await
    .unwrap();

    let app = create_app(db, state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::debug!("listening on {}", addr);

    axum_server::bind_rustls(addr, config)
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
    auth: WebSocketAuth,
) -> impl IntoResponse {
    let db_clone = db.clone();
    let token = auth.token.clone();
    let mut r = ws.map(|w| {w.protocols([header::SEC_WEBSOCKET_PROTOCOL.to_string()])}).on_upgrade(|socket| game_process(socket, state, db_clone, auth)).into_response();
    r.headers_mut().insert(header::SEC_WEBSOCKET_PROTOCOL, token.parse().unwrap());
    print!("{:?}", r);
    r
}

pub fn create_app(db: PgPool, state: Arc<State>) -> Router {
    let cors = CorsLayer::new()
        // allow `GET` and `POST` when accessing the resource
        .allow_methods(vec![Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers(Any)
        // allow requests from any origin
        .allow_origin(Any);

    Router::new()
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
        .route("/users/quick_connect", put(quick_connect_endpoint))
        .route("/quick_connect", put(quick_connect_endpoint_no_user))
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
        .route("/lobby/:id/stop", post(stop_game_endpoint))
        .route("/lobby/:id/stats/game/", get(game_stats))
        .route("/lobby/:id/stats/users/", get(players_stats))
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
                .layer(Extension(state))
                .layer(cors), //can add cookie management here
        )
}

async fn restore_lobbies(state: &Arc<State>, db: &PgPool) {
    let mut builder = QueryBuilder::new("select * from \"lobby\" ");

    let query = builder.build_query_as::<Lobby>();

    let lobbies = match query
        .fetch_all(db)
        .await {
            Ok(l) => l,
            Err(e) => panic!("couldn't get lobbies {}", e),
        }
    ;
    for lobby in lobbies {
        print!("{}", lobby.id);
        if lobby.started {
            let game_state = sqlx::query_as!(GameState,
                r#"
                    select id, round, user_states as "user_states: sqlx::types::Json<BTreeMap<Uuid, UserState>>", round_orders as "round_orders: sqlx::types::Json<BTreeMap<Uuid, Order>>", flow as "flow: sqlx::types::Json<Flow>", demand, send_orders as "send_orders: sqlx::types::Json<BTreeMap<Uuid, Order>>", players_classes as "players_classes: sqlx::types::Json<BTreeMap<Uuid, u32>>", game_id
                    from "game_state"
                    where game_id = $1"#,
                    lobby.id,
            ).fetch_one(db)
            .await
            .unwrap();

            let (tx, rx) = sync::broadcast::channel(33);
            state.lobbies.write().await.insert(lobby.id, LobbyState { 
                sender: Arc::new(tx), 
                _receiver: Arc::new(rx), 
                started: false, 
                round_state: RoundState{
                    round: game_state.round,
                    players: game_state.players_classes.0.len() as i64,
                    players_finished: game_state.players_classes.0.len() as i64,
                    users_states: game_state.user_states.0,
                    round_orders: game_state.round_orders.0,
                    send_orders: game_state.send_orders.0,
                    player_classes: game_state.players_classes.0,
                    settings: lobby.settings.0,
                    flow: game_state.flow.0,
                    demand: game_state.demand,
                }
            });
        } else {
            //TODO: magic number fix
            let (tx, rx) = sync::broadcast::channel(33);
            state.lobbies.write().await.insert(lobby.id, LobbyState { 
                sender: Arc::new(tx), 
                _receiver: Arc::new(rx), 
                started: false, 
                round_state: RoundState::new()
            });
        }
    }
}