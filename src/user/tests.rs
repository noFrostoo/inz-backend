use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
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

use crate::{
    auth::{AuthBody, AuthPayload},
    common_tests::{authorize_admin, authorize_user, build_request, create_test_app},
    create_app,
    entities::{User, UserRole},
    user::user::{CreateUser, UpdateUser},
    State,
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
async fn test_get_create(db: PgPool) {
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
                role: UserRole::GameAdmin,
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

    assert_eq!(resp_user.role, UserRole::GameAdmin);
    assert_eq!(resp_user.username, "user");

    let opt: Option<&AuthPayload> = None;

    let response = app
        .oneshot(build_request(
            "GET",
            &*format!("/users/{}", resp_user.id.to_string()),
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
                role: UserRole::GameAdmin,
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
async fn test_get_delete(db: PgPool) {
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
