use axum::{
    body::Body,
    http::{Request, StatusCode},
};

use sqlx::PgPool;
use std::{process::id, str};

use tower::Service;
use tower::ServiceExt;
use uuid::Uuid;

use crate::{
    auth::{self, Auth, AuthPayload},
    common_tests::{
        authorize_admin, authorize_user, build_request, create_test_app, create_test_lobbies,
    },
    entities::{User, UserRole},
    lobby::lobby::LobbyResponse,
    user::user::{ConnectUser, CreateUser, UpdateUser},
};

#[sqlx::test(fixtures("users"))]
async fn test_get_lobby(db: PgPool) {
    let (mut app, state) = create_test_app(db.clone()).await;

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

    app.ready()
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
