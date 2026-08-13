//! Shared test harness.
//!
//! Every integration test drives the app over real HTTP against the real
//! router from `internal_dashboard::build_router`. Nothing here reconstructs
//! routing — reconstructing it would defeat the point, since several of the
//! bugs these tests guard against lived in router composition itself.

// Each integration test binary links this module and uses a different subset of
// it, so unused helpers are expected per-binary.
#![allow(dead_code)]

use internal_dashboard::build_router;
use internal_dashboard::events::AppState;
use internal_dashboard::models::Link;
use sqlx::PgPool;

pub struct TestApp {
    /// Base URL, e.g. `http://127.0.0.1:45321` — no trailing slash.
    pub addr: String,
    pub client: reqwest::Client,
    pub pool: PgPool,
}

/// Serve the real router on an ephemeral port.
///
/// Pair with `#[sqlx::test]`, which hands each test its own migrated database:
///
/// ```ignore
/// #[sqlx::test]
/// async fn my_test(pool: PgPool) {
///     let app = common::spawn(pool).await;
///     let res = app.client.get(app.url("/")).send().await.unwrap();
///     assert!(res.status().is_success());
/// }
/// ```
pub async fn spawn(pool: PgPool) -> TestApp {
    let state = AppState::new(pool.clone());
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("server");
    });

    TestApp {
        addr: format!("http://{addr}"),
        client: reqwest::Client::new(),
        pool,
    }
}

impl TestApp {
    /// Absolute URL for a path, e.g. `app.url("/api/v1/links")`.
    #[must_use]
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.addr)
    }

    /// Create a link through the public JSON API and return it.
    /// Used as setup by tests whose subject is something other than creation.
    pub async fn create_link(&self, url: &str, title: &str, tags: &[&str]) -> Link {
        let res = self
            .client
            .post(self.url("/api/v1/links"))
            .json(&serde_json::json!({ "url": url, "title": title, "tags": tags }))
            .send()
            .await
            .expect("create link request");

        assert_eq!(
            res.status(),
            reqwest::StatusCode::CREATED,
            "setup: creating {title} should return 201"
        );

        res.json().await.expect("decode created link")
    }

    /// GET a path and return (status, body).
    pub async fn get(&self, path: &str) -> (reqwest::StatusCode, String) {
        let res = self
            .client
            .get(self.url(path))
            .send()
            .await
            .expect("GET request");
        let status = res.status();
        let body = res.text().await.expect("read body");
        (status, body)
    }
}
