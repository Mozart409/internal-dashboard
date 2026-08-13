//! Proves the test harness itself works: an isolated database per test, the
//! real router served over a real socket.

mod common;

use sqlx::PgPool;

#[sqlx::test]
async fn app_serves_the_dashboard(pool: PgPool) {
    let app = common::spawn(pool).await;

    let (status, body) = app.get("/").await;

    assert!(status.is_success(), "GET / returned {status}");
    assert!(
        body.contains("id=\"link-list\""),
        "dashboard should render the link list container"
    );
}

#[sqlx::test]
async fn each_test_gets_an_empty_database(pool: PgPool) {
    let app = common::spawn(pool).await;

    let (status, body) = app.get("/api/v1/links").await;

    assert!(status.is_success());
    assert_eq!(body, "[]", "a fresh test database should have no links");
}

#[sqlx::test]
async fn migrations_are_applied_to_the_test_database(pool: PgPool) {
    // If #[sqlx::test] did not run ./migrations, this query would error.
    let count: i64 = sqlx::query_scalar("select count(*) from links")
        .fetch_one(&pool)
        .await
        .expect("links table should exist in the test database");

    assert_eq!(count, 0);
}
