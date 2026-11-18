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
use internal_dashboard::events::{self, AppState};
use internal_dashboard::models::Link;
use sqlx::PgPool;

pub struct TestApp {
    /// Base URL, e.g. `http://127.0.0.1:45321` — no trailing slash.
    pub addr: String,
    pub client: reqwest::Client,
    pub pool: PgPool,
    /// The state the router was built from; cancel `state.shutdown` (or call
    /// [`TestApp::shutdown`]) to start a graceful shutdown.
    pub state: AppState,
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
///
/// Calling it twice with `pool.clone()` gives two replicas on one database,
/// each with its own listener, broadcast channel and shutdown token.
pub async fn spawn(pool: PgPool) -> TestApp {
    let state = AppState::new(pool.clone());
    events::listen(&state, (*pool.connect_options()).clone())
        .await
        .expect("listen for link events");
    let app = build_router(state.clone());

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
        state,
    }
}

impl Drop for TestApp {
    /// End this replica's listener task (and its own connection) with the test.
    fn drop(&mut self) {
        self.state.shutdown.cancel();
    }
}

impl TestApp {
    /// Start this replica's shutdown, as SIGTERM does in the binary.
    pub fn shutdown(&self) {
        self.state.shutdown.cancel();
    }

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
