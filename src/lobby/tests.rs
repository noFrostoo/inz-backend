use axum::http::StatusCode;

use sqlx::PgPool;
use std::str;

use tower::Service;
use tower::ServiceExt;
use uuid::Uuid;

use crate::{
    auth::AuthPayload,
    common_tests::{
        authorize_admin, authorize_user, build_request, create_test_app, create_test_lobbies,
    },
    entities::{GameEvents, Lobby, Settings, User, UserRole},
    lobby::lobby::{CreateLobby, LobbyResponse},
};

#[sqlx::test(fixtures("users"))]
async fn test_get_lobby(db: PgPool) {
    let (app, state) = create_test_app(db.clone()).await;

    let (auth, mut app) = authorize_user(app).await;

    // creates two lobbies, usage just to limit lines
    let (lobby_1, lobby_2) = create_test_lobbies(
        db.clone(),
        state.clone(),
        "alice",
        "51b374f1-93ae-4c5c-89dd-611bda8412ce",
    )
    .await;

    let opt: Option<&AuthPayload> = None;

    let response = app
        .ready()
        .await
        .unwrap()
        .call(build_request(
            "GET",
            format!("/lobby/{}", lobby_2.id.to_string(),).as_str(),
            opt,
            Some(&auth),
        ))
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{:?}",
        str::from_utf8(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..]).unwrap()
    );

    let lobby_response: LobbyResponse =
        serde_json::from_slice(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..])
            .unwrap();

    let mut admin_user = User {
        id: Uuid::parse_str("51b374f1-93ae-4c5c-89dd-611bda8412ce").unwrap(),
        username: "alice".to_string(),
        password: "$argon2id$v=19$m=4096,t=3,p=1$2dT4Yay43+XevGqR+xFSow$hb2/4PMw0RFg2AH/5zHPEXl9oDDM5+qsbcU2qfR2GE8".to_string(),
        game_id: None,
        role: UserRole::Admin };

    let assert_response = LobbyResponse {
        lobby: lobby_2.clone(),
        players: vec![],
        owner: admin_user.clone(),
    };
    assert_eq!(lobby_response, assert_response);

    let response = app
        .ready()
        .await
        .unwrap()
        .call(build_request(
            "PUT",
            format!(
                "/users/51b374f1-93ae-4c5c-89dd-611bda8412ce/connect?game_id={}&password={}",
                lobby_1.id.to_string().as_str(),
                "temp"
            )
            .as_str(),
            opt,
            Some(&auth),
        ))
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{:?}",
        str::from_utf8(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..]).unwrap()
    );

    let response = app
        .ready()
        .await
        .unwrap()
        .call(build_request(
            "GET",
            format!("/lobby/{}", lobby_1.id.to_string(),).as_str(),
            opt,
            Some(&auth),
        ))
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{:?}",
        str::from_utf8(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..]).unwrap()
    );

    let lobby_response: LobbyResponse =
        serde_json::from_slice(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..])
            .unwrap();

    admin_user.game_id = Some(lobby_1.id);

    let assert_response = LobbyResponse {
        lobby: lobby_1.clone(),
        players: vec![admin_user.clone()],
        owner: admin_user,
    };

    assert_eq!(lobby_response, assert_response);
}

#[sqlx::test(fixtures("users"))]
async fn test_create_lobby(db: PgPool) {
    let (app, state) = create_test_app(db.clone()).await;

    let (auth, mut app) = authorize_admin(app).await;

    let response = app
        .ready()
        .await
        .unwrap()
        .call(build_request(
            "POST",
            "/lobby",
            Some(&CreateLobby {
                name: "lobby-1".to_string(),
                password: Some("XD".to_string()),
                public: true,
                generate_connect_code: false,
                code_use_times: 0,
                max_players: 5,
                settings: None,
                events: None,
            }),
            Some(&auth),
        ))
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{:?}",
        str::from_utf8(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..]).unwrap()
    );

    let returned_lobby: LobbyResponse =
        serde_json::from_slice(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..])
            .unwrap();

    let lobby = sqlx::query_as!(Lobby,
        // language=PostgreSQL
        r#"select id, name, password, public, connect_code, code_use_times, max_players, started, owner_id, settings as "settings: sqlx::types::Json<Settings>", events as "events: sqlx::types::Json<GameEvents>" from "lobby" where id = $1"#,
        returned_lobby.lobby.id
    )
    .fetch_one(&db)
    .await
    .unwrap();

    assert_eq!(lobby, returned_lobby.lobby);

    assert!(state
        .lobbies
        .read()
        .await
        .get(&returned_lobby.lobby.id)
        .is_some())
}

#[sqlx::test(fixtures("users"))]
async fn test_get_public_private_all_lobbies(db: PgPool) {
    let (app, state) = create_test_app(db.clone()).await;

    let (auth, mut app) = authorize_admin(app).await;

    let (_, _) = create_test_lobbies(
        db.clone(),
        state,
        "alice",
        "51b374f1-93ae-4c5c-89dd-611bda8412ce",
    )
    .await;

    let opt: Option<&AuthPayload> = None;

    let response = app
        .ready()
        .await
        .unwrap()
        .call(build_request(
            "GET",
            "/lobby?lobby_type=Public",
            opt,
            Some(&auth),
        ))
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{:?}",
        str::from_utf8(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..]).unwrap()
    );

    let returned_lobby: Vec<Lobby> =
        serde_json::from_slice(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..])
            .unwrap();

    let lobbies = sqlx::query_as!(Lobby,
        // language=PostgreSQL
        r#"select id, name, password, public, connect_code, code_use_times, max_players, started, owner_id, settings as "settings: sqlx::types::Json<Settings>", events as "events: sqlx::types::Json<GameEvents>" from "lobby" where public = true"#,
    )
    .fetch_all(&db)
    .await
    .unwrap();

    assert_eq!(lobbies, returned_lobby);

    let response = app
        .ready()
        .await
        .unwrap()
        .call(build_request(
            "GET",
            "/lobby?lobby_type=Private",
            opt,
            Some(&auth),
        ))
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{:?}",
        str::from_utf8(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..]).unwrap()
    );

    let returned_lobby: Vec<Lobby> =
        serde_json::from_slice(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..])
            .unwrap();

    let lobbies = sqlx::query_as!(Lobby,
        // language=PostgreSQL
        r#"select id, name, password, public, connect_code, code_use_times, max_players, started, owner_id, settings as "settings: sqlx::types::Json<Settings>", events as "events: sqlx::types::Json<GameEvents>" from "lobby" where public = false"#,
    )
    .fetch_all(&db)
    .await
    .unwrap();

    assert_eq!(lobbies, returned_lobby);

    let response = app
        .ready()
        .await
        .unwrap()
        .call(build_request(
            "GET",
            "/lobby?lobby_type=All",
            opt,
            Some(&auth),
        ))
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{:?}",
        str::from_utf8(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..]).unwrap()
    );

    let returned_lobby: Vec<Lobby> =
        serde_json::from_slice(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..])
            .unwrap();

    let lobbies = sqlx::query_as!(Lobby,
        // language=PostgreSQL
        r#"select id, name, password, public, connect_code, code_use_times, max_players, started, owner_id, settings as "settings: sqlx::types::Json<Settings>", events as "events: sqlx::types::Json<GameEvents>" from "lobby""#,
    )
    .fetch_all(&db)
    .await
    .unwrap();

    assert_eq!(lobbies, returned_lobby);
}

#[sqlx::test(fixtures("users"))]
async fn test_delete_lobby(db: PgPool) {
    let (app, state) = create_test_app(db.clone()).await;

    let (auth, mut app) = authorize_admin(app).await;

    let (lobby_1, _) = create_test_lobbies(
        db.clone(),
        state.clone(),
        "alice",
        "51b374f1-93ae-4c5c-89dd-611bda8412ce",
    )
    .await;

    let opt: Option<&AuthPayload> = None;

    let response = app
        .ready()
        .await
        .unwrap()
        .call(build_request(
            "DELETE",
            format!("/lobby/{}", lobby_1.id.to_string().as_str()).as_str(),
            opt,
            Some(&auth),
        ))
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{:?}",
        str::from_utf8(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..]).unwrap()
    );

    let lobbies = sqlx::query_as!(Lobby,
        // language=PostgreSQL
        r#"select id, name, password, public, connect_code, code_use_times, max_players, started, owner_id, settings as "settings: sqlx::types::Json<Settings>", events as "events: sqlx::types::Json<GameEvents>" from "lobby""#,
    )
    .fetch_all(&db)
    .await
    .unwrap();

    assert_eq!(lobbies.len(), 1);

    assert_eq!(state.lobbies.read().await.len(), 1);
}
