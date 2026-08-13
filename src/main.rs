//! Process entry point: configuration, pool, migrations, serve.
//! All routing lives in [`internal_dashboard::build_router`].

use std::time::Duration;

use internal_dashboard::{build_router, events::AppState};
use sqlx::postgres::PgPoolOptions;
use tracing_subscriber::EnvFilter;

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

    let app = build_router(AppState::new(pool));

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    tracing::info!("dashboard   http://{bind_addr}/");
    tracing::info!("scalar      http://{bind_addr}/scalar");
    tracing::info!("swagger-ui  http://{bind_addr}/swagger-ui");
    tracing::info!("mcp         http://{bind_addr}/mcp");
    axum::serve(listener, app).await?;

    Ok(())
}
