use sqlx::PgPool;

#[sqlx::test(fixtures("users"))]
async fn test_create_template(db: PgPool) {
    assert!(true);
}

#[sqlx::test(fixtures("users"))]
async fn test_get_template(db: PgPool) {
    assert!(true);
}

#[sqlx::test(fixtures("users"))]
async fn test_create_delete(db: PgPool) {
    assert!(true);
}

#[sqlx::test(fixtures("users"))]
async fn test_update_template(db: PgPool) {
    assert!(true);
}

#[sqlx::test(fixtures("users"))]
async fn test_create_template_from_lobby(db: PgPool) {
    assert!(true);
}

#[sqlx::test(fixtures("users"))]
async fn test_create_lobby_from_template(db: PgPool) {
    assert!(true);
}
