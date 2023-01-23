use axum::{
    body::Body,
    http::{Request, StatusCode},
};

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
    entities::{User, UserRole},
    user::user::{CreateUser, UpdateUser},
};

#[sqlx::test(fixtures("users"))]
async fn test_hello_world(db: PgPool) {
    let (app, _) = create_test_app(db).await;

    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    assert_eq!(&body[..], b"Hello, World!");
}

#[sqlx::test(fixtures("users"))]
async fn test_authorize(db: PgPool) {
    let (app, _) = create_test_app(db).await;

    let response = app
        .oneshot(build_request(
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

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{:?}",
        str::from_utf8(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..]).unwrap()
    );
}

#[sqlx::test(fixtures("users"))]
async fn test_authorize_bad(db: PgPool) {
    let (app, _) = create_test_app(db).await;

    let response = app
        .oneshot(build_request(
            "POST",
            "/authorize",
            Some(&AuthPayload {
                password: "alice".to_string(),
                username: "alice2".to_string(),
            }),
            None,
        ))
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "{:?}",
        str::from_utf8(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..]).unwrap()
    );
}

#[sqlx::test(fixtures("users"))]
async fn test_register_username_taken(db: PgPool) {
    let (app, _) = create_test_app(db).await;

    let response = app
        .oneshot(build_request(
            "POST",
            "/register",
            Some(&CreateUser {
                password: "alice".to_string(),
                username: "alice".to_string(),
                role: UserRole::User,
            }),
            None,
        ))
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "{:?}",
        str::from_utf8(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..]).unwrap()
    );
}

#[sqlx::test(fixtures("users"))]
async fn test_register_empty_data(db: PgPool) {
    let (app, _) = create_test_app(db).await;

    let response = app
        .oneshot(build_request(
            "POST",
            "/register",
            Some(&CreateUser {
                password: "".to_string(),
                username: "".to_string(),
                role: UserRole::User,
            }),
            None,
        ))
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "{:?}",
        str::from_utf8(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..]).unwrap()
    );
}

#[sqlx::test(fixtures("users"))]
async fn test_register_bad_role(db: PgPool) {
    let (app, _) = create_test_app(db).await;

    let response = app
        .oneshot(build_request(
            "POST",
            "/register",
            Some(&CreateUser {
                password: "alice2".to_string(),
                username: "alice2".to_string(),
                role: UserRole::Admin,
            }),
            None,
        ))
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "{:?}",
        str::from_utf8(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..]).unwrap()
    );
}

#[sqlx::test(fixtures("users"))]
async fn test_get_user(db: PgPool) {
    let (app, _) = create_test_app(db).await;

    let opt: Option<&AuthPayload> = None;

    let (auth, app) = authorize_admin(app).await;

    let response = app
        .oneshot(build_request(
            "GET",
            "/users/c994b839-84f4-4509-ad49-59119133d6f5",
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

    let resp_user: User =
        serde_json::from_slice(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..])
            .unwrap();

    let correct_user = User{
        id: Uuid::parse_str("c994b839-84f4-4509-ad49-59119133d6f5").unwrap(), 
        username: "bob".to_string(), 
        password: "$argon2id$v=19$m=4096,t=3,p=1$/6XXIkFwpibpEe4sq8Qs4w$UG575rlLgt0THTBSsFrynPm/hpy7F1xzJ4DdpZ47mYc".to_string(), 
        role: UserRole::User,
        game_id: None};

    assert_eq!(resp_user, correct_user);
}

#[sqlx::test(fixtures("users"))]
async fn test_get_users(db: PgPool) {
    let (app, _) = create_test_app(db).await;

    let opt: Option<&AuthPayload> = None;

    let (auth, app) = authorize_admin(app).await;

    let response = app
        .oneshot(build_request("GET", "/users", opt, Some(&auth)))
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{:?}",
        str::from_utf8(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..]).unwrap()
    );
}

#[sqlx::test(fixtures("users"))]
async fn test_user_create(db: PgPool) {
    let (app, _) = create_test_app(db).await;

    let (auth, mut app) = authorize_admin(app).await;

    let response = app
        .ready()
        .await
        .unwrap()
        .call(build_request(
            "POST",
            "/users",
            Some(&CreateUser {
                password: "user".to_string(),
                username: "user".to_string(),
                role: UserRole::Admin,
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

    let resp_user: User =
        serde_json::from_slice(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..])
            .unwrap();

    assert_eq!(resp_user.role, UserRole::Admin);
    assert_eq!(resp_user.username, "user");

    let opt: Option<&AuthPayload> = None;

    let response = app
        .oneshot(build_request(
            "GET",
            format!("/users/{}", resp_user.id).as_str(),
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

    let resp_user2: User =
        serde_json::from_slice(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..])
            .unwrap();

    assert_eq!(resp_user, resp_user2)
}

#[sqlx::test(fixtures("users"))]
async fn test_get_create_bad_role(db: PgPool) {
    let (app, _) = create_test_app(db).await;

    let (auth, app) = authorize_user(app).await;

    let response = app
        .oneshot(build_request(
            "POST",
            "/users",
            Some(&CreateUser {
                password: "user".to_string(),
                username: "user".to_string(),
                role: UserRole::Admin,
            }),
            Some(&auth),
        ))
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "{:?}",
        str::from_utf8(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..]).unwrap()
    );
}

#[sqlx::test(fixtures("users"))]
async fn test_delete_user(db: PgPool) {
    let (app, _) = create_test_app(db).await;

    let opt: Option<&AuthPayload> = None;

    let (auth, mut app) = authorize_admin(app).await;

    let response = app
        .ready()
        .await
        .unwrap()
        .call(build_request(
            "DELETE",
            "/users/c994b839-84f4-4509-ad49-59119133d6f5",
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
            "/users/c994b839-84f4-4509-ad49-59119133d6f5",
            opt,
            Some(&auth),
        ))
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "{:?}",
        str::from_utf8(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..]).unwrap()
    );
}

#[sqlx::test(fixtures("users"))]
async fn test_get_delete_bad_role(db: PgPool) {
    let (app, _) = create_test_app(db).await;

    let opt: Option<&AuthPayload> = None;

    let (auth, mut app) = authorize_user(app).await;

    let response = app
        .ready()
        .await
        .unwrap()
        .call(build_request(
            "DELETE",
            "/users/51b374f1-93ae-4c5c-89dd-611bda8412ce",
            opt,
            Some(&auth),
        ))
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "{:?}",
        str::from_utf8(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..]).unwrap()
    );

    let response = app
        .ready()
        .await
        .unwrap()
        .call(build_request(
            "GET",
            "/users/51b374f1-93ae-4c5c-89dd-611bda8412ce",
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
}

#[sqlx::test(fixtures("users"))]
async fn test_get_delete_self(db: PgPool) {
    let (app, _) = create_test_app(db).await;

    let opt: Option<&AuthPayload> = None;

    let (auth, mut app) = authorize_user(app).await;

    let response = app
        .ready()
        .await
        .unwrap()
        .call(build_request(
            "DELETE",
            "/users/c994b839-84f4-4509-ad49-59119133d6f5",
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
            "/users/c994b839-84f4-4509-ad49-59119133d6f5",
            opt,
            Some(&auth),
        ))
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "{:?}",
        str::from_utf8(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..]).unwrap()
    );
}

#[sqlx::test(fixtures("users"))]
async fn test_get_update_self(db: PgPool) {
    let (app, _) = create_test_app(db).await;

    let (auth, mut app) = authorize_user(app).await;

    let response = app
        .ready()
        .await
        .unwrap()
        .call(build_request(
            "PUT",
            "/users/c994b839-84f4-4509-ad49-59119133d6f5",
            Some(&UpdateUser {
                password: "bob2".to_string(),
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

    let response = app
        .ready()
        .await
        .unwrap()
        .call(build_request(
            "POST",
            "/authorize",
            Some(&AuthPayload {
                password: "bob2".to_string(),
                username: "bob".to_string(),
            }),
            None,
        ))
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{:?}",
        str::from_utf8(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..]).unwrap()
    );
}

#[sqlx::test(fixtures("users"))]
async fn test_get_update_not_self_as_user(db: PgPool) {
    let (app, _) = create_test_app(db).await;

    let (auth, mut app) = authorize_user(app).await;

    let response = app
        .ready()
        .await
        .unwrap()
        .call(build_request(
            "PUT",
            "/users/51b374f1-93ae-4c5c-89dd-611bda8412ce",
            Some(&UpdateUser {
                password: "bob2".to_string(),
            }),
            Some(&auth),
        ))
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "{:?}",
        str::from_utf8(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..]).unwrap()
    );
}

#[sqlx::test(fixtures("users"))]
async fn test_get_update_not_self_as_admin(db: PgPool) {
    let (app, _) = create_test_app(db).await;

    let (auth, mut app) = authorize_admin(app).await;

    let response = app
        .ready()
        .await
        .unwrap()
        .call(build_request(
            "PUT",
            "/users/c994b839-84f4-4509-ad49-59119133d6f5",
            Some(&UpdateUser {
                password: "bob2".to_string(),
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
}

#[sqlx::test(fixtures("users"))]
async fn test_connect(db: PgPool) {
    let (app, state) = create_test_app(db.clone()).await;

    let (auth, mut app) = authorize_admin(app).await;

    let (lobby_1, _) = create_test_lobbies(
        db.clone(),
        state.clone(),
        "alice",
        "51b374f1-93ae-4c5c-89dd-611bda8412ce",
    )
    .await;

    assert!(state.lobbies.read().await.get(&lobby_1.id).is_some());

    let opt: Option<&AuthPayload> = None;

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

    assert_eq!(
        state
            .lobbies
            .read()
            .await
            .get(&lobby_1.id)
            .unwrap()
            ._receiver
            .len(),
        1
    );

    let count = sqlx::query_scalar!(
        // language=PostgreSQL
        r#"select count(*) from "user" where game_id = $1"#,
        lobby_1.id
    )
    .fetch_one(&db)
    .await
    .unwrap()
    .unwrap();

    assert_eq!(count, 1);
}

#[sqlx::test(fixtures("users"))]
async fn test_connect_no_pass(db: PgPool) {
    let (app, state) = create_test_app(db.clone()).await;

    let (auth, mut app) = authorize_admin(app).await;

    let (lobby_1, _) = create_test_lobbies(
        db,
        state.clone(),
        "alice",
        "51b374f1-93ae-4c5c-89dd-611bda8412ce",
    )
    .await;

    assert!(state.lobbies.read().await.get(&lobby_1.id).is_some());

    let opt: Option<&AuthPayload> = None;

    let response = app
        .ready()
        .await
        .unwrap()
        .call(build_request(
            "PUT",
            format!(
                "/users/51b374f1-93ae-4c5c-89dd-611bda8412ce/connect?game_id={}",
                lobby_1.id.to_string().as_str(),
            )
            .as_str(),
            opt,
            Some(&auth),
        ))
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "{:?}",
        str::from_utf8(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..]).unwrap()
    );
}

#[sqlx::test(fixtures("users"))]
async fn test_connect_max_user_limit(db: PgPool) {
    let (app, state) = create_test_app(db.clone()).await;

    let (auth, mut app) = authorize_admin(app).await;

    let (lobby_1, _) = create_test_lobbies(
        db.clone(),
        state.clone(),
        "alice",
        "51b374f1-93ae-4c5c-89dd-611bda8412ce",
    )
    .await;

    assert!(state.lobbies.read().await.get(&lobby_1.id).is_some());

    let opt: Option<&AuthPayload> = None;

    let ids_to_connect = vec![
        "51b374f1-93ae-4c5c-89dd-611bda8412ce",
        "c994b839-84f4-4509-ad49-59429133d6f5",
        "b994b839-84f4-4509-ad49-59429133d6f5",
        "d994b839-84f4-4509-ad49-59429133d6f5",
    ];
    let ids_len = ids_to_connect.len();

    let mut count = 0;

    for id in ids_to_connect {
        let response = app
            .ready()
            .await
            .unwrap()
            .call(build_request(
                "PUT",
                format!(
                    "/users/{}/connect?game_id={}&password={}",
                    id,
                    lobby_1.id.to_string().as_str(),
                    "temp".to_string()
                )
                .as_str(),
                opt,
                Some(&auth),
            ))
            .await
            .unwrap();

        count += 1;

        if count != ids_len {
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "{:?}",
                str::from_utf8(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..])
                    .unwrap()
            );
        } else {
            assert_eq!(
                response.status(),
                StatusCode::BAD_REQUEST,
                "{:?}",
                str::from_utf8(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..])
                    .unwrap()
            );
        }
    }

    let count = sqlx::query_scalar!(
        // language=PostgreSQL
        r#"select count(*) from "user" where game_id = $1"#,
        lobby_1.id
    )
    .fetch_one(&db)
    .await
    .unwrap()
    .unwrap();

    assert_eq!(count, 3);
}

#[sqlx::test(fixtures("users"))]
async fn test_connect_user_connected(db: PgPool) {
    let (app, state) = create_test_app(db.clone()).await;

    let (auth, mut app) = authorize_admin(app).await;

    let (lobby_1, _) = create_test_lobbies(
        db,
        state.clone(),
        "alice",
        "51b374f1-93ae-4c5c-89dd-611bda8412ce",
    )
    .await;

    assert!(state.lobbies.read().await.get(&lobby_1.id).is_some());

    let opt: Option<&AuthPayload> = None;

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
        StatusCode::BAD_REQUEST,
        "{:?}",
        str::from_utf8(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..]).unwrap()
    );
}

#[sqlx::test(fixtures("users"))]
async fn test_quick_connect(db: PgPool) {
    let (app, state) = create_test_app(db.clone()).await;

    let (auth, mut app) = authorize_admin(app).await;

    let (_, mut lobby_2) = create_test_lobbies(
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
            "PUT",
            format!(
                "/users/quick_connect?connect_code={}",
                lobby_2.connect_code.clone().unwrap().to_string(),
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

    lobby_2.code_use_times -= 1;

    let lobby_response =
        serde_json::from_slice(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..])
            .unwrap();

    assert_eq!(lobby_2, lobby_response,);

    assert_eq!(
        state
            .lobbies
            .read()
            .await
            .get(&lobby_2.id)
            .unwrap()
            ._receiver
            .len(),
        1
    );

    let count = sqlx::query_scalar!(
        // language=PostgreSQL
        r#"select count(*) from "user" where game_id = $1"#,
        lobby_2.id
    )
    .fetch_one(&db)
    .await
    .unwrap()
    .unwrap();

    assert_eq!(count, 1);
}

#[sqlx::test(fixtures("users"))]
async fn test_quick_connect_temp_user(db: PgPool) {
    let (app, state) = create_test_app(db.clone()).await;

    let (_, mut app) = authorize_admin(app).await;

    let (_, mut lobby_2) = create_test_lobbies(
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
            "PUT",
            format!(
                "/quick_connect?connect_code={}",
                lobby_2.connect_code.clone().unwrap().to_string(),
            )
            .as_str(),
            opt,
            None,
        ))
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{:?}",
        str::from_utf8(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..]).unwrap()
    );

    lobby_2.code_use_times -= 1;

    let lobby_response =
        serde_json::from_slice(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..])
            .unwrap();

    assert_eq!(lobby_2, lobby_response,);

    assert_eq!(
        state
            .lobbies
            .read()
            .await
            .get(&lobby_2.id)
            .unwrap()
            ._receiver
            .len(),
        1
    );

    let count = sqlx::query_scalar!(
        // language=PostgreSQL
        r#"select count(*) from "user" where game_id = $1"#,
        lobby_2.id
    )
    .fetch_one(&db)
    .await
    .unwrap()
    .unwrap();

    assert_eq!(count, 1);

    let count = sqlx::query_scalar!(
        // language=PostgreSQL
        r#"select count(*) from "user" "#,
    )
    .fetch_one(&db)
    .await
    .unwrap()
    .unwrap();

    assert_eq!(count, 7);
}

// TODO:
// #[sqlx::test(fixtures("users"))]
// async fn test_disconnect(db: PgPool) {
//     let (app, state) = create_test_app(db.clone()).await;

//     let (auth, mut app) = authorize_admin(app).await;

//     let (lobby_1, _) =
//         create_test_lobbies(db, state.clone(), "alice", "51b374f1-93ae-4c5c-89dd-611bda8412ce").await;

//     let opt: Option<&AuthPayload> = None;

//     let response = app
//         .ready()
//         .await
//         .unwrap()
//         .call(build_request(
//             "PUT",
//             format!(
//                 "/users/51b374f1-93ae-4c5c-89dd-611bda8412ce/connect?game_id={}&password={}",
//                 lobby_1.id.to_string().as_str(),
//                 "temp"
//             )
//             .as_str(),
//             opt,
//             Some(&auth),
//         ))
//         .await
//         .unwrap();

//     assert_eq!(
//         response.status(),
//         StatusCode::OK,
//         "{:?}",
//         str::from_utf8(&hyper::body::to_bytes(response.into_body()).await.unwrap()[..]).unwrap()
//     );

// }
