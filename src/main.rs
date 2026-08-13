mod api;
mod db;
mod error;
mod events;
mod mcp;
mod models;
mod sse;
mod ui;

use std::time::Duration;

use sqlx::postgres::PgPoolOptions;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_scalar::{Scalar, Servable as _};
use utoipa_swagger_ui::SwaggerUi;

use crate::events::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                EnvFilter::new("internal_dashboard=debug,tower_http=debug,info")
            }),
        )
        .init();

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://dashboard:dashboard@localhost:5433/dashboard".into());
    let bind_addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".into());

    let pool = PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&database_url)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    let state = AppState::new(pool);

    // Handlers register themselves into the spec, so the docs cannot drift
    // from the routes.
    let (api_router, openapi) = OpenApiRouter::with_openapi(api::ApiDoc::openapi())
        .nest("/api/v1", api::router())
        .split_for_parts();

    let app = axum::Router::new()
        .merge(ui::router())
        .merge(sse::router())
        .merge(api_router)
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", openapi.clone()))
        .merge(Scalar::with_url("/scalar", openapi))
        .nest_service("/mcp", mcp::service(state.clone()))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    tracing::info!("dashboard   http://{bind_addr}/");
    tracing::info!("scalar      http://{bind_addr}/scalar");
    tracing::info!("swagger-ui  http://{bind_addr}/swagger-ui");
    tracing::info!("mcp         http://{bind_addr}/mcp");
    axum::serve(listener, app).await?;

    Ok(())
}
