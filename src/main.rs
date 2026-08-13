//! Process entry point: configuration, pool, migrations, serve.
//! All routing lives in [`internal_dashboard::build_router`].

use std::{fmt::Display, str::FromStr, time::Duration};

use internal_dashboard::{build_router, events::AppState};
use sqlx::postgres::PgPoolOptions;
use tracing_subscriber::EnvFilter;

/// Parse a numeric setting, falling back to `default` when it is unset.
///
/// A malformed value is a hard error rather than a silent fallback: a typo in a
/// unit file should stop the service loudly instead of quietly running with a
/// pool a tenth of the intended size. A non-UTF-8 value reads as unset.
fn setting<T>(key: &str, raw: Option<&str>, default: T) -> anyhow::Result<T>
where
    T: FromStr,
    <T as FromStr>::Err: Display,
{
    match raw {
        None => Ok(default),
        Some(value) => value
            .parse()
            .map_err(|e| anyhow::anyhow!("{key}={value:?} is not a valid value: {e}")),
    }
}

/// Read a numeric setting straight from the environment.
fn env_setting<T>(key: &str, default: T) -> anyhow::Result<T>
where
    T: FromStr,
    <T as FromStr>::Err: Display,
{
    setting(key, std::env::var(key).ok().as_deref(), default)
}

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

    let max_connections = env_setting("DB_MAX_CONNECTIONS", 10u32)?;
    let acquire_timeout = env_setting("DB_ACQUIRE_TIMEOUT_SECS", 5u64)?;

    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(acquire_timeout))
        .connect(&database_url)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    let app = build_router(AppState::new(pool));

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    tracing::info!("dashboard   http://{bind_addr}/");
    tracing::info!("scalar      http://{bind_addr}/scalar");
    tracing::info!("mcp         http://{bind_addr}/mcp");
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::setting;

    #[test]
    fn an_unset_setting_falls_back_to_the_default() {
        assert_eq!(setting("DB_MAX_CONNECTIONS", None, 10u32).unwrap(), 10);
    }

    #[test]
    fn a_valid_setting_overrides_the_default() {
        assert_eq!(
            setting("DB_MAX_CONNECTIONS", Some("42"), 10u32).unwrap(),
            42
        );
    }

    #[test]
    fn a_malformed_setting_is_an_error_naming_the_variable() {
        let err = setting("DB_MAX_CONNECTIONS", Some("plenty"), 10u32)
            .expect_err("a non-numeric pool size must not fall back to the default");
        let message = err.to_string();
        assert!(
            message.contains("DB_MAX_CONNECTIONS") && message.contains("plenty"),
            "the error must name the variable and the offending value, got {message:?}"
        );
    }
}
